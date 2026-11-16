//! The running level: every placed object with its position, collision and behaviour.
//!
//! Objects talk through names (see `specs/engine/Triggers.md`): `trigger_event("X")` calls
//! `trigger` on every object whose tag is X. Each object's behaviour is Rust code chosen by its
//! class when the level starts; objects without one just sit there.
//!
//! Sounds and music are not played here. They are collected in `outputs` for the program to play.

use std::collections::HashSet;

use glam::{Mat3, Vec3};
use hp1_data::ClassTable;
use hp1_data::level::LevelFile;

use crate::collision::{Collider, Hit, overlaps_model, trace_model};
use crate::props::ActorProps;
use crate::units::rotator_matrix;

pub type ActorId = usize;

/// Something for the program to play.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Sound {
        path: String,
        at: Vec3,
        radius: f32,
        volume: f32,
    },
    /// A looping sound attached to an actor, until `AmbientStop`.
    AmbientStart {
        actor: ActorId,
        path: String,
        at: Vec3,
        radius: f32,
        volume: f32,
    },
    AmbientStop {
        actor: ActorId,
    },
    /// Switch the music (None = silence).
    Music {
        song: Option<String>,
    },
}

/// What an object does. Every method has a do-nothing default.
pub trait Behaviour {
    fn tick(&mut self, _w: &mut World, _me: ActorId, _dt: f32) {}
    fn trigger(&mut self, _w: &mut World, _me: ActorId, _instigator: Option<ActorId>) {}
    fn untrigger(&mut self, _w: &mut World, _me: ActorId, _instigator: Option<ActorId>) {}
    /// A pawn started touching this object's collision.
    fn touch(&mut self, _w: &mut World, _me: ActorId, _other: ActorId) {}
    fn untouch(&mut self, _w: &mut World, _me: ActorId, _other: ActorId) {}
    /// A pawn walked into this object.
    fn bump(&mut self, _w: &mut World, _me: ActorId, _other: ActorId) {}
}

pub struct Entity {
    pub name: String,
    pub class: String,
    pub tag: String,
    pub event: String,
    pub pos: Vec3,
    /// Pitch, yaw, roll in the game's units (65536 = full turn).
    pub rot: [i32; 3],
    /// How far it moved during the last tick; whatever stands on it moves along.
    pub moved: Vec3,
    pub radius: f32,
    pub height: f32,
    /// For box-shaped collision (HP1 `CollideType` 2): half-size across.
    pub width: Option<f32>,
    pub collide_actors: bool,
    pub block_players: bool,
    pub is_pawn: bool,
    pub is_player: bool,
    pub hidden: bool,
    /// Brush key (into `LevelFile::brushes`) and pre-pivot of brush objects such as movers.
    pub brush: Option<(String, Vec3)>,
    /// What this pawn stands on.
    pub base: Option<ActorId>,
    pub sound_radius: f32,
    pub sound_volume: f32,
    pub behaviour: Option<Box<dyn Behaviour>>,
}

fn name_value(v: Option<&str>) -> String {
    v.filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("None")).unwrap_or("").to_owned()
}

impl Entity {
    pub fn from_props(name: &str, class: &str, p: &ActorProps, brush: Option<String>) -> Self {
        let short = class.rsplit('.').next().unwrap_or(class);
        let tag = name_value(p.get("Tag").and_then(|v| v.as_str()));
        Self {
            name: name.to_owned(),
            class: class.to_owned(),
            // Objects without a tag answer to their class name.
            tag: if tag.is_empty() { short.to_owned() } else { tag },
            event: name_value(p.get("Event").and_then(|v| v.as_str())),
            pos: Vec3::from(p.vec3("Location")),
            rot: p.rotator("Rotation"),
            moved: Vec3::ZERO,
            radius: p.f32("CollisionRadius", 22.0),
            height: p.f32("CollisionHeight", 22.0),
            width: (p.byte("CollideType", 0) == 2)
                .then(|| p.f32("CollisionWidth", p.f32("CollisionRadius", 22.0))),
            collide_actors: p.bool("bCollideActors"),
            block_players: p.bool("bBlockPlayers"),
            is_pawn: false,
            is_player: false,
            hidden: p.bool("bHidden"),
            brush: brush.map(|b| (b, Vec3::from(p.vec3("PrePivot")))),
            base: None,
            sound_radius: (p.byte("SoundRadius", 32) as f32 + 1.0) * 25.0,
            sound_volume: (p.byte("SoundVolume", 128) as f32 / 255.0).min(1.0),
            behaviour: None,
        }
    }

