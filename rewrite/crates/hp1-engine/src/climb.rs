//! Harry pulling himself up onto ledges (see `specs/characters/Climbing.md`). When a climb
//! starts is our own rule (the original's is native and lost); what a climb does follows
//! Harry's script.

use glam::Vec3;

use crate::collision::{Collider, move_box};
use crate::walk::{Body, Mode, WALKABLE_Z, horizontal};
use crate::world::Sequence;

/// Harry's `MaxMountHeight`.
pub const MAX_MOUNT: f32 = 96.5;
/// A falling Harry catches the ledge this far below his place on it.
const HANG: f32 = 82.0;
/// The forward movement the climb animations make themselves.
const ANIM_FORWARD: f32 = 30.0;

pub const LANDING: &str = "HPSounds.HAR_emotes.landing5";
pub const PULL_UP_HANG: &str = "HPSounds.HAR_emotes.pull_up3";
pub const PULL_UP_LOW: &str = "HPSounds.HAR_emotes.EmotiveHarry5_b_pullup6";
pub const PULL_UP: &str = "HPSounds.HAR_emotes.EmotiveHarry5_a_pullup5";

/// Our rule: a ledge Harry can climb onto from where his body is, wanting to go `wish`. Gives
/// where his centre would stand on it.
pub fn find_ledge(model: &dyn Collider, body: &Body, wish: Vec3, step: f32, max_mount: f32) -> Option<Vec3> {
    let normal = body.wall?;
    let into = -horizontal(normal);
    let wish = Vec3::new(wish.x, wish.y, 0.0);
    if wish.length_squared() < 1e-6 || wish.normalize().dot(into) < 0.5 {
        return None;
    }
    let extent = body.extent;
    let width = extent.x * 2.0;
    let (up, _) = move_box(model, body.pos, Vec3::Z * max_mount, extent);
    let (across, _) = move_box(model, up, into * width, extent);
    if (across - up).truncate().length() < width - 0.5 {
        return None;
    }
    let (down, floor) = move_box(model, across, Vec3::Z * -(max_mount + 2.0), extent);
    let floor = floor?;
    let rise = down.z - body.pos.z;
    if floor.normal.z < WALKABLE_Z || rise <= step || rise > max_mount || model.overlaps(down, extent) {
        return None;
    }
    Some(down)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Falling, holding on for the ledge.
    Hang,
    /// Playing a climb animation; `then_end` means `climb96end` follows.
    Anim { left: f32, then_end: bool },
}

/// A climb in progress.
#[derive(Clone, Debug)]
pub struct Climb {
    dest: Vec3,
    falling: bool,
    phase: Phase,
    /// The script's `MountDelta`: the way not covered by the animation itself.
    delta: Vec3,
    pub anim: &'static str,
    /// Seconds into `anim`; 0 while hanging (its first frame held).
    pub anim_time: f32,
    /// One loop of `anim` (the script moves by the delta times the part of a loop played).
    cycle: f32,
    /// The animation's own travel (turned to Harry's facing), applied at its end.
    travel: Vec3,
}

/// What the climb wants this tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// A sound to play from Harry (at half volume).
    pub sound: Option<&'static str>,
    /// Where the camera should follow instead of Harry's place (moving smoothly along the
    /// animation's own travel).
    pub focus: Vec3,
    /// The climb is over; Harry walks again.
    pub done: bool,
}

fn facing(yaw: f32) -> Vec3 {
    Vec3::new(yaw.cos(), yaw.sin(), 0.0)
}

/// The animation's travel turned to a facing: x forward, y right, z up.
fn turned(travel: Vec3, yaw: f32) -> Vec3 {
    let f = facing(yaw);
    f * travel.x + Vec3::Z.cross(f) * travel.y + Vec3::Z * travel.z
}

impl Climb {
    /// `Mount(delta)`: Harry's `body` climbs to `dest`.
    pub fn start(body: &Body, dest: Vec3) -> Self {
        Self {
            dest,
            falling: body.mode == Mode::Falling,
            phase: Phase::Hang,
            delta: Vec3::ZERO,
            anim: "climb96end",
            anim_time: 0.0,
            cycle: 1.0,
            travel: Vec3::ZERO,
        }
    }

