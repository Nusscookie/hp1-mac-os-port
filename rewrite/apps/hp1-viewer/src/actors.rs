//! Placed objects drawn as meshes: props stand still, characters play their animation.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use glam::{Mat4, Vec3};
use hp1_data::ClassTable;
use hp1_data::level::LevelFile;
use hp1_data::mesh::{AnimFile, MeshBody, MeshFile, MeshIndexEntry, load_binary};
use hp1_engine::anim::{bone_links, bone_matrices, mesh_to_object, sample, skin, vertex_frame};
use hp1_engine::bsp::zone_at;
use hp1_engine::level_mesh::poly_flags;
use hp1_engine::lighting::{LightSource, level_lights, vertex_light, zone_ambient};
use hp1_engine::props::ActorProps;
use hp1_engine::units::rotator_matrix;
use hp1_engine::world::{Sequence, World};
use hp1_render::{Blend, DrawBatch, Vertex};

use crate::textures::Textures;

/// How an object is drawn (the `DrawType` setting).
const DRAW_MESH: i64 = 2;

#[derive(Clone)]
enum Motion {
    Still,
    Skeletal { anim: Arc<AnimFile>, links: Vec<Option<usize>>, seq: usize },
    Frames { start: u32, frames: u32, rate: f32 },
}

#[derive(Clone)]
struct Instance {
    mesh: Arc<MeshFile>,
    /// The world object it shows, once the world drives it (position, facing, animation).
    actor: Option<usize>,
    /// The class's PrePivot and DrawScale, for placing it where the world has it.
    fit: (Vec3, f32),
    /// Seconds into the animation chosen by the world, instead of free running.
    driven: Option<f32>,
    /// Moved by its behaviour in the world even without an animation of its own (beans).
    follows: bool,
    motion: Motion,
    to_world: Mat4,
    /// Per material: texture index and blend.
    materials: Vec<(u32, Blend)>,
    lights: Vec<LightSource>,
    ambient: Vec3,
    /// Full bright (no lighting).
    unlit: bool,
    /// Seconds offset so identical objects don't move in step.
    phase: f32,
}

pub struct Actors {
    still: Vec<Instance>,
    moving: Vec<Instance>,
    pub count: usize,
    pub skipped: usize,
    /// The player's character, posed by the program every frame.
    player: Option<Instance>,
    /// The player class's PrePivot and DrawScale.
    player_fit: (Vec3, f32),
    all_lights: Vec<LightSource>,
    /// Ready-made instances of classes that the level may spawn later.
    templates: HashMap<String, Instance>,
}

/// What loading an object's mesh needs; kept so the player can be loaded the same way.
struct Loader<'a> {
    data: &'a Path,
    index: HashMap<String, MeshIndexEntry>,
    meshes: HashMap<String, Option<Arc<MeshFile>>>,
    anims: HashMap<String, Option<Arc<AnimFile>>>,
}

impl Actors {
    pub fn moving_count(&self) -> usize {
        self.moving.len()
    }

    /// How long the animations of world object `id` are, if it is drawn with a skeletal mesh.
    pub fn sequences(&self, id: usize) -> Option<Vec<(String, Sequence)>> {
        self.moving.iter().find(|i| i.actor == Some(id)).and_then(lengths)
    }

    /// The same for the player's mesh.
    pub fn player_sequences(&self) -> Option<Vec<(String, Sequence)>> {
        self.player.as_ref().and_then(lengths)
    }

    /// Every world object drawn with an animated mesh.
    pub fn animated(&self) -> Vec<usize> {
        self.moving.iter().filter_map(|i| i.actor).collect()
    }

    /// Lets the world move every still object that has a behaviour there (beans, frogs).
    pub fn drive(&mut self, world: &World) {
        let (driven, still): (Vec<_>, Vec<_>) = std::mem::take(&mut self.still)
            .into_iter()
            .partition(|i| i.actor.and_then(|id| world.get(id)).is_some_and(|e| e.behaviour.is_some()));
        self.still = still;
        self.moving.extend(driven.into_iter().map(|mut i| {
            i.follows = true;
            i
        }));
    }

