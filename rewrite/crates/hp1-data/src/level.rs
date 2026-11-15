//! Levels: BSP geometry with zones and light-map data, brush models, and every placed object.
//!
//! References between objects inside a level are actor indices (`-1` = none).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::PropMap;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Plane `x·n = w` as `[nx, ny, nz, w]`.
    pub plane: [f32; 4],
    pub zone_mask: u64,
    pub flags: u8,
    /// First entry in `Model::verts`; `num_vertices` follow.
    pub vert_pool: i32,
    pub surf: i32,
    pub back: i32,
    pub front: i32,
    /// Next node on the same plane (coplanar polygons).
    pub plane_next: i32,
    pub collision_bound: i32,
    pub render_bound: i32,
    pub zone: [i32; 2],
    pub num_vertices: u8,
    pub leaf: [i32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub texture: Option<String>,
    pub poly_flags: u32,
    /// Indices into `points` / `vectors`.
    pub base: i32,
    pub normal: i32,
    pub texture_u: i32,
    pub texture_v: i32,
    pub light_map: i32,
    pub brush_poly: i32,
    pub pan_u: i16,
    pub pan_v: i16,
    pub brush_actor: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vert {
    pub point: i32,
    pub side: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoneEntry {
    pub actor: i32,
    pub connectivity: u64,
    pub visibility: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightMap {
    /// Byte offset into `Model::light_bits`.
    pub data_offset: i32,
    pub pan: [f32; 3],
    pub u_clamp: i32,
    pub v_clamp: i32,
    pub u_scale: f32,
    pub v_scale: f32,
    /// First index into `Model::lights` (the list continues until a `-1`), or -1.
    pub first_light: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LeafEntry {
    pub zone: i32,
    pub permeating: i32,
    pub volumetric: i32,
    pub visible_zones: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub vectors: Vec<[f32; 3]>,
    pub points: Vec<[f32; 3]>,
    pub nodes: Vec<Node>,
    pub surfaces: Vec<Surface>,
    pub verts: Vec<Vert>,
    pub zones: Vec<ZoneEntry>,
    pub light_maps: Vec<LightMap>,
    /// Shadow masks, one bit per light-map texel per light (rows padded to whole bytes).
    pub light_bits: Vec<u8>,
    pub bounds: Vec<Bound>,
    pub leaf_hulls: Vec<i32>,
    pub leaves: Vec<LeafEntry>,
    /// Actor indices of lights, lists separated by -1.
    pub lights: Vec<i32>,
    pub root_outside: bool,
    pub linked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Poly {
    pub base: [f32; 3],
    pub normal: [f32; 3],
    pub texture_u: [f32; 3],
    pub texture_v: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
    pub flags: u32,
    pub texture: Option<String>,
    pub item_name: String,
    pub link: i32,
    pub pan_u: i16,
    pub pan_v: i16,
}

/// The geometry of a brush actor (movers, special brushes), in the brush's own space.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Brush {
    pub model: Model,
    pub polys: Vec<Poly>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    pub name: String,
    /// Full class path, e.g. `HPBase.harry`.
    pub class: String,
    /// Values set on this object in the map (class defaults come from `classes.json`).
    pub props: PropMap,
    /// Key into `LevelFile::brushes` for brush actors.
    pub brush: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReachSpec {
    pub distance: i32,
    pub start: i32,
    pub end: i32,
    pub collision_radius: i32,
    pub collision_height: i32,
    pub flags: i32,
    pub pruned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LevelFile {
    pub name: String,
    /// Index = actor index used by every reference in this file. Empty slots are `None`.
    pub actors: Vec<Option<Actor>>,
    pub model: Model,
    pub brushes: BTreeMap<String, Brush>,
    pub reach_specs: Vec<ReachSpec>,
}

impl LevelFile {
    pub fn actor_by_name(&self, name: &str) -> Option<(usize, &Actor)> {
        self.actors
            .iter()
            .enumerate()
            .find_map(|(i, a)| a.as_ref().filter(|a| a.name.eq_ignore_ascii_case(name)).map(|a| (i, a)))
    }
}