    /// Whether Harry is still hanging on, falling (input still steers him then).
    pub fn hanging(&self) -> bool {
        self.phase == Phase::Hang
    }

    /// One tick. While hanging the caller has already moved `body` by falling; landing or
    /// dropping to the catch point starts the pull-up.
    pub fn tick(
        &mut self,
        body: &mut Body,
        yaw: &mut f32,
        moved: Vec3,
        dt: f32,
        seq: &dyn Fn(&str) -> Option<Sequence>,
    ) -> Frame {
        match self.phase {
            Phase::Hang => {
                if !self.falling {
                    return self.mounting(body, yaw, seq);
                }
                // The destination moves sideways with him.
                self.dest.x += moved.x;
                self.dest.y += moved.y;
                if body.mode == Mode::Walking {
                    let mut f = self.mounting(body, yaw, seq);
                    f.sound = Some(LANDING);
                    return f;
                }
                if body.pos.z <= self.dest.z - HANG {
                    body.pos.z = self.dest.z - HANG;
                    return self.mounting(body, yaw, seq);
                }
                Frame { sound: None, focus: body.pos, done: false }
            }
            Phase::Anim { left, then_end } => {
                body.pos += self.delta * dt / self.cycle;
                self.anim_time += dt;
                let left = left - dt;
                let progress = (self.anim_time / (self.anim_time + left.max(0.0))).clamp(0.0, 1.0);
                if left > 0.0 {
                    self.phase = Phase::Anim { left, then_end };
                    return Frame { sound: None, focus: body.pos + self.travel * progress, done: false };
                }
                // The animation's own movement, now that it is over.
                body.pos += self.travel;
                if then_end {
                    self.play("climb96end", *yaw, seq);
                    return Frame { sound: Some(PULL_UP), focus: body.pos, done: false };
                }
                body.mode = Mode::Walking;
                body.vel = Vec3::ZERO;
                Frame { sound: None, focus: body.pos, done: true }
            }
        }
    }

    fn mounting(&mut self, body: &mut Body, yaw: &mut f32, seq: &dyn Fn(&str) -> Option<Sequence>) -> Frame {
        body.vel = Vec3::ZERO;
        let to = self.dest - body.pos;
        if to.truncate().length() > 1e-3 {
            *yaw = to.y.atan2(to.x);
        }
        self.delta = to - facing(*yaw) * ANIM_FORWARD;
        let (anim, lift, sound) = if self.falling {
            ("climb96end", HANG, Some(PULL_UP_HANG))
        } else if self.delta.z < 48.0 {
            ("climb32", 32.0, Some(PULL_UP_LOW))
        } else if self.delta.z < 80.0 {
            ("climb64", 64.0, Some(PULL_UP))
        } else {
            ("climb96start", 96.0, None)
        };
        self.delta.z -= lift;
        if anim == "climb96start" {
            // Split in two: half during each animation.
            self.delta *= 0.5;
        }
        self.play(anim, *yaw, seq);
        Frame { sound, focus: body.pos, done: false }
    }

