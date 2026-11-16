//! Walking and falling characters (see `specs/engine/Walking.md`).

use crate::collision::{Collider, Hit, find_spot, move_box};
use glam::{Vec3, Vec3Swizzles};

/// Floors whose normal points up at least this much can be walked on (about 45°).
pub const WALKABLE_Z: f32 = 0.7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Walking,
    Falling,
}

/// How a character moves; read from its class and zone.
#[derive(Clone, Copy, Debug)]
pub struct Movement {
    pub ground_speed: f32,
    pub accel_rate: f32,
    pub air_control: f32,
    pub jump_z: f32,
    pub step_height: f32,
    pub gravity: f32,
    pub ground_friction: f32,
}

impl Default for Movement {
    /// Harry's values.
    fn default() -> Self {
        Self {
            ground_speed: 200.0,
            accel_rate: 1024.0,
            air_control: 0.25,
            jump_z: 245.0,
            step_height: 25.0,
            gravity: -512.0,
            ground_friction: 8.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Body {
    /// Centre of the collision box.
    pub pos: Vec3,
    pub vel: Vec3,
    /// Half-size: (radius, radius, height).
    pub extent: Vec3,
    pub mode: Mode,
    /// What it stands on (an actor such as a lift), if not the level.
    pub base: Option<usize>,
    /// An actor it walked into during the last tick (doors open when bumped).
    pub bumped: Option<usize>,
}

impl Body {
    pub fn new(pos: Vec3, radius: f32, height: f32) -> Self {
        Self {
            pos,
            vel: Vec3::ZERO,
            extent: Vec3::new(radius, radius, height),
            mode: Mode::Falling,
            base: None,
            bumped: None,
        }
    }

    /// Moves the body out of the geometry if it was placed touching it.
    pub fn settle(&mut self, model: &dyn Collider) {
        if let Some(p) = find_spot(model, self.pos, self.extent, self.extent.z) {
            self.pos = p;
        }
    }

    /// One tick. `wish` is the wanted horizontal direction (length 0..1).
    pub fn tick(&mut self, model: &dyn Collider, m: &Movement, wish: Vec3, jump: bool, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        if model.overlaps(self.pos, self.extent) {
            self.settle(model);
        }
        let wish = (wish * Vec3::new(1.0, 1.0, 0.0)).clamp_length_max(1.0);
        match self.mode {
            Mode::Walking => {
                self.accelerate(wish, m.accel_rate, m.ground_speed, m.ground_friction, dt);
                if jump {
                    self.vel.z = m.jump_z;
                    self.mode = Mode::Falling;
                    self.fall(model, m, dt);
                } else {
                    self.vel.z = 0.0;
                    self.walk(model, m, dt);
                }
            }
            Mode::Falling => {
                self.accelerate(wish, m.accel_rate * m.air_control, m.ground_speed, 0.0, dt);
                self.vel.z += m.gravity * dt;
                self.fall(model, m, dt);
            }
        }
    }

    /// One tick of scripted movement: the horizontal velocity is set directly, as for characters
    /// walking a route. Gravity, steps, slopes and walls work as in `tick`.
    pub fn drive(&mut self, model: &dyn Collider, m: &Movement, vel: glam::Vec2, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        if model.overlaps(self.pos, self.extent) {
            self.settle(model);
        }
        self.vel = vel.extend(self.vel.z);
        match self.mode {
            Mode::Walking => {
                self.vel.z = 0.0;
                self.walk(model, m, dt);
            }
            Mode::Falling => {
                self.vel.z += m.gravity * dt;
                self.fall(model, m, dt);
            }
        }
    }

    /// Speeds the horizontal velocity up towards `wish * max` and lets friction slow the rest.
    fn accelerate(&mut self, wish: Vec3, accel: f32, max: f32, friction: f32, dt: f32) {
        let mut v = self.vel.xy();
        let w = wish.xy();
        let slow = (1.0 - friction * dt).max(0.0);
        if w == glam::Vec2::ZERO {
            v *= slow;
        } else {
            // Friction only on the part of the velocity that isn't going where we want.
            let dir = w.normalize();
            let along = v.dot(dir);
            v = dir * along + (v - dir * along) * slow;
            v += w * accel * dt;
            v = v.clamp_length_max(max * w.length());
        }
        self.vel = v.extend(self.vel.z);
    }

    fn walk(&mut self, model: &dyn Collider, m: &Movement, dt: f32) {
        let delta = self.vel * dt;
        if delta.length_squared() > 0.0 {
            let start = self.pos;
            let (p, hit) = move_box(model, self.pos, delta, self.extent);
            self.pos = p;
            if let Some(hit) = hit {
                let left = delta * (1.0 - hit.time);
                if hit.normal.z >= WALKABLE_Z {
                    // A ramp: carry on along its surface.
                    self.slide(model, left, hit.normal);
                } else if !self.step_up(model, left, m.step_height) {
                    self.bumped = hit.actor.or(self.bumped);
                    self.slide(model, left, horizontal(hit.normal));
                    // Lose the speed that went into the wall, so it doesn't build up.
                    let moved = (self.pos - start).xy() / dt;
                    if moved.length() < self.vel.xy().length() {
                        self.vel = moved.extend(0.0);
                    }
                }
            }
        }
        // Stay on the floor: look down at most a step's height.
        let (p, floor) = move_box(model, self.pos, Vec3::Z * -(m.step_height + 2.0), self.extent);
        match floor {
            Some(f) if f.normal.z >= WALKABLE_Z => {
                self.pos = p;
                self.base = f.actor;
            }
            _ => {
                self.mode = Mode::Falling;
                self.base = None;
            }
        }
    }

    /// Tries to get over an obstacle by lifting, moving and putting down again.
    fn step_up(&mut self, model: &dyn Collider, left: Vec3, height: f32) -> bool {
        let (up, _) = move_box(model, self.pos, Vec3::Z * height, self.extent);
        let (across, blocked) = move_box(model, up, left, self.extent);
        let gained = (across - up).xy().length();
        if gained < 0.5 || blocked.is_some_and(|b| b.time < 0.05) {
            return false;
        }
        let (down, floor) = move_box(model, across, Vec3::Z * -(height + 2.0), self.extent);
        match floor {
            Some(f) if f.normal.z >= WALKABLE_Z => {
                self.pos = down;
                true
            }
            // Stepping off an edge: fine, we'll start falling from there.
            None => {
                self.pos = down;
                true
            }
            _ => false,
        }
    }

    /// Moves what is left of a move along a surface.
    fn slide(&mut self, model: &dyn Collider, left: Vec3, normal: Vec3) {
        let mut delta = left - normal * left.dot(normal);
        let mut normal = normal;
        for _ in 0..2 {
            if delta.length_squared() < 1e-4 {
                return;
            }
            let (p, hit) = move_box(model, self.pos, delta, self.extent);
            self.pos = p;
            let Some(h) = hit else { return };
            // Into a corner: move along the crease of the two surfaces.
            let rest = delta * (1.0 - h.time);
            let crease = normal.cross(h.normal);
            delta = if crease.length_squared() > 1e-4 {
                let c = crease.normalize();
                c * rest.dot(c)
            } else {
                rest - h.normal * rest.dot(h.normal)
            };
            normal = h.normal;
        }
    }

    fn fall(&mut self, model: &dyn Collider, _m: &Movement, dt: f32) {
        let mut delta = self.vel * dt;
        for _ in 0..3 {
            if delta.length_squared() < 1e-6 {
                return;
            }
            let (p, hit) = move_box(model, self.pos, delta, self.extent);
            self.pos = p;
            let Some(Hit { time, normal, actor, .. }) = hit else { return };
            if normal.z >= WALKABLE_Z && self.vel.z <= 0.0 {
                self.mode = Mode::Walking;
                self.base = actor;
                self.vel.z = 0.0;
                return;
            }
            // Slide along walls and ceilings, losing the speed going into them.
            self.vel -= normal * self.vel.dot(normal).min(0.0);
            let rest = delta * (1.0 - time);
            delta = rest - normal * rest.dot(normal).min(0.0);
        }
    }
}

fn horizontal(n: Vec3) -> Vec3 {
    let h = Vec3::new(n.x, n.y, 0.0);
    if h.length_squared() > 1e-6 { h.normalize() } else { n }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::{floor, ramp, step};

    fn run(model: &hp1_data::level::Model, body: &mut Body, wish: Vec3, seconds: f32) {
        let m = Movement::default();
        for _ in 0..(seconds * 60.0) as usize {
            body.tick(model, &m, wish, false, 1.0 / 60.0);
        }
    }

    #[test]
    fn falls_and_lands_on_the_floor() {
        let model = floor();
        let mut b = Body::new(Vec3::new(0.0, 0.0, 200.0), 15.0, 42.0);
        run(&model, &mut b, Vec3::ZERO, 2.0);
        assert_eq!(b.mode, Mode::Walking);
        assert!((b.pos.z - 42.0).abs() < 0.5, "{:?}", b.pos);
    }

    #[test]
    fn walks_at_ground_speed() {
        let model = floor();
        let mut b = Body::new(Vec3::new(0.0, 0.0, 42.5), 15.0, 42.0);
        run(&model, &mut b, Vec3::X, 2.0);
        assert!((b.vel.x - 200.0).abs() < 1.0, "{:?}", b.vel);
        assert!(b.pos.x > 300.0, "{:?}", b.pos);
        assert!((b.pos.z - 42.0).abs() < 0.5);
        // And stops quickly without input.
        run(&model, &mut b, Vec3::ZERO, 0.5);
        assert!(b.vel.length() < 5.0, "{:?}", b.vel);
    }

    #[test]
    fn climbs_a_ramp() {
        let model = ramp();
        let mut b = Body::new(Vec3::new(-100.0, 0.0, 42.5), 15.0, 42.0);
        run(&model, &mut b, Vec3::X, 1.2);
        assert_eq!(b.mode, Mode::Walking);
        assert!(b.pos.z > 80.0, "should be part-way up: {:?}", b.pos);
    }

    #[test]
    fn steps_up_a_low_step() {
        let model = step(16.0);
        let mut b = Body::new(Vec3::new(-100.0, 0.0, 42.5), 15.0, 42.0);
        run(&model, &mut b, Vec3::X, 1.5);
        assert!(b.pos.x > 50.0 && (b.pos.z - 58.0).abs() < 1.0, "{:?}", b.pos);
    }

    #[test]
    fn a_high_step_is_a_wall() {
        let model = step(60.0);
        let mut b = Body::new(Vec3::new(-100.0, 0.0, 42.5), 15.0, 42.0);
        run(&model, &mut b, Vec3::X, 1.5);
        assert!(b.pos.x < -14.0 && b.pos.x > -16.0, "{:?}", b.pos);
    }

    #[test]
    fn spawning_in_the_floor_gets_out() {
        let model = floor();
        let mut b = Body::new(Vec3::new(0.0, 0.0, 38.0), 15.0, 42.0);
        b.settle(&model);
        assert!(b.pos.z >= 42.0 && b.pos.z < 44.0, "{:?}", b.pos);
    }
}
