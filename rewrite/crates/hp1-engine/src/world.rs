//! The running level: every placed object with its position, collision and behaviour.
//!
//! Objects talk through names (see `specs/engine/Triggers.md`): `trigger_event("X")` calls
//! `trigger` on every object whose tag is X. Each object's behaviour is Rust code chosen by its
//! class when the level starts; objects without one just sit there.
//!
//! Sounds and music are not played here. They are collected in `outputs` for the program to play.

use std::collections::{HashMap, HashSet};

use glam::{Mat3, Vec3};
use hp1_data::ClassTable;
use hp1_data::level::{Actor, LevelFile};

use crate::camera::CutCamera;
use crate::collision::{Collider, Hit, overlaps_model, trace_model};
use crate::dialog::{Lines, NoLines};
use crate::props::ActorProps;
use crate::puppet::Puppet;
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
    /// A dialogue voice, heard everywhere at full volume. A new one cuts off the one before
    /// (the original plays them all in the same sound slot on Harry).
    Voice {
        path: String,
    },
    /// A message (dialogue subtitle) shown for `seconds`.
    Message {
        text: String,
        seconds: f32,
    },
    /// Go to another level.
    ChangeLevel {
        map: String,
    },
    /// A script problem the original reported on the console (e.g. a target not found).
    Note(String),
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
    /// Something a cutscene told to move or turn has finished (cutscenes only).
    fn cue(&mut self, _w: &mut World, _me: ActorId, _cue: &str) {}
}

/// The animation a character plays, for the program to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Animation {
    /// Sequence name, e.g. `Walk`.
    pub name: String,
    /// Seconds into the sequence.
    pub time: f32,
    /// Loops; otherwise it plays once and holds its last frame.
    pub looping: bool,
}

impl Animation {
    pub fn looping(name: &str) -> Self {
        Self { name: name.to_owned(), time: 0.0, looping: true }
    }
}

/// How long a mesh's animation sequence is: `frames` at `rate` frames per second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sequence {
    pub frames: u32,
    pub rate: f32,
}

impl Sequence {
    /// When playing it once ends: at its last frame, or after 0.1 s for a single frame.
    pub fn once(&self) -> f32 {
        if self.frames > 1 && self.rate > 0.0 { (self.frames - 1) as f32 / self.rate } else { 0.1 }
    }
    /// One loop.
    pub fn cycle(&self) -> f32 {
        if self.frames > 1 && self.rate > 0.0 { self.frames as f32 / self.rate } else { 0.1 }
    }
}

/// The screen fade (`specs/cutscenes/CutScene.md`): 0 is normal, −1 black.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fade {
    glow: f32,
    rate: f32,
}