    /// Stops drawing world object `id` (the placed Harry, who is drawn as the player).
    pub fn forget(&mut self, id: usize) {
        self.moving.retain(|i| i.actor != Some(id));
    }
}

fn lengths(inst: &Instance) -> Option<Vec<(String, Sequence)>> {
    let Motion::Skeletal { anim, .. } = &inst.motion else { return None };
    Some(
        anim.sequences
            .iter()
            .map(|s| (s.info.name.clone(), Sequence { frames: s.info.frames, rate: s.info.rate }))
            .collect(),
    )
}

/// Mesh geometry ready for the renderer.
#[derive(Default)]
pub struct Built {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<DrawBatch>,
}

fn read_index(data: &Path) -> Result<HashMap<String, MeshIndexEntry>> {
    let mut out = HashMap::new();
    for e in std::fs::read_dir(data.join("meshes"))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "json") {
            let list: Vec<MeshIndexEntry> = serde_json::from_slice(&std::fs::read(&p)?)?;
            out.extend(list.into_iter().map(|m| (m.path.to_ascii_lowercase(), m)));
        }
    }
    Ok(out)
}

impl Actors {
    pub fn load(
        data: &Path,
        level: &LevelFile,
        classes: &ClassTable,
        textures: &mut Textures,
    ) -> Result<Self> {
        let mut loader =
            Loader { data, index: read_index(data)?, meshes: HashMap::new(), anims: HashMap::new() };
        let all_lights = level_lights(level, classes);
        let mut out = Actors {
            still: Vec::new(),
            moving: Vec::new(),
            count: 0,
            skipped: 0,
            player: None,
            player_fit: (Vec3::ZERO, 1.0),
            all_lights: Vec::new(),
            templates: HashMap::new(),
        };
        for (n, actor) in level.actors.iter().enumerate() {
            let Some(actor) = actor else { continue };
            let p = ActorProps::new(actor, classes);
            if p.int("DrawType", 1) != DRAW_MESH || p.bool("bHidden") || p.get("Mesh").is_none() {
                continue;
            }
            let Some(mut inst) = loader.instance(&p, level, classes, textures, &all_lights, n) else {
                out.skipped += 1;
                continue;
            };
            out.count += 1;
            inst.actor = Some(n);
            match inst.motion {
                Motion::Skeletal { .. } | Motion::Frames { frames: 2.., .. } => out.moving.push(inst),
                _ => out.still.push(inst),
            }
        }
        // Characters that triggers can spawn during play.
        let spawnable: std::collections::BTreeSet<String> = level
            .actors
            .iter()
            .flatten()
            .flat_map(|a| a.props.iter())
            .filter(|(k, _)| k.starts_with("BaseCharToSpawn"))
            .filter_map(|(_, v)| v.as_str().map(str::to_owned))
            .collect();
        for class in spawnable {
            let actor = hp1_data::level::Actor {
                name: class.clone(),
                class: class.clone(),
                props: Default::default(),
                brush: None,
            };
            let p = ActorProps::new(&actor, classes);
            if let Some(inst) = loader.instance(&p, level, classes, textures, &[], out.templates.len()) {
                out.templates.insert(class.to_ascii_lowercase(), inst);
            }
        }
        out.all_lights = all_lights;
        Ok(out)
    }

    /// Follows the world: shows newly spawned characters and puts every character the world
    /// drives where it is now, playing its animation. Objects the world removed disappear.
    pub fn follow(&mut self, world: &World, spawned: &[usize]) {
        for &id in spawned {
            let Some(a) = world.actor_data(id) else { continue };
            if let Some(t) = self.templates.get(&a.class.to_ascii_lowercase()) {
                let mut inst = t.clone();
                inst.actor = Some(id);
                self.moving.push(inst);
            }
        }
        let (level, classes) = (&world.level, &world.classes);
        let all_lights = &self.all_lights;
        self.moving.retain_mut(|inst| {
            let Some(id) = inst.actor else { return true };
            let Some(e) = world.get(id) else { return false };
            match &e.anim {
                Some(anim) => place(inst, level, classes, all_lights, e.pos, e.rot[1], &anim.name, anim.time),
                None if inst.follows => put(inst, level, classes, all_lights, e.pos, e.rot[1]),
                None => {}
            }
            true
        });
    }