    /// A walking character placed by code (the player, for now).
    pub fn pawn(name: &str, class: &str, pos: Vec3, radius: f32, height: f32, player: bool) -> Self {
        Self {
            name: name.to_owned(),
            class: class.to_owned(),
            tag: class.rsplit('.').next().unwrap_or(class).to_owned(),
            event: String::new(),
            pos,
            rot: [0; 3],
            moved: Vec3::ZERO,
            radius,
            height,
            width: None,
            collide_actors: true,
            block_players: false,
            is_pawn: true,
            is_player: player,
            hidden: false,
            brush: None,
            base: None,
            sound_radius: 825.0,
            sound_volume: 1.0,
            behaviour: None,
        }
    }

    /// Brush space → world: rotate the pre-pivoted point, then move to the location.
    pub fn brush_to_world(&self) -> (Mat3, Vec3) {
        let r = Mat3::from_mat4(rotator_matrix(self.rot[0], self.rot[1], self.rot[2]));
        let pivot = self.brush.as_ref().map_or(Vec3::ZERO, |b| b.1);
        (r, self.pos - r * pivot)
    }

    /// Whether a pawn box at `pos` with half-size `extent` touches this object's collision.
    fn touches(&self, pos: Vec3, extent: Vec3) -> bool {
        let d = pos - self.pos;
        if d.z.abs() > self.height + extent.z {
            return false;
        }
        match self.width {
            Some(w) => {
                let yaw = self.rot[1] as f32 * crate::units::ROT_TO_RAD;
                let (s, c) = yaw.sin_cos();
                let (x, y) = (d.x * c + d.y * s, -d.x * s + d.y * c);
                x.abs() <= self.radius + extent.x && y.abs() <= w + extent.y
            }
            None => d.truncate().length() <= self.radius + extent.x,
        }
    }
}

pub struct World {
    pub level: LevelFile,
    pub classes: ClassTable,
    /// Index = the level's actor index; spawned objects come after.
    pub actors: Vec<Option<Entity>>,
    pub time: f32,
    pub outputs: Vec<Output>,
    touching: HashSet<(ActorId, ActorId)>,
}

impl World {
    pub fn new(level: LevelFile, classes: ClassTable) -> Self {
        let mut actors = Vec::with_capacity(level.actors.len());
        for a in &level.actors {
            actors.push(a.as_ref().map(|a| {
                let p = ActorProps::new(a, &classes);
                let mut e = Entity::from_props(&a.name, &a.class, &p, a.brush.clone());
                e.behaviour = crate::behaviours::make(&a.class, &p, &classes);
                e
            }));
        }
        Self { level, classes, actors, time: 0.0, outputs: Vec::new(), touching: HashSet::new() }
    }

    pub fn spawn(&mut self, e: Entity) -> ActorId {
        self.actors.push(Some(e));
        self.actors.len() - 1
    }

    pub fn get(&self, id: ActorId) -> Option<&Entity> {
        self.actors.get(id)?.as_ref()
    }

    pub fn get_mut(&mut self, id: ActorId) -> Option<&mut Entity> {
        self.actors.get_mut(id)?.as_mut()
    }