impl Fade {
    /// Black at once, then brighter over `seconds`.
    pub fn fade_in(&mut self, seconds: f32) {
        self.glow = -1.0;
        self.rate = if seconds > 0.0 { 1.0 / seconds } else { f32::MAX };
    }
    /// From normal to black over `seconds`.
    pub fn fade_out(&mut self, seconds: f32) {
        self.glow = 0.0;
        self.rate = if seconds > 0.0 { -1.0 / seconds } else { -f32::MAX };
    }
    fn tick(&mut self, dt: f32) {
        if self.rate == 0.0 {
            return;
        }
        // The original steps the fade by at most 0.1 s per tick.
        self.glow += self.rate * dt.min(0.1);
        if self.glow <= -1.0 {
            self.glow = -1.0;
            self.rate = 0.0;
        } else if self.glow >= 0.0 {
            self.glow = 0.0;
            self.rate = 0.0;
        }
    }
    /// How bright the screen is drawn, 0..1.
    pub fn brightness(&self) -> f32 {
        1.0 + self.glow
    }
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
    /// Set by characters whose behaviour chooses their animation; None plays the map's own.
    pub anim: Option<Animation>,
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
            anim: None,
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
            anim: None,
            behaviour: None,
        }
    }

    /// Brush space → world: rotate the pre-pivoted point, then move to the location.
    pub fn brush_to_world(&self) -> (Mat3, Vec3) {
        let r = Mat3::from_mat4(rotator_matrix(self.rot[0], self.rot[1], self.rot[2]));
        let pivot = self.brush.as_ref().map_or(Vec3::ZERO, |b| b.1);
        (r, self.pos - r * pivot)
    }

    /// Half-size of the axis-aligned box that blocks movement. Box-shaped collision is
    /// `radius` long and `width` wide along its facing; turned, it takes the box around it
    /// (exact for quarter turns, which is how the maps use it).
    pub fn blocking_half(&self) -> Vec3 {
        match self.width {
            Some(w) => {
                let (s, c) = (self.rot[1] as f32 * crate::units::ROT_TO_RAD).sin_cos();
                let (s, c) = (s.abs(), c.abs());
                Vec3::new(c * self.radius + s * w, s * self.radius + c * w, self.height)
            }
            None => Vec3::new(self.radius, self.radius, self.height),
        }
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
    /// Objects spawned by `spawn_actor` since the program last looked, for it to load their meshes.
    pub spawned: Vec<ActorId>,
    /// Harry, the player.
    pub player: Option<ActorId>,
    /// The camera the player looks through while a cutscene has it.
    pub camera: CutCamera,
    pub fade: Fade,
    /// Cutscene mode: the HUD draws its black bars and Harry takes no input.
    pub cut_mode: bool,
    /// Dialogue text and voices.
    pub lines: Box<dyn Lines>,
    /// Cutscene bars and spoken lines on screen.
    pub hud: crate::hud::Hud,
    /// Report every cutscene line as it runs (as notes).
    pub trace_cutscenes: bool,
    /// Characters and Harry while cutscenes drive them.
    pub(crate) puppets: HashMap<ActorId, Puppet>,
    /// Animation lengths per object, by lower-case sequence name (from the program's meshes).
    sequences: HashMap<ActorId, HashMap<String, Sequence>>,
    touching: HashSet<(ActorId, ActorId)>,
    /// Settings of spawned objects (placed ones are in `level.actors`).
    extra: HashMap<ActorId, Actor>,
    rng: u64,
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
        let camera = CutCamera::new(&level, &classes);
        Self {
            level,
            classes,
            actors,
            time: 0.0,
            outputs: Vec::new(),
            spawned: Vec::new(),
            player: None,
            camera,
            fade: Fade::default(),
            cut_mode: false,
            lines: Box::new(NoLines),
            hud: Default::default(),
            trace_cutscenes: false,
            puppets: HashMap::new(),
            sequences: HashMap::new(),
            touching: HashSet::new(),
            extra: HashMap::new(),
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// The Harry placed in the level, which the original reuses as the player.
    pub fn placed_harry(&self) -> Option<ActorId> {
        self.actors
            .iter()
            .position(|e| e.as_ref().is_some_and(|e| self.classes.is_a(&e.class, "HPBase.baseHarry")))
    }

    /// The object with this name (letter case doesn't matter).
    pub fn find_named(&self, name: &str) -> Option<ActorId> {
        self.actors.iter().position(|e| e.as_ref().is_some_and(|e| e.name.eq_ignore_ascii_case(name)))
    }

    /// Tells the world how long the animations of object `id`'s mesh are.
    pub fn set_sequences(&mut self, id: ActorId, list: impl IntoIterator<Item = (String, Sequence)>) {
        let map = list.into_iter().map(|(n, s)| (n.to_ascii_lowercase(), s)).collect();
        self.sequences.insert(id, map);
    }

    /// Whether object `id` can play animation `name`. Without known lengths any name goes.
    pub(crate) fn has_anim(&self, id: ActorId, name: &str) -> bool {
        !name.is_empty() && self.sequences.get(&id).is_none_or(|m| m.contains_key(&name.to_ascii_lowercase()))
    }

    pub(crate) fn sequence(&self, id: ActorId, name: &str) -> Option<Sequence> {
        self.sequences.get(&id)?.get(&name.to_ascii_lowercase()).copied()
    }

    /// Whether a cutscene is driving this object right now (its own behaviour should wait).
    pub fn puppet_busy(&self, id: ActorId) -> bool {
        self.puppets.get(&id).is_some_and(Puppet::busy)
    }

    /// True once after a cutscene released this object back to its own behaviour, which should
    /// then start over what it was doing.
    pub fn take_restart(&mut self, id: ActorId) -> bool {
        self.puppets.get_mut(&id).is_some_and(|p| std::mem::take(&mut p.restart))
    }

    pub fn spawn(&mut self, e: Entity) -> ActorId {
        self.actors.push(Some(e));
        self.actors.len() - 1
    }

    /// Adds an object as if it had been placed with these settings, behaviour included.
    ///
    /// Like the original engine, a solid character can't be spawned into another character or
    /// into a brush object (HP1's pawns refuse to overlap any other pawn): then nothing is
    /// spawned and None comes back.
    pub fn spawn_actor(&mut self, actor: Actor) -> Option<ActorId> {
        let e = {
            let p = ActorProps::new(&actor, &self.classes);
            let mut e = Entity::from_props(&actor.name, &actor.class, &p, None);
            let solid = p.bool("bBlockActors") || p.bool("bBlockPlayers");
            if solid
                && !p.bool("bIsPlayer")
                && self.classes.is_a(&actor.class, "Engine.Pawn")
                && self.occupied(&e)
            {
                return None;
            }
            e.behaviour = crate::behaviours::make(&actor.class, &p, &self.classes);
            e
        };
        let id = self.spawn(e);
        self.extra.insert(id, actor);
        self.spawned.push(id);
        Some(id)
    }

    /// Whether a character placed as `e` would overlap a solid character or a brush object.
    fn occupied(&self, e: &Entity) -> bool {
        let extent = Vec3::new(e.radius, e.radius, e.height);
        let pawn_in_way = self.actors.iter().flatten().any(|o| {
            let pawn = o.is_pawn || self.classes.is_a(&o.class, "Engine.Pawn");
            pawn && (o.block_players || o.is_player)
                && (o.pos - e.pos).truncate().length() < o.radius + e.radius
                && (o.pos.z - e.pos.z).abs() < o.height + e.height
        });
        pawn_in_way
            || self.collider(None).brushes().any(|(id, _, _)| self.brush_collider(id).overlaps(e.pos, extent))
    }

    /// An object's settings: placed in the level or spawned later.
    pub fn actor_data(&self, id: ActorId) -> Option<&Actor> {
        self.level.actors.get(id).and_then(Option::as_ref).or_else(|| self.extra.get(&id))
    }

    /// A random number in 0..n (n > 0).
    pub fn rand(&mut self, n: usize) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % n.max(1) as u64) as usize
    }

    /// Takes an object out of the level for good.
    pub fn remove(&mut self, id: ActorId) {
        if let Some(slot) = self.actors.get_mut(id) {
            *slot = None;
            self.outputs.push(Output::AmbientStop { actor: id });
        }
        self.extra.remove(&id);
        self.puppets.remove(&id);
        self.sequences.remove(&id);
        self.touching.retain(|&(a, b)| a != id && b != id);
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
            if let Some(mut p) = self.puppets.remove(&id) {
                let cue = if self.get(id).is_some() { p.tick(self, id, dt) } else { None };
                self.puppets.insert(id, p);
                self.send_cue(cue);
            }
            if self.camera.actor == Some(id) {
                let cue = crate::cutscene::tick_camera(self, dt);
                self.send_cue(cue);
            }
        }
        if self.camera.actor.is_none() {
            let cue = crate::cutscene::tick_camera(self, dt);
            self.send_cue(cue);
        }
        self.fade.tick(dt);
        self.hud.tick(dt);
        self.update_touches();
    }

    /// Tells cutscene `to` that cue `cue` was given.
    pub(crate) fn send_cue(&mut self, cue: Option<(ActorId, String)>) {
        if let Some((to, cue)) = cue {
            self.with_behaviour(to, |b, w| b.cue(w, to, &cue));
        }
    }

    pub(crate) fn note(&mut self, text: String) {
        self.outputs.push(Output::Note(text));
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

/// A walker swept against an upright cylinder (how the original collides with characters and
/// other round objects). The walker counts as a cylinder of its half-width too, so the normal
/// points away from the cylinder's axis and walkers slide round each other.
fn sweep_cylinder(
    start: Vec3,
    end: Vec3,
    extent: Vec3,
    center: Vec3,
    radius: f32,
    height: f32,
) -> Option<(f32, Vec3)> {
    let d = end - start;
    let r = radius + extent.x;
    let h = height + extent.z;
    let rel = start - center;
    // Entry and exit times of the side wall (infinite cylinder) and of the top/bottom slab.
    let (a, b, c) =
        (d.x * d.x + d.y * d.y, 2.0 * (rel.x * d.x + rel.y * d.y), rel.x * rel.x + rel.y * rel.y - r * r);
    let (side_in, side_out) = if a < 1e-9 {
        if c > 0.0 {
            return None;
        }
        (f32::NEG_INFINITY, f32::INFINITY)
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None;
        }
        let q = disc.sqrt();
        ((-b - q) / (2.0 * a), (-b + q) / (2.0 * a))
    };
    let (slab_in, slab_out) = if d.z.abs() < 1e-9 {
        if rel.z.abs() > h {
            return None;
        }
        (f32::NEG_INFINITY, f32::INFINITY)
    } else {
        let (t0, t1) = ((-h - rel.z) / d.z, (h - rel.z) / d.z);
        (t0.min(t1), t0.max(t1))
    };
    let t_in = side_in.max(slab_in);
    let t_out = side_out.min(slab_out);
    // Starting inside doesn't block, so characters can walk out of each other.
    if t_in > t_out || !(0.0..=1.0).contains(&t_in) {
        return None;
    }
    let normal = if slab_in > side_in {
        Vec3::Z * -d.z.signum()
    } else {
        let p = rel + d * t_in;
        Vec3::new(p.x, p.y, 0.0).normalize_or_zero()
    };
    Some(((t_in - 0.1 / d.length().max(1e-3)).max(0.0), normal))
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
                let hit = if e.width.is_some() {
                    sweep_aabb(start, end, extent, e.pos, e.blocking_half())
                } else {
                    sweep_cylinder(start, end, extent, e.pos, e.radius, e.height)
                };
                if let Some((time, normal)) = hit {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walkers_hit_cylinders_on_their_round_side() {
        let extent = Vec3::new(22.0, 22.0, 42.0);
        let (start, end) = (Vec3::ZERO, Vec3::new(200.0, 0.0, 0.0));
        // Head on: stops 44 units short of the centre, pushed straight back.
        let (t, n) = sweep_cylinder(start, end, extent, Vec3::new(100.0, 0.0, 0.0), 22.0, 42.0).unwrap();
        assert!((start.lerp(end, t).x - 56.0).abs() < 0.5, "{t}");
        assert!((n - Vec3::NEG_X).length() < 1e-4);
        // Off to one side: the normal leans sideways, so sliding goes round.
        let (_, n) = sweep_cylinder(start, end, extent, Vec3::new(100.0, 20.0, 0.0), 22.0, 42.0).unwrap();
        assert!(n.x < 0.0 && n.y < -0.3, "{n}");
        // Passing by, or starting inside: no hit.
        assert!(sweep_cylinder(start, end, extent, Vec3::new(100.0, 50.0, 0.0), 22.0, 42.0).is_none());
        assert!(sweep_cylinder(start, end, extent, Vec3::new(10.0, 0.0, 0.0), 22.0, 42.0).is_none());
    }

    #[test]
    fn walkers_slide_round_a_standing_character() {
        use crate::walk::{Body, Movement};
        let level = LevelFile {
            name: "Test".into(),
            actors: Vec::new(),
            model: crate::collision::tests::floor(),
            brushes: Default::default(),
            reach_specs: Vec::new(),
        };
        let mut w = World::new(level, ClassTable::default());
        let mut other = Entity::pawn("Other", "Test.Other", Vec3::new(150.0, 8.0, 42.0), 22.0, 40.0, false);
        other.block_players = true;
        w.spawn(other);
        let mut body = Body::new(Vec3::new(0.0, 0.0, 42.5), 22.0, 42.0);
        let m = Movement::default();
        let goal = Vec3::new(300.0, 0.0, 42.0);
        for _ in 0..240 {
            let wish = (goal - body.pos).truncate().normalize_or_zero().extend(0.0);
            body.tick(&w.collider(None), &m, wish, false, 1.0 / 60.0);
        }
        assert!(body.pos.x > 250.0, "got round: {:?}", body.pos);
    }
}