    /// Loads the player's character (an object of `class` placed by code).
    pub fn load_player(
        &mut self,
        data: &Path,
        level: &LevelFile,
        classes: &ClassTable,
        textures: &mut Textures,
        class: &str,
    ) -> Result<bool> {
        let mut loader =
            Loader { data, index: read_index(data)?, meshes: HashMap::new(), anims: HashMap::new() };
        let actor = hp1_data::level::Actor {
            name: "Player".into(),
            class: class.into(),
            props: Default::default(),
            brush: None,
        };
        let p = ActorProps::new(&actor, classes);
        self.player_fit = (Vec3::from(p.vec3("PrePivot")), p.f32("DrawScale", 1.0));
        self.player = loader.instance(&p, level, classes, textures, &[], 0).map(|mut i| {
            i.phase = 0.0;
            i
        });
        Ok(self.player.is_some())
    }

    /// Places and poses the player: collision centre `pos`, facing `yaw` radians, playing the
    /// sequence `anim` at `time` seconds into it.
    pub fn pose_player(
        &mut self,
        level: &LevelFile,
        classes: &ClassTable,
        pos: Vec3,
        yaw: f32,
        anim: &str,
        time: f32,
    ) {
        let Some(p) = &mut self.player else { return };
        p.fit = self.player_fit;
        let yaw = (yaw / hp1_engine::units::ROT_TO_RAD) as i32;
        place(p, level, classes, &self.all_lights, pos, yaw, anim, time);
    }

    pub fn build_player(&self) -> Built {
        self.player.as_ref().map_or_else(Built::default, |p| build(std::slice::from_ref(p), 0.0))
    }

    pub fn build_still(&self) -> Built {
        build(&self.still, 0.0)
    }

    pub fn build_moving(&self, time: f32) -> Built {
        build(&self.moving, time)
    }
}

/// Puts an instance at collision centre `pos`, facing `yaw` (rotation units), playing the
/// sequence `anim` at `time` seconds into it.
#[allow(clippy::too_many_arguments)]
fn place(
    inst: &mut Instance,
    level: &LevelFile,
    classes: &ClassTable,
    all_lights: &[LightSource],
    pos: Vec3,
    yaw: i32,
    anim: &str,
    time: f32,
) {
    put(inst, level, classes, all_lights, pos, yaw);
    if let Motion::Skeletal { anim: file, seq, .. } = &mut inst.motion
        && let Some(i) = file.sequences.iter().position(|s| s.info.name.eq_ignore_ascii_case(anim))
    {
        *seq = i;
    }
    if !(inst.follows && anim.is_empty()) {
        inst.driven = Some(time);
    }
}

/// Puts an instance at collision centre `pos`, facing `yaw` (rotation units), lit from there.
fn put(
    inst: &mut Instance,
    level: &LevelFile,
    classes: &ClassTable,
    all_lights: &[LightSource],
    pos: Vec3,
    yaw: i32,
) {
    let (pivot, scale) = inst.fit;
    inst.to_world = Mat4::from_translation(pos + pivot)
        * rotator_matrix(0, yaw, 0)
        * Mat4::from_scale(Vec3::splat(scale))
        * mesh_to_object(&inst.mesh.transform);
    inst.lights =
        all_lights.iter().filter(|l| l.location.distance(pos) < l.radius + 100.0).copied().collect();
    inst.ambient = zone_ambient(level, classes, zone_at(&level.model, pos));
}

