//! Conversions for the game's units.

use glam::{Mat4, Quat, Vec3};

/// 65536 rotation units are a full turn.
pub const ROT_TO_RAD: f32 = std::f32::consts::TAU / 65536.0;

/// The game's rotation (pitch, yaw, roll) as a matrix, in the game's left-handed, Z-up frame
/// (X forward, Y right, Z up). Yaw turns around Z, pitch around Y, roll around X.
pub fn rotator_matrix(pitch: i32, yaw: i32, roll: i32) -> Mat4 {
    let (sp, cp) = (pitch as f32 * ROT_TO_RAD).sin_cos();
    let (sy, cy) = (yaw as f32 * ROT_TO_RAD).sin_cos();
    let (sr, cr) = (roll as f32 * ROT_TO_RAD).sin_cos();
    // Columns are the rotated X (forward), Y (right) and Z (up) axes.
    let x = Vec3::new(cp * cy, cp * sy, sp);
    let y = Vec3::new(sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp);
    let z = Vec3::new(-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp);
    Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), glam::Vec4::W)
}

/// `[x, y, z, w]` → glam quaternion.
pub fn quat(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], q[1], q[2], q[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_quarter_turn_points_x_to_y() {
        let m = rotator_matrix(0, 16384, 0);
        let v = m.transform_vector3(Vec3::X);
        assert!((v - Vec3::Y).length() < 1e-5, "{v}");
    }

    #[test]
    fn pitch_up_points_x_to_z() {
        let m = rotator_matrix(16384, 0, 0);
        let v = m.transform_vector3(Vec3::X);
        assert!((v - Vec3::Z).length() < 1e-5, "{v}");
    }
}