    /// Runs `f` on an object's behaviour while the behaviour is out of the world, so it can
    /// change the world freely (including itself through `get_mut`).
    pub fn with_behaviour(&mut self, id: ActorId, f: impl FnOnce(&mut dyn Behaviour, &mut World)) {
        let Some(mut b) = self.get_mut(id).and_then(|e| e.behaviour.take()) else { return };
        f(b.as_mut(), self);
        if let Some(e) = self.get_mut(id)
            && e.behaviour.is_none()
        {
            e.behaviour = Some(b);
        }
    }

    fn tagged(&self, tag: &str) -> Vec<ActorId> {
        if tag.is_empty() {
            return Vec::new();
        }
        self.actors
            .iter()
            .enumerate()
            .filter(|(_, e)| e.as_ref().is_some_and(|e| e.tag.eq_ignore_ascii_case(tag)))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn trigger_event(&mut self, event: &str, instigator: Option<ActorId>) {
        for id in self.tagged(event) {
            self.with_behaviour(id, |b, w| b.trigger(w, id, instigator));
        }
    }

    pub fn untrigger_event(&mut self, event: &str, instigator: Option<ActorId>) {
        for id in self.tagged(event) {
            self.with_behaviour(id, |b, w| b.untrigger(w, id, instigator));
        }
    }

    pub fn bump(&mut self, id: ActorId, by: ActorId) {
        self.with_behaviour(id, |b, w| b.bump(w, id, by));
    }

    pub fn sound(&mut self, from: ActorId, path: &str) {
        let Some(e) = self.get(from) else { return };
        if path.is_empty() {
            return;
        }
        let out = Output::Sound {
            path: path.to_owned(),
            at: e.pos,
            radius: e.sound_radius,
            volume: e.sound_volume,
        };
        self.outputs.push(out);
    }

    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
        for e in self.actors.iter_mut().flatten() {
            e.moved = Vec3::ZERO;
        }
        for id in 0..self.actors.len() {
            if self.get(id).is_some_and(|e| e.behaviour.is_some()) {
                self.with_behaviour(id, |b, w| b.tick(w, id, dt));
            }
        }
        self.update_touches();
    }

    /// Sends touch / untouch to objects whose collision a pawn entered or left.
    fn update_touches(&mut self) {
        let pawns: Vec<(ActorId, Vec3, Vec3)> = self
            .actors
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let e = e.as_ref()?;
                e.is_pawn.then(|| (i, e.pos, Vec3::new(e.radius, e.radius, e.height)))
            })
            .collect();
        let mut now = HashSet::new();
        for (i, e) in self.actors.iter().enumerate() {
            let Some(e) = e else { continue };
            if !e.collide_actors || e.is_pawn || e.brush.is_some() || e.behaviour.is_none() {
                continue;
            }
            for &(p, pos, extent) in &pawns {
                if e.touches(pos, extent) {
                    now.insert((i, p));
                }
            }
        }
        let started: Vec<_> = now.difference(&self.touching).copied().collect();
        let ended: Vec<_> = self.touching.difference(&now).copied().collect();
        self.touching = now;
        for (i, p) in started {
            self.with_behaviour(i, |b, w| b.touch(w, i, p));
        }
        for (i, p) in ended {
            self.with_behaviour(i, |b, w| b.untouch(w, i, p));
        }
    }

    /// Stops sending touches between this object and anything (used by once-only triggers).
    pub fn set_collide_actors(&mut self, id: ActorId, on: bool) {
        if let Some(e) = self.get_mut(id) {
            e.collide_actors = on;
        }
    }

    /// Collision against the level and the solid objects in it, ignoring `ignore`.
    pub fn collider(&self, ignore: Option<ActorId>) -> WorldCollider<'_> {
        WorldCollider { world: self, ignore, only: None }
    }

    /// Collision against one brush object alone.
    pub fn brush_collider(&self, id: ActorId) -> WorldCollider<'_> {
        WorldCollider { world: self, ignore: None, only: Some(id) }
    }
}