impl Loader<'_> {
    fn instance(
        &mut self,
        p: &ActorProps,
        level: &LevelFile,
        classes: &ClassTable,
        textures: &mut Textures,
        all_lights: &[LightSource],
        n: usize,
    ) -> Option<Instance> {
        let mesh_path = p.get("Mesh").and_then(|v| v.as_str()).map(str::to_owned)?;
        let entry = self.index.get(&mesh_path.to_ascii_lowercase()).cloned();
        let entry = entry.as_ref();
        let data = self.data;
        let mesh = self
            .meshes
            .entry(mesh_path.to_ascii_lowercase())
            .or_insert_with(|| {
                entry.and_then(|e| load_binary::<MeshFile>(&data.join(&e.file)).ok()).map(Arc::new)
            })
            .clone();
        let mesh = mesh?;

        let loc = Vec3::from(p.vec3("Location"));
        let [pitch, yaw, roll] = p.rotator("Rotation");
        let scale = p.f32("DrawScale", 1.0);
        let to_world = Mat4::from_translation(loc + Vec3::from(p.vec3("PrePivot")))
            * rotator_matrix(pitch, yaw, roll)
            * Mat4::from_scale(Vec3::splat(scale))
            * mesh_to_object(&mesh.transform);

        // Textures: the object's own per-slot skins, then the mesh's, then the object's skin.
        let style = p.byte("Style", 1);
        let skin = p.get("Skin").and_then(|v| v.as_str()).map(str::to_owned);
        let materials = mesh
            .materials
            .iter()
            .map(|m| {
                let slot = m.slot.max(0) as usize;
                let key = if slot == 0 { "MultiSkins".to_owned() } else { format!("MultiSkins[{slot}]") };
                let multi = p.get(&key).and_then(|v| v.as_str()).map(str::to_owned);
                let path = multi.or_else(|| m.texture.clone()).or_else(|| skin.clone());
                let tex = path.as_deref().map_or(0, |t| textures.get(t));
                let masked = path.as_deref().and_then(|t| textures.info(t)).is_some_and(|t| t.masked);
                let f = m.poly_flags;
                let blend = if style == 3 || f & poly_flags::TRANSLUCENT != 0 {
                    Blend::Translucent
                } else if style == 4 || f & poly_flags::MODULATED != 0 {
                    Blend::Modulated
                } else if style == 2 || masked || f & poly_flags::MASKED != 0 {
                    Blend::Masked
                } else {
                    Blend::Opaque
                };
                (tex, blend)
            })
            .collect();

        // Lights that can reach the object.
        let size = (Vec3::from(mesh.bounds_max) - Vec3::from(mesh.bounds_min)).length() * scale;
        let lights =
            all_lights.iter().filter(|l| l.location.distance(loc) < l.radius + size).copied().collect();
        let ambient = zone_ambient(level, classes, zone_at(&level.model, loc));

        let seq_name = p.get("AnimSequence").and_then(|v| v.as_str()).map(str::to_owned);
        let motion = match &mesh.body {
            MeshBody::Skeletal { bones, .. } => {
                let anim = entry.and_then(|e| e.animation_file.clone()).and_then(|f| {
                    self.anims
                        .entry(f.clone())
                        .or_insert_with(|| load_binary(&data.join(&f)).ok().map(Arc::new))
                        .clone()
                });
                match anim {
                    Some(anim) => {
                        // The map's sequence, else a standing-still one (behaviour code will
                        // choose animations later).
                        let find = |want: &str| {
                            anim.sequences.iter().position(|s| s.info.name.eq_ignore_ascii_case(want))
                        };
                        let seq = seq_name.as_deref().and_then(find).or_else(|| {
                            anim.sequences.iter().position(|s| {
                                let n = s.info.name.to_ascii_lowercase();
                                n.contains("breath") || n.contains("idle")
                            })
                        });
                        match seq {
                            Some(seq) => Motion::Skeletal { links: bone_links(bones, &anim), anim, seq },
                            None => Motion::Still,
                        }
                    }
                    None => Motion::Still,
                }
            }
            MeshBody::Vertex { sequences, .. } => {
                let seq = seq_name.and_then(|n| sequences.iter().find(|s| s.name.eq_ignore_ascii_case(&n)));
                match seq {
                    Some(s) if s.frames > 1 => {
                        Motion::Frames { start: s.start_frame, frames: s.frames, rate: s.rate.max(1.0) }
                    }
                    Some(s) => Motion::Frames { start: s.start_frame, frames: 1, rate: 1.0 },
                    None => Motion::Still,
                }
            }
        };
        Some(Instance {
            mesh,
            actor: None,
            fit: (Vec3::from(p.vec3("PrePivot")), scale),
            driven: None,
            follows: false,
            to_world,
            materials,
            lights,
            ambient,
            unlit: p.bool("bUnlit"),
            phase: (n as f32 * 0.37) % 1.0,
            motion,
        })
    }
}

