//! Walking as Harry: his body in the world, the follow camera and which animation he plays.

use glam::{Vec2, Vec3};
use hp1_engine::camera::{FollowCamera, View};
use hp1_engine::units::ROT_TO_RAD;
use hp1_engine::walk::{Body, Mode, Movement};
use hp1_engine::world::{ActorId, Animation, World};

/// Harry's eye height above his collision centre.
pub const EYE: f32 = 40.75;

pub struct Player {
    pub body: Body,
    /// Facing, radians.
    pub yaw: f32,
    pub cam: FollowCamera,
    pub anim: String,
    pub anim_time: f32,
    movement: Movement,
    /// Position and facing last written to Harry's object, to notice when a cutscene moved him.
    synced: (Vec3, i32),
}

fn yaw_units(yaw: f32) -> i32 {
    (yaw / ROT_TO_RAD) as i32
}

impl Player {
    pub fn new(world: &World, me: ActorId, pos: Vec3, yaw: f32) -> Self {
        let mut body = Body::new(pos, 15.0, 42.0);
        body.settle(&world.collider(Some(me)));
        Self {
            body,
            yaw,
            cam: FollowCamera::default(),
            anim: "breath".into(),
            anim_time: 0.0,
            movement: Movement::default(),
            synced: (Vec3::NAN, 0),
        }
    }

    /// Takes Harry's place, facing and animation from the world (after a cutscene had him).
    pub fn adopt(&mut self, world: &World, me: ActorId) {
        let Some(e) = world.get(me) else { return };
        self.body.pos = e.pos;
        self.body.vel = Vec3::ZERO;
        self.yaw = e.rot[1] as f32 * ROT_TO_RAD;
        if let Some(a) = &e.anim {
            self.anim = a.name.clone();
            self.anim_time = a.time;
        }
        self.synced = (e.pos, e.rot[1]);
    }

    /// While a cutscene drives Harry: show him as the world has him, camera following.
    pub fn follow_world(&mut self, world: &World, me: ActorId, dt: f32) -> View {
        self.adopt(world, me);
        self.cam.update(&world.collider(Some(me)), self.body.pos, self.yaw, EYE, false, dt)
    }

    /// One frame. `input` is (forward, right) in -1..1; the mouse has already turned `yaw`.
    pub fn step(&mut self, world: &mut World, me: ActorId, input: Vec2, jump: bool, dt: f32) -> View {
        // Moved or turned by a cutscene since the last frame.
        if let Some(e) = world.get(me)
            && !self.synced.0.is_nan()
            && (e.pos, e.rot[1]) != self.synced
        {
            let (anim, time) = (self.anim.clone(), self.anim_time);
            self.adopt(world, me);
            (self.anim, self.anim_time) = (anim, time);
        }
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
            e.rot = [0, yaw_units(self.yaw), 0];
            self.synced = (e.pos, e.rot[1]);
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
            self.anim = anim.to_owned();
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
        if let Some(e) = world.get_mut(me) {
            e.anim = Some(Animation { name: self.anim.clone(), time: self.anim_time, looping: true });
        }
        let moving = v.truncate().length() > 10.0;
        self.cam.update(&world.collider(Some(me)), self.body.pos, self.yaw, EYE, moving, dt)
    }
}
