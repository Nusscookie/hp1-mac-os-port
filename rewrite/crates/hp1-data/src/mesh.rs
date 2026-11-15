//! Meshes and skeletal animations, stored as compact binary files (postcard).
//!
//! Conventions (already converted from Unreal's):
//! - Quaternions are standard `[x, y, z, w]` (glam order and handedness).
//! - Positions are in Unreal units in mesh space; `MeshTransform` maps mesh space to the object.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshTransform {
    pub scale: [f32; 3],
    pub origin: [f32; 3],
    /// Pitch, yaw, roll in Unreal units (65536 = full turn).
    pub rot_origin: [i32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshMaterial {
    /// Slot number that an object's `MultiSkins[n]` / `Skins[n]` can override.
    pub slot: i32,
    /// The mesh's own texture for that slot, if any.
    pub texture: Option<String>,
    pub poly_flags: u32,
}

/// A drawable vertex: which point it uses plus its texture coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshVertex {
    pub point: u32,
    pub uv: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Triangle {
    pub vertices: [u32; 3],
    pub material: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    pub name: String,
    /// Index of the parent bone, `-1` for the root.
    pub parent: i32,
    /// Bind pose relative to the parent.
    pub rotation: [f32; 4],
    pub position: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Influence {
    pub bone: u16,
    pub weight: f32,
    /// The point's position in this bone's space (skinned = Σ weight · bone_matrix · local).
    pub local: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimNotify {
    /// 0..1 through the sequence.
    pub time: f32,
    /// Name of the event the game should fire (e.g. `FootStep`).
    pub event: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SequenceInfo {
    pub name: String,
    pub group: String,
    /// Frames per second the sequence is authored for.
    pub rate: f32,
    pub frames: u32,
    /// First frame (vertex meshes only).
    pub start_frame: u32,
    pub notifies: Vec<AnimNotify>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MeshBody {
    Skeletal {
        bones: Vec<Bone>,
        /// Bind-pose positions, one per point.
        points: Vec<[f32; 3]>,
        /// Per point, strongest first.
        influences: Vec<Vec<Influence>>,
        default_animation: Option<String>,
        weapon_bone: u32,
        /// Origin, X, Y, Z axis of the weapon attachment frame in the weapon bone's space.
        weapon_adjust: [[f32; 3]; 4],
    },
    /// Vertex animation: `frame_points` positions per frame.
    Vertex { frame_points: u32, frames: u32, positions: Vec<[f32; 3]>, sequences: Vec<SequenceInfo> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshFile {
    pub path: String,
    pub transform: MeshTransform,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub materials: Vec<MeshMaterial>,
    pub vertices: Vec<MeshVertex>,
    pub triangles: Vec<Triangle>,
    /// Attachment triangles (not drawn).
    pub special_triangles: Vec<Triangle>,
    pub body: MeshBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub rotations: Vec<[f32; 4]>,
    pub positions: Vec<[f32; 3]>,
    /// Seconds, one per key (if fewer times than keys, the remaining keys are static).
    pub times: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sequence {
    pub info: SequenceInfo,
    /// Length in seconds.
    pub duration: f32,
    pub root_speed: [f32; 3],
    /// One per animation bone (`AnimFile::bones`).
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimBone {
    pub name: String,
    pub parent: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimFile {
    pub path: String,
    pub bones: Vec<AnimBone>,
    pub sequences: Vec<Sequence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshIndexEntry {
    pub path: String,
    pub file: String,
    pub skeletal: bool,
    pub triangles: usize,
    pub bones: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_animation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_file: Option<String>,
}

pub fn save_binary<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = postcard::to_stdvec(value).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

pub fn load_binary<T: for<'de> Deserialize<'de>>(path: &Path) -> std::io::Result<T> {
    let bytes = std::fs::read(path)?;
    postcard::from_bytes(&bytes).map_err(std::io::Error::other)
}