fn points(inst: &Instance, time: f32) -> Vec<Vec3> {
    match (&inst.mesh.body, &inst.motion) {
        (MeshBody::Skeletal { bones, influences, .. }, Motion::Skeletal { anim, links, seq }) => {
            let s = &anim.sequences[*seq];
            let t = match inst.driven {
                _ if s.duration <= 0.0 => 0.0,
                Some(t) => t % s.duration,
                None => (time + inst.phase * s.duration) % s.duration,
            };
            skin(influences, &bone_matrices(bones, &sample(bones, links, s, t)))
        }
        (MeshBody::Skeletal { points, .. }, _) => points.iter().map(|p| Vec3::from(*p)).collect(),
        (MeshBody::Vertex { frame_points, positions, .. }, Motion::Frames { start, frames, rate }) => {
            let f = (time + inst.phase) * rate;
            let i = f.floor() as u32 % frames;
            vertex_frame(positions, *frame_points, start + i, start + (i + 1) % frames, f.fract())
        }
        (MeshBody::Vertex { frame_points, positions, .. }, _) => {
            vertex_frame(positions, *frame_points, 0, 0, 0.0)
        }
    }
}

fn build(list: &[Instance], time: f32) -> Built {
    let mut by_batch: HashMap<(u32, u8), Vec<u32>> = HashMap::new();
    let mut vertices = Vec::new();
    for inst in list {
        let pts: Vec<Vec3> =
            points(inst, time).into_iter().map(|p| inst.to_world.transform_point3(p)).collect();
        if pts.is_empty() {
            continue;
        }
        // Smooth normals from the triangles around each point.
        let mut normals = vec![Vec3::ZERO; pts.len()];
        for t in &inst.mesh.triangles {
            let [a, b, c] = t.vertices.map(|v| inst.mesh.vertices[v as usize].point as usize);
            if a.max(b).max(c) >= pts.len() {
                continue;
            }
            let n = (pts[b] - pts[a]).cross(pts[c] - pts[a]);
            for i in [a, b, c] {
                normals[i] += n;
            }
        }
        let colors: Vec<Vec3> = pts
            .iter()
            .zip(&normals)
            .map(|(p, n)| {
                if inst.unlit {
                    Vec3::ONE
                } else {
                    // The game's handedness flips the cross product, so either side may face out.
                    let n = n.normalize_or_zero();
                    let front = vertex_light(&inst.lights, inst.ambient, *p, n);
                    let back = vertex_light(&inst.lights, inst.ambient, *p, -n);
                    front.max(back) * 2.0
                }
            })
            .collect();
        let first = vertices.len() as u32;
        for v in &inst.mesh.vertices {
            let i = (v.point as usize).min(pts.len() - 1);
            vertices.push(Vertex {
                pos: pts[i].into(),
                uv: v.uv,
                lm_uv: [-1.0, -1.0],
                color: colors[i].into(),
            });
        }
        for t in &inst.mesh.triangles {
            let (tex, blend) = inst.materials.get(t.material as usize).copied().unwrap_or((0, Blend::Opaque));
            by_batch.entry((tex, blend as u8)).or_default().extend(t.vertices.map(|v| first + v));
        }
    }
    let mut built = Built { vertices, ..Default::default() };
    let mut keys: Vec<_> = by_batch.keys().copied().collect();
    keys.sort();
    for key in keys {
        let idx = &by_batch[&key];
        let blend = match key.1 {
            x if x == Blend::Masked as u8 => Blend::Masked,
            x if x == Blend::Translucent as u8 => Blend::Translucent,
            x if x == Blend::Modulated as u8 => Blend::Modulated,
            _ => Blend::Opaque,
        };
        built.batches.push(DrawBatch {
            texture: key.0,
            lightmap: 0,
            blend,
            first_index: built.indices.len() as u32,
            index_count: idx.len() as u32,
        });
        built.indices.extend_from_slice(idx);
    }
    built
}