    fn play(&mut self, anim: &'static str, yaw: f32, seq: &dyn Fn(&str) -> Option<Sequence>) {
        let s = seq(anim).unwrap_or(Sequence::new(31, 30.0));
        self.anim = anim;
        self.anim_time = 0.0;
        self.cycle = s.cycle();
        self.travel = turned(s.travel, yaw);
        self.phase = Phase::Anim { left: s.once(), then_end: anim == "climb96start" };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::step;
    use crate::walk::Movement;

    /// The climb animations' lengths and own travel as in Harry's data.
    fn seq(name: &str) -> Option<Sequence> {
        let (frames, travel) = match name {
            "climb32" => (38, Vec3::new(30.0, 0.0, 31.0)),
            "climb64" => (60, Vec3::new(30.0, 0.0, 64.0)),
            "climb96start" => (32, Vec3::new(9.0, 0.0, 12.0)),
            "climb96end" => (49, Vec3::new(21.0, 0.0, 82.0)),
            _ => return None,
        };
        Some(Sequence { frames, rate: 30.0, travel })
    }

    fn walk_into(height: f32) -> (Body, Option<Vec3>) {
        let model = step(height);
        let m = Movement::default();
        let mut b = Body::new(Vec3::new(-60.0, 0.0, 42.5), 15.0, 42.0);
        let mut ledge = None;
        for _ in 0..120 {
            b.tick(&model, &m, Vec3::X, false, 1.0 / 60.0);
            ledge = ledge.or_else(|| find_ledge(&model, &b, Vec3::X, m.step_height, MAX_MOUNT));
        }
        (b, ledge)
    }

    fn climb(height: f32) -> (Body, Vec<&'static str>) {
        let (mut b, ledge) = walk_into(height);
        let dest = ledge.expect("a ledge");
        assert!((dest.z - (height + 42.0)).abs() < 1.0, "{dest}");
        let mut c = Climb::start(&b, dest);
        let (mut yaw, mut anims) = (0.0, Vec::new());
        for _ in 0..600 {
            let f = c.tick(&mut b, &mut yaw, Vec3::ZERO, 1.0 / 60.0, &seq);
            if anims.last() != Some(&c.anim) {
                anims.push(c.anim);
            }
            if f.done {
                return (b, anims);
            }
        }
        panic!("the climb didn't end");
    }

    #[test]
    fn low_walls_are_steps_and_ledges_up_to_the_mount_height_are_climbed() {
        assert_eq!(walk_into(16.0).1, None);
        assert_eq!(walk_into(120.0).1, None);
        let (b, anims) = climb(40.0);
        assert_eq!(anims, ["climb32"]);
        // Ends on the ledge, a little short of the destination as the script's numbers are.
        assert!(b.pos.x > 10.0 && (b.pos.z - 82.0).abs() < 3.0, "{}", b.pos);
        let (b, anims) = climb(64.0);
        assert_eq!(anims, ["climb64"]);
        assert!(b.pos.x > 10.0 && (b.pos.z - 106.0).abs() < 3.0, "{}", b.pos);
        let (b, anims) = climb(90.0);
        assert_eq!(anims, ["climb96start", "climb96end"]);
        assert!(b.pos.x > 10.0 && (b.pos.z - 132.0).abs() < 3.0, "{}", b.pos);
    }

    #[test]
    fn jumping_at_a_higher_ledge_hangs_on_and_pulls_up() {
        let model = step(120.0);
        let m = Movement::default();
        let mut b = Body::new(Vec3::new(-40.0, 0.0, 42.5), 15.0, 42.0);
        // Walk up to the wall, then jump.
        for _ in 0..60 {
            b.tick(&model, &m, Vec3::X, false, 1.0 / 60.0);
        }
        let mut climb = None;
        for i in 0..120 {
            b.tick(&model, &m, Vec3::X, i == 0, 1.0 / 60.0);
            if let Some(dest) = find_ledge(&model, &b, Vec3::X, m.step_height, MAX_MOUNT) {
                climb = Some(Climb::start(&b, dest));
                break;
            }
        }
        let mut c = climb.expect("a ledge in reach while jumping");
        assert!(c.falling);
        let mut yaw = 0.0;
        for _ in 0..600 {
            if c.hanging() {
                let before = b.pos;
                b.tick(&model, &m, Vec3::X, false, 1.0 / 60.0);
                let moved = b.pos - before;
                c.tick(&mut b, &mut yaw, moved, 1.0 / 60.0, &seq);
                continue;
            }
            if c.tick(&mut b, &mut yaw, Vec3::ZERO, 1.0 / 60.0, &seq).done {
                assert_eq!(c.anim, "climb96end");
                // climb96end moves 21 forward where the script expects 30, so he ends 9 short
                // of the destination, still on the ledge.
                assert!(b.pos.x > 5.0 && (b.pos.z - 162.0).abs() < 4.0, "{}", b.pos);
                return;
            }
        }
        panic!("the climb didn't end");
    }
}
