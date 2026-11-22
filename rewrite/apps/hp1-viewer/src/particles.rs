//! Draws the world's particle systems as camera-facing quads.

use std::collections::HashMap;

use hp1_data::ClassTable;
use hp1_data::level::{Actor, LevelFile};
use hp1_engine::camera::View;
use hp1_engine::particles::{Blend as ParticleBlend, Settings};
use hp1_engine::props::ActorProps;
use hp1_engine::world::World;
use hp1_render::{Blend, DrawBatch, Vertex};

use crate::actors::Built;
use crate::textures::Textures;

pub struct ParticleDraw {
    /// Renderer index by lower-case texture path.
    textures: HashMap<String, u32>,
    /// Texture sizes, for sprites.
    sizes: HashMap<String, (f32, f32)>,
    /// Classes drawn as sprites (`DrawType` sprite, shown, with a real texture), by lower-case
    /// name. The engine's own icons (`Engine.S_…`) are editor-only and left out.
    sprite_classes: std::collections::HashSet<String>,
}

impl ParticleDraw {
    /// Loads the textures of every particle class and of the level's own systems up front, as
    /// systems can be spawned at any time.
    pub fn load(level: &LevelFile, classes: &ClassTable, textures: &mut Textures) -> Self {
        let mut paths = Vec::new();
        for class in classes.classes.keys().filter(|c| classes.is_a(c, "Engine.ParticleFX")) {
            let actor =
                Actor { name: class.clone(), class: class.clone(), props: Default::default(), brush: None };
            paths.extend(Settings::from_props(&ActorProps::new(&actor, classes)).textures);
        }
        for a in level.actors.iter().flatten().filter(|a| classes.is_a(&a.class, "Engine.ParticleFX")) {
            paths.extend(Settings::from_props(&ActorProps::new(a, classes)).textures);
        }
        let mut sprite_classes = std::collections::HashSet::new();
        for class in classes.classes.keys().filter(|c| !classes.is_a(c, "Engine.ParticleFX")) {
            let actor =
                Actor { name: class.clone(), class: class.clone(), props: Default::default(), brush: None };
            let p = ActorProps::new(&actor, classes);
            let texture = p.text("Texture");
            if p.byte("DrawType", 1) == 1
                && !p.bool("bHidden")
                && !texture.is_empty()
                && !texture.to_ascii_lowercase().starts_with("engine.s_")
            {
                sprite_classes.insert(class.to_ascii_lowercase());
                paths.push(texture);
            }
        }
        let mut sizes = HashMap::new();
        for p in &paths {
            if let Some(t) = crate::movers::texture_size(textures, p) {
                sizes.insert(p.to_ascii_lowercase(), (t.width as f32, t.height as f32));
            }
        }
        let textures = paths
            .into_iter()
            .map(|p| {
                let i = textures.get(&p);
                (p.to_ascii_lowercase(), i)
            })
            .filter(|&(_, i)| i != 0)
            .collect();
        Self { textures, sizes, sprite_classes }
    }

    pub fn build(&self, world: &World, view: &View) -> Built {
        let mut by_batch: HashMap<(u32, u8), Vec<u32>> = HashMap::new();
        let mut vertices = Vec::new();
        for (&id, system) in &world.particles {
            let Some(e) = world.get(id) else { continue };
            if e.hidden {
                continue;
            }
            let blend = match system.settings.blend {
                ParticleBlend::Masked => Blend::Masked,
                ParticleBlend::Translucent => Blend::Translucent,
                ParticleBlend::Modulated => Blend::Modulated,
            };
            for s in system.sprites(e.pos, view) {
                let Some(&tex) = self.textures.get(&s.texture.to_ascii_lowercase()) else { continue };
                let first = vertices.len() as u32;
                for (corner, uv) in s.corners.iter().zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]) {
                    vertices.push(Vertex { pos: (*corner).into(), uv, lm_uv: [-1.0, -1.0], color: s.color });
                }
                by_batch.entry((tex, blend as u8)).or_default().extend([0, 1, 2, 0, 2, 3].map(|i| first + i));
            }
        }
        self.sprites(world, view, &mut vertices, &mut by_batch);
        let mut built = Built { vertices, ..Default::default() };
        // Darkening (modulated) ones first, so they don't darken the bright ones in front.
        let mut keys: Vec<_> = by_batch.keys().copied().collect();
        keys.sort_by_key(|&(tex, blend)| (blend != Blend::Modulated as u8, blend, tex));
        for key in keys {
            let idx = &by_batch[&key];
            let blend = match key.1 {
                x if x == Blend::Masked as u8 => Blend::Masked,
                x if x == Blend::Modulated as u8 => Blend::Modulated,
                _ => Blend::Translucent,
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

    /// Sprite objects: their texture as a square facing the camera, `DrawScale` × its size.
    fn sprites(
        &self,
        world: &World,
        view: &View,
        vertices: &mut Vec<Vertex>,
        by_batch: &mut HashMap<(u32, u8), Vec<u32>>,
    ) {
        let forward = (view.target - view.pos).normalize_or_zero();
        let right = glam::Vec3::Z.cross(forward).normalize_or_zero();
        let up = forward.cross(right);
        for (id, e) in world.actors.iter().enumerate() {
            let Some(e) = e else { continue };
            if e.hidden || !self.sprite_classes.contains(&e.class.to_ascii_lowercase()) {
                continue;
            }
            let Some(a) = world.actor_data(id) else { continue };
            let p = ActorProps::new(a, &world.classes);
            let texture = p.text("Texture").to_ascii_lowercase();
            let (Some(&tex), Some(&(w, h))) = (self.textures.get(&texture), self.sizes.get(&texture)) else {
                continue;
            };
            let scale = p.f32("DrawScale", 1.0);
            let (side, along) = (right * w * scale * 0.5, up * h * scale * 0.5);
            let blend = match p.byte("Style", 1) {
                2 => Blend::Masked,
                3 => Blend::Translucent,
                4 => Blend::Modulated,
                _ => Blend::Masked,
            };
            let first = vertices.len() as u32;
            let corners =
                [e.pos - side + along, e.pos + side + along, e.pos + side - along, e.pos - side - along];
            for (corner, uv) in corners.iter().zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]) {
                vertices.push(Vertex { pos: (*corner).into(), uv, lm_uv: [-1.0, -1.0], color: [1.0; 3] });
            }
            by_batch.entry((tex, blend as u8)).or_default().extend([0, 1, 2, 0, 2, 3].map(|i| first + i));
        }
    }
}
