//! Skeletal animation: sampling a sequence into a pose, bone matrices and skinning.

use glam::{Mat4, Quat, Vec3};
use hp1_data::mesh::{AnimFile, Bone, Influence, Sequence, Track};

use crate::units::{quat, rotator_matrix};

/// Bone transforms relative to their parents, one per mesh bone.
#[derive(Clone, Debug, Default)]
pub struct Pose {
    pub rotations: Vec<Quat>,
    pub positions: Vec<Vec3>,
}

impl Pose {
    pub fn bind(bones: &[Bone]) -> Self {
        Self {
            rotations: bones.iter().map(|b| quat(b.rotation)).collect(),
            positions: bones.iter().map(|b| Vec3::from(b.position)).collect(),
        }
    }

    /// Moves this pose towards `other` (0 keeps this pose, 1 replaces it).
    pub fn blend(&mut self, other: &Pose, alpha: f32) {
        for (a, b) in self.rotations.iter_mut().zip(&other.rotations) {
            *a = nlerp(*a, *b, alpha);
        }
        for (a, b) in self.positions.iter_mut().zip(&other.positions) {
            *a = a.lerp(*b, alpha);
        }
    }
}

/// Normalized lerp on the short path. Animation keys are dense enough that this matches slerp.
pub fn nlerp(a: Quat, b: Quat, t: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    let q = a.lerp(b, t);
    if q.length_squared() > 1e-12 { q.normalize() } else { a }
}

/// For each mesh bone, the animation bone with the same name (case-insensitive).
pub fn bone_links(mesh_bones: &[Bone], anim: &AnimFile) -> Vec<Option<usize>> {
    mesh_bones.iter().map(|b| anim.bones.iter().position(|a| a.name.eq_ignore_ascii_case(&b.name))).collect()
}

fn sample_track(track: &Track, time: f32) -> (Option<Quat>, Option<Vec3>) {
    if track.rotations.is_empty() {
        return (None, None);
    }
    let n = track.times.len().min(track.rotations.len());
    let (k0, k1, alpha) = if n > 1 {
        let mut k1 = 1;
        while k1 + 1 < n && track.times[k1] <= time {
            k1 += 1;
        }
        let (t0, t1) = (track.times[k1 - 1], track.times[k1]);
        let a = if t1 > t0 { ((time - t0) / (t1 - t0)).clamp(0.0, 1.0) } else { 0.0 };
        (k1 - 1, k1, a)
    } else {
        (0, 0, 0.0)
    };
    let rot = if k0 == k1 {
        quat(track.rotations[k0])
    } else {
        nlerp(quat(track.rotations[k0]), quat(track.rotations[k1]), alpha)
    };
    let pos = match track.positions.len() {
        0 => None,
        1 => Some(Vec3::from(track.positions[0])),
        len if k1 < len => Some(Vec3::from(track.positions[k0]).lerp(Vec3::from(track.positions[k1]), alpha)),
        _ => None,
    };
    (Some(rot), pos)
}

/// Samples `seq` at `time` seconds. Bones the animation doesn't drive keep the bind pose.
pub fn sample(mesh_bones: &[Bone], links: &[Option<usize>], seq: &Sequence, time: f32) -> Pose {
    let mut pose = Pose::bind(mesh_bones);
    for (i, link) in links.iter().enumerate() {
        let Some(track) = link.and_then(|a| seq.tracks.get(a)) else { continue };
        let (rot, pos) = sample_track(track, time);
        if let Some(r) = rot {
            pose.rotations[i] = r;
        }
        if let Some(p) = pos {
            pose.positions[i] = p;
        }
    }
    pose
}

/// Bone-to-mesh-space matrices.
pub fn bone_matrices(bones: &[Bone], pose: &Pose) -> Vec<Mat4> {
    let mut out: Vec<Mat4> = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        let local = Mat4::from_rotation_translation(pose.rotations[i], pose.positions[i]);
        let m = match usize::try_from(b.parent).ok().filter(|&p| p < i) {
            Some(p) => out[p] * local,
            None => local,
        };
        out.push(m);
    }
    out
}

/// Skinned point positions in mesh space.
pub fn skin(influences: &[Vec<Influence>], matrices: &[Mat4]) -> Vec<Vec3> {
    influences
        .iter()
        .map(|list| {
            list.iter()
                .filter_map(|inf| {
                    matrices
                        .get(inf.bone as usize)
                        .map(|m| m.transform_point3(Vec3::from(inf.local)) * inf.weight)
                })
                .sum()
        })
        .collect()
}

/// Mesh space → object space: rotate by the mesh's rotation origin, scale, then move by its origin.
pub fn mesh_to_object(t: &hp1_data::mesh::MeshTransform) -> Mat4 {
    let [p, y, r] = t.rot_origin;
    rotator_matrix(p, y, r)
        * Mat4::from_scale(Vec3::from(t.scale))
        * Mat4::from_translation(-Vec3::from(t.origin))
}