pub struct WorldCollider<'a> {
    world: &'a World,
    ignore: Option<ActorId>,
    only: Option<ActorId>,
}

/// A box swept against an axis-aligned box: entry time and face normal.
fn sweep_aabb(start: Vec3, end: Vec3, extent: Vec3, center: Vec3, half: Vec3) -> Option<(f32, Vec3)> {
    let d = end - start;
    let size = half + extent;
    let (mut t_in, mut t_out, mut normal) = (-1.0f32, 1.0f32, Vec3::ZERO);
    for k in 0..3 {
        let (lo, hi) = (center[k] - size[k], center[k] + size[k]);
        if d[k].abs() < 1e-6 {
            if start[k] < lo || start[k] > hi {
                return None;
            }
            continue;
        }
        let (mut a, mut b) = ((lo - start[k]) / d[k], (hi - start[k]) / d[k]);
        let mut n = Vec3::ZERO;
        n[k] = -d[k].signum();
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        if a > t_in {
            t_in = a;
            normal = n;
        }
        t_out = t_out.min(b);
        if t_in > t_out {
            return None;
        }
    }
    // Starting inside doesn't block, so pawns can walk out of each other.
    (0.0..=1.0).contains(&t_in).then(|| ((t_in - 0.1 / d.length().max(1e-3)).max(0.0), normal))
}

impl WorldCollider<'_> {
    fn brushes(&self) -> impl Iterator<Item = (ActorId, &Entity, &hp1_data::level::Model)> {
        self.world.actors.iter().enumerate().filter_map(move |(i, e)| {
            let e = e.as_ref()?;
            if Some(i) == self.ignore || self.only.is_some_and(|o| o != i) {
                return None;
            }
            let (key, _) = e.brush.as_ref()?;
            Some((i, e, &self.world.level.brushes.get(key)?.model))
        })
    }
}

impl Collider for WorldCollider<'_> {
    fn trace(&self, start: Vec3, end: Vec3, extent: Vec3) -> Option<Hit> {
        let mut best =
            if self.only.is_none() { trace_model(&self.world.level.model, start, end, extent) } else { None };
        let mut keep = |h: Hit| {
            if best.is_none_or(|b| h.time < b.time) {
                best = Some(h);
            }
        };
        for (i, e, model) in self.brushes() {
            let (r, t) = e.brush_to_world();
            let inv = r.transpose();
            let local_extent =
                (Mat3::from_cols(inv.x_axis.abs(), inv.y_axis.abs(), inv.z_axis.abs())) * extent;
            if let Some(h) = trace_model(model, inv * (start - t), inv * (end - t), local_extent) {
                keep(Hit { normal: (r * h.normal).normalize_or_zero(), actor: Some(i), ..h });
            }
        }
        if self.only.is_none() {
            for (i, e) in self.world.actors.iter().enumerate() {
                let Some(e) = e else { continue };
                if Some(i) == self.ignore || !e.block_players || !e.collide_actors || e.brush.is_some() {
                    continue;
                }
                let half = Vec3::new(e.radius, e.radius, e.height);
                if let Some((time, normal)) = sweep_aabb(start, end, extent, e.pos, half) {
                    keep(Hit { time, normal, node: 0, actor: Some(i) });
                }
            }
        }
        best
    }

    fn overlaps(&self, pos: Vec3, extent: Vec3) -> bool {
        if self.only.is_none() && overlaps_model(&self.world.level.model, pos, extent) {
            return true;
        }
        self.brushes().any(|(_, e, model)| {
            let (r, t) = e.brush_to_world();
            let inv = r.transpose();
            let local_extent =
                (Mat3::from_cols(inv.x_axis.abs(), inv.y_axis.abs(), inv.z_axis.abs())) * extent;
            overlaps_model(model, inv * (pos - t), local_extent)
        })
    }
}
