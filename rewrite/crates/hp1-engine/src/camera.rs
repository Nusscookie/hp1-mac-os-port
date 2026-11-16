//! The standard follow camera (see `specs/engine/Camera.md`).

use glam::Vec3;

use crate::collision::Collider;

pub struct FollowCamera {
    pub pos: Vec3,
    pub distance: f32,
    pub height: f32,
    /// Aim point relative to Harry: forward, right, up.
    pub aim: Vec3,
    /// Extra tilt in radians (mouse up/down while standing).
    pub tilt: f32,
    started: bool,
}

impl Default for FollowCamera {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            distance: 150.0,
            height: 80.0,
            aim: Vec3::new(75.0, 0.0, 50.0),
            tilt: 0.0,
            started: false,
        }
    }
}

/// How the camera looks: from `pos` towards `target`.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub pos: Vec3,
    pub target: Vec3,
}

impl View {
    /// Yaw and pitch in radians.
    pub fn angles(&self) -> (f32, f32) {
        let d = self.target - self.pos;
        (d.y.atan2(d.x), d.z.atan2(d.truncate().length()))
    }
}

pub const MAX_TILT: f32 = 12000.0 * crate::units::ROT_TO_RAD;

impl FollowCamera {
    /// Moves the camera for Harry at `harry` (collision centre) facing `yaw` radians.
    pub fn update(
        &mut self,
        world: &dyn Collider,
        harry: Vec3,
        yaw: f32,
        eye: f32,
        moving: bool,
        dt: f32,
    ) -> View {
        let (s, c) = yaw.sin_cos();
        let forward = Vec3::new(c, s, 0.0);
        let right = Vec3::new(-s, c, 0.0);
        let target = harry + forward * self.aim.x + right * self.aim.y + Vec3::Z * self.aim.z;
        if moving {
            // Ease the tilt back to level.
            self.tilt *= (1.0 - 4.0 * dt).max(0.0);
        }
        self.tilt = self.tilt.clamp(-MAX_TILT, MAX_TILT);
        let above = harry + Vec3::Z * (eye + self.height);
        let mut dir = (above - target).normalize_or(-forward);
        // Tilting turns the line around Harry's right-hand axis.
        if self.tilt != 0.0 {
            dir = glam::Quat::from_axis_angle(right, -self.tilt) * dir;
        }
        let goal = harry + dir * self.distance;
        let head = harry + Vec3::Z * eye;
        let extent = Vec3::splat(6.0);
        let safe_goal = match world.trace(head, goal, extent) {
            Some(h) => head.lerp(goal, h.time),
            None => goal,
        };
        if !self.started {
            self.pos = safe_goal;
            self.started = true;
        } else {
            let mut next = self.pos.lerp(safe_goal, (8.0 * dt).min(1.0));
            // Gliding must not pass through walls either.
            if let Some(h) = world.trace(head, next, extent) {
                next = head.lerp(next, h.time);
            }
            self.pos = next;
        }
        View { pos: self.pos, target }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::floor;

    #[test]
    fn hangs_behind_and_above_and_looks_down_past_harry() {
        let model = floor();
        let mut cam = FollowCamera::default();
        let harry = Vec3::new(0.0, 0.0, 42.0);
        let v = cam.update(&model, harry, 0.0, 40.0, false, 0.016);
        assert!(v.pos.x < -50.0 && v.pos.z > harry.z + 40.0, "{:?}", v.pos);
        let (yaw, pitch) = v.angles();
        assert!(yaw.abs() < 0.01 && pitch < 0.0, "{yaw} {pitch}");
    }

    #[test]
    fn stays_out_of_the_ground_when_tilted_down() {
        let model = floor();
        let mut cam = FollowCamera { tilt: -MAX_TILT, ..Default::default() };
        let v = cam.update(&model, Vec3::new(0.0, 0.0, 42.0), 0.0, 40.0, false, 0.016);
        assert!(v.pos.z > 5.0, "{:?}", v.pos);
    }
}
