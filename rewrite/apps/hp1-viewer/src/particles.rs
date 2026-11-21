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
        let textures = paths
            .into_iter()
            .map(|p| {
                let i = textures.get(&p);
                (p.to_ascii_lowercase(), i)
            })
            .filter(|&(_, i)| i != 0)
            .collect();
        Self { textures }
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
        let mut built = Built { vertices, ..Default::default() };
        let mut keys: Vec<_> = by_batch.keys().copied().collect();
        keys.sort();
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
}
