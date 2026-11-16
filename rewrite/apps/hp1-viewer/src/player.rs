//! Walking as Harry: his body in the world, the follow camera and which animation he plays.

use glam::{Vec2, Vec3};
use hp1_engine::camera::{FollowCamera, View};
use hp1_engine::walk::{Body, Mode, Movement};
use hp1_engine::world::{ActorId, World};

/// Harry's eye height above his collision centre.
pub const EYE: f32 = 40.75;

pub struct Player {
    pub body: Body,
    /// Facing, radians.
    pub yaw: f32,
    pub cam: FollowCamera,
    pub anim: &'static str,
    pub anim_time: f32,
    movement: Movement,
}

impl Player {
    pub fn new(world: &World, me: ActorId, pos: Vec3, yaw: f32) -> Self {
        let mut body = Body::new(pos, 15.0, 42.0);
        body.settle(&world.collider(Some(me)));
        Self {
            body,
            yaw,
            cam: FollowCamera::default(),
            anim: "breath",
            anim_time: 0.0,
            movement: Movement::default(),
        }
    }

    /// One frame. `input` is (forward, right) in -1..1; the mouse has already turned `yaw`.
    pub fn step(&mut self, world: &mut World, me: ActorId, input: Vec2, jump: bool, dt: f32) -> View {
        let (s, c) = self.yaw.sin_cos();
        let forward = Vec3::new(c, s, 0.0);
        let right = Vec3::new(-s, c, 0.0);
        let wish = (forward * input.x + right * input.y).clamp_length_max(1.0);
        let can_jump = self.body.mode == Mode::Walking;
        // Ride along with whatever we stand on.
        if let Some(moved) = self.body.base.and_then(|b| world.get(b)).map(|e| e.moved) {
            self.body.pos += moved;
        }
        // Small fixed steps keep the movement the same at any frame rate.
        let steps = (dt / (1.0 / 120.0)).ceil().max(1.0);
        for i in 0..steps as usize {
            self.body.tick(
                &world.collider(Some(me)),
                &self.movement,
                wish,
                jump && can_jump && i == 0,
                dt / steps,
            );
            if let Some(b) = self.body.bumped.take() {
                world.bump(b, me);
            }
        }
        if let Some(e) = world.get_mut(me) {
            e.pos = self.body.pos;
            e.base = self.body.base;
            e.rot = [0, (self.yaw / hp1_engine::units::ROT_TO_RAD) as i32, 0];
        }

        let v = self.body.vel;
        let (ahead, side) = (v.dot(forward), v.dot(right));
        let anim = match self.body.mode {
            Mode::Falling if v.z > 0.0 => "Jump",
            Mode::Falling => "fall",
            _ if v.truncate().length() < 10.0 => "breath",
            _ if ahead < -10.0 && ahead.abs() >= side.abs() => "runback",
            _ if side.abs() > ahead.abs() => {
                if side > 0.0 {
                    "straferight"
                } else {
                    "strafeleft"
                }
            }
            _ => "run",
        };
        if anim != self.anim {
            self.anim = anim;
            self.anim_time = 0.0;
        }
        // Running plays faster or slower with the speed.
        let rate = match anim {
            "run" | "runback" | "straferight" | "strafeleft" => {
                (v.truncate().length() / self.movement.ground_speed).clamp(0.3, 1.5)
            }
            _ => 1.0,
        };
        self.anim_time += dt * rate;
        let moving = v.truncate().length() > 10.0;
        self.cam.update(&world.collider(Some(me)), self.body.pos, self.yaw, EYE, moving, dt)
    }
}
