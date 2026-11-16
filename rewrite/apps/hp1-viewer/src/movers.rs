//! Brush objects that move (doors, lifts): their geometry, lit where they start, redrawn wherever
//! the world has moved them.

use glam::Vec3;
use hp1_data::ClassTable;
use hp1_data::level::LevelFile;
use hp1_engine::level_mesh::{LevelMesh, LightPacker, Style, TextureSize, build_model_mesh};
use hp1_engine::lighting::build_light_maps;
use hp1_engine::world::World;
use hp1_render::{Blend, DrawBatch, Vertex};

use crate::textures::Textures;

pub fn blend(style: Style) -> Blend {
    match style {
        Style::Opaque => Blend::Opaque,
        Style::Sky => Blend::Sky,
        Style::Masked => Blend::Masked,
        Style::Translucent => Blend::Translucent,
        Style::Modulated => Blend::Modulated,
    }
}

/// Turns a built mesh into the renderer's vertices, indices and batches.
pub fn to_draw(mesh: &LevelMesh, textures: &mut Textures) -> (Vec<Vertex>, Vec<u32>, Vec<DrawBatch>) {
    let mut indices = Vec::new();
    let mut batches = Vec::new();
    for b in &mesh.batches {
        let tex = b.texture.as_deref().map_or(0, |t| textures.get(t));
        batches.push(DrawBatch {
            texture: tex,
            lightmap: b.lightmap_page,
            blend: blend(b.style),
            first_index: indices.len() as u32,
            index_count: b.indices.len() as u32,
        });
        indices.extend_from_slice(&b.indices);
    }
    let vertices = mesh
        .vertices
        .iter()
        .map(|v| Vertex { pos: v.pos, uv: v.uv, lm_uv: v.lm_uv, color: [1.0; 3] })
        .collect();
    (vertices, indices, batches)
}

pub fn texture_size(textures: &Textures, path: &str) -> Option<TextureSize> {
    let t = textures.info(path)?;
    Some(TextureSize {
        width: t.width,
        height: t.height,
        draw_scale: textures.draw_scale(path),
        masked: t.masked,
    })
}

struct MoverMesh {
    actor: usize,
    /// In the brush's own space.
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    batches: Vec<DrawBatch>,
}

pub struct Movers {
    meshes: Vec<MoverMesh>,
    /// Pose each mesh was last drawn at, to skip rebuilding when nothing moved.
    drawn: Vec<(Vec3, [i32; 3])>,
}

impl Movers {
    /// Builds the geometry of every brush object; light maps go into `packer`.
    pub fn build(
        world: &World,
        level: &LevelFile,
        classes: &ClassTable,
        packer: &mut LightPacker,
        textures: &mut Textures,
    ) -> Self {
        let mut meshes = Vec::new();
        for (i, e) in world.actors.iter().enumerate() {
            let Some(e) = e else { continue };
            let Some(brush) = e.brush.as_ref().and_then(|(key, _)| level.brushes.get(key)) else { continue };
            if e.hidden || !classes.is_a(&e.class, "Engine.Mover") {
                continue;
            }
            let (r, t) = e.brush_to_world();
            let zone = hp1_engine::bsp::zone_at(&level.model, e.pos);
            let zone_actor = level.model.zones.get(zone).map(|z| z.actor);
            let maps = build_light_maps(&brush.model, level, classes, r, t, zone_actor);
            let mesh = build_model_mesh(&brush.model, &maps, packer, |p| texture_size(textures, p));
            let (vertices, indices, batches) = to_draw(&mesh, textures);
            meshes.push(MoverMesh { actor: i, vertices, indices, batches });
        }
        let drawn = vec![(Vec3::NAN, [0; 3]); meshes.len()];
        Self { meshes, drawn }
    }

    pub fn count(&self) -> usize {
        self.meshes.len()
    }

    /// All movers where the world has them now, or None if none moved since the last call.
    pub fn build_if_moved(&mut self, world: &World) -> Option<(Vec<Vertex>, Vec<u32>, Vec<DrawBatch>)> {
        let poses: Vec<_> = self
            .meshes
            .iter()
            .map(|m| world.get(m.actor).map_or((Vec3::ZERO, [0; 3]), |e| (e.pos, e.rot)))
            .collect();
        if poses == self.drawn {
            return None;
        }
        self.drawn = poses;
        let (mut vertices, mut indices, mut batches) = (Vec::new(), Vec::new(), Vec::new());
        for m in &self.meshes {
            let Some(e) = world.get(m.actor) else { continue };
            let (r, t) = e.brush_to_world();
            let base = vertices.len() as u32;
            let first = indices.len() as u32;
            vertices
                .extend(m.vertices.iter().map(|v| Vertex { pos: (r * Vec3::from(v.pos) + t).into(), ..*v }));
            indices.extend(m.indices.iter().map(|i| i + base));
            batches.extend(m.batches.iter().map(|b| DrawBatch { first_index: b.first_index + first, ..*b }));
        }
        Some((vertices, indices, batches))
    }
}
