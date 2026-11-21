//! The standard follow camera (see `specs/engine/Camera.md`) and the camera's cutscene mode
//! (`specs/cutscenes/CutScene.md`).

use glam::Vec3;
use hp1_data::ClassTable;
use hp1_data::level::LevelFile;

use crate::collision::Collider;
use crate::props::ActorProps;
use crate::units::ROT_TO_RAD;
use crate::world::ActorId;

/// The camera's flying speed in cutscenes at `CAMSPEED 1`.
pub const NORMAL_SPEED: f32 = 2.0;
/// A camera move counts as done this close, unless `CAMPROX` says otherwise.
pub const NORMAL_PROX: f32 = 15.0;

/// What capturing the camera saves and releasing it gives back.
#[derive(Clone, Debug)]
pub(crate) struct SavedCamera {
    pub speed: f32,
    pub rate: [f32; 3],
    pub position: Option<ActorId>,
    pub direction: Option<ActorId>,
    /// Where it was relative to Harry, and Harry's facing then.
    pub offset: Vec3,
    pub harry_yaw: i32,
    pub rot: [i32; 3],
    pub desired: [i32; 3],
}

/// The player's camera as cutscenes see it. Outside cutscene mode the follow camera moves it
/// and the program keeps `pos` and `rot` up to date.
#[derive(Clone, Debug)]
pub struct CutCamera {
    /// The camera object in the level (HP1 `BaseCam`), if placed.
    pub actor: Option<ActorId>,
    /// In cutscene mode: the cutscene places and turns the camera.
    pub active: bool,
    pub pos: Vec3,
    /// Pitch, yaw, roll in rotation units.
    pub rot: [i32; 3],
    pub(crate) desired: [i32; 3],
    pub(crate) speed: f32,
    /// Turning speed (pitch, yaw, roll) and its normal value.
    pub(crate) rate: [f32; 3],
    pub(crate) normal_rate: [f32; 3],
    /// Where it flies to and what it looks at.
    pub(crate) position: Option<ActorId>,
    pub(crate) direction: Option<ActorId>,
    pub(crate) prox: f32,
    /// The cutscene and cue to tell when it reaches `position`.
    pub(crate) notify: Option<(ActorId, String)>,
    pub(crate) stack: Vec<SavedCamera>,
    /// Set when it leaves cutscene mode, for the follow camera to carry on from here.
    pub left: bool,
}

impl CutCamera {
    pub fn new(level: &LevelFile, classes: &ClassTable) -> Self {
        let placed = level.actors.iter().enumerate().find_map(|(i, a)| {
            let a = a.as_ref()?;
            classes.is_a(&a.class, "HPBase.BaseCam").then(|| (i, ActorProps::new(a, classes)))
        });
        let rate = placed.as_ref().map_or([20000; 3], |(_, p)| p.rotator("RotationRate")).map(|r| r as f32);
        Self {
            actor: placed.as_ref().map(|(i, _)| *i),
            active: false,
            pos: placed.as_ref().map_or(Vec3::ZERO, |(_, p)| Vec3::from(p.vec3("Location"))),
            rot: [0; 3],
            desired: [0; 3],
            speed: NORMAL_SPEED,
            rate,
            normal_rate: rate,
            position: None,
            direction: None,
            prox: NORMAL_PROX,
            notify: None,
            stack: Vec::new(),
            left: false,
        }
    }

    /// Where it looks from and to.
    pub fn view(&self) -> View {
        let (pitch, yaw) = (self.rot[0] as f32 * ROT_TO_RAD, self.rot[1] as f32 * ROT_TO_RAD);
        let dir = Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin());
        View { pos: self.pos, target: self.pos + dir * 100.0 }
    }

    /// The follow camera's place and aim, while not in cutscene mode.
    pub fn follow(&mut self, view: View) {
        self.pos = view.pos;
        self.rot = rotation_towards(view.target - view.pos);
        self.desired = self.rot;
    }
}

/// The facing that looks along `v` (pitch, yaw, roll), as the original engine works it out.
pub fn rotation_towards(v: Vec3) -> [i32; 3] {
    let scale = 65536.0 / std::f32::consts::TAU;
    let yaw = (v.y.atan2(v.x) * scale) as i32;
    let pitch = (v.z.atan2(v.truncate().length()) * scale) as i32;
    [pitch, yaw, 0]
}

/// Turns `from` towards `to` the short way by at most `speed` (rotation units, 0..65535).
pub fn turn_shortest(from: i32, to: i32, speed: i32) -> i32 {
    let (from, to) = (from & 0xffff, to & 0xffff);
    if from > to {
        if from - to < 0x8000 {
            (from - (from - to).min(speed)) & 0xffff
        } else {
            (from + (to + 0x10000 - from).min(speed)) & 0xffff
        }
    } else if to - from < 0x8000 {
        (from + (to - from).min(speed)) & 0xffff
    } else {
        (from - (from + 0x10000 - to).min(speed)) & 0xffff
    }
}

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
