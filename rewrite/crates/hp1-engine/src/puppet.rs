//! HP1 characters and Harry while a cutscene drives them (`specs/cutscenes/CutScene.md`,
//! "Characters in a cutscene" and "Harry in a cutscene").
//!
//! A puppet is made the first time a cutscene tells an object to do something. While it is
//! busy, the object's own behaviour (patrolling, the player's control) waits.

use glam::{Vec3, Vec3Swizzles};
use hp1_data::level::Actor;

use crate::collision::move_smooth;
use crate::props::ActorProps;
use crate::units::ROT_TO_RAD;
use crate::walk::{Body, Movement};
use crate::world::{ActorId, Animation, World};

/// Facing within this many rotation units counts as facing the target.
const FACING: f32 = 2000.0;
/// How often a walk checks whether it is done, and talking whether its time is up.
const WALK_POLL: f32 = 0.1;
const TALK_POLL: f32 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Harry,
    Character,
}

/// Where an animation or a release leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Back {
    /// Its own behaviour again.
    Free,
    /// Standing still for the cutscene.
    Still,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TalkPhase {
    Start,
    Loop,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum EmotePhase {
    /// Start animation playing (`left` until it ends).
    Start { left: f32 },
    /// About to loop the loop animation.
    Loop,
    /// Waiting for the emote's time to run out, checking every `poll`.
    Wait { poll: f32 },
    /// Stop animation playing.
    Stop { left: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Free,
    Still,
    Walk {
        dest: ActorId,
        timeout: f32,
        arrived: bool,
        poll: f32,
    },
    Turn {
        target: ActorId,
    },
    Animate {
        left: f32,
        back: Back,
    },
    Talk {
        left: f32,
        phase: TalkPhase,
        phase_left: f32,
        poll: f32,
    },
    /// Emoting; `turning` while it turns to Harry before going on.
    Emote {
        phase: EmotePhase,
        turning: bool,
    },
}

/// One of a character's emotes: start, loop and stop animations ("" = none).
#[derive(Clone, Debug, Default, PartialEq)]
struct Emote {
    start: String,
    looping: String,
    stop: String,
}

pub struct Puppet {
    pub kind: Kind,
    mode: Mode,
    /// What a captured character was doing (None: never captured).
    saved: Option<Back>,
    /// Set when released back to its own behaviour, which then starts over.
    pub(crate) restart: bool,
    notify: Option<(ActorId, String)>,
    idle_anim: String,
    /// What it loops standing on its own (`idleAnimName`, else its starting animation).
    own_idle: String,
    walk_anim: String,
    talk_anim: String,
    talk_start: String,
    talk_end: String,
    /// The character's emotes by name.
    emotes: Vec<(String, Emote)>,
    /// The emote last chosen; an unknown name plays this one again, as in the original.
    emote: Emote,
    /// Emoting started and its stop animation hasn't begun.
    emoting: bool,
    /// The emote's time left (counts down while emoting).
    emote_left: f32,
    /// What it was doing before the emote, its physics and its facing.
    emote_back: Option<(Mode, bool, f32)>,
    movement: Movement,
    turn_rate: f32,
    body: Body,
    /// Walking physics (gravity, floors) on. Turning and a captured Harry switch it off.
    physics: bool,
    yaw: f32,
    want_yaw: f32,
    anim: Animation,
}

fn yaw_to(from: Vec3, to: Vec3) -> Option<f32> {
    let d = (to - from).xy();
    (d.length_squared() > 1e-6).then(|| d.y.atan2(d.x) / ROT_TO_RAD)
}

/// Shortest signed turn from `a` to `b`, in rotation units.
fn turn(a: f32, b: f32) -> f32 {
    (b - a + 32768.0).rem_euclid(65536.0) - 32768.0
}

impl Puppet {
    fn new(w: &World, id: ActorId, kind: Kind) -> Self {
        let e = w.get(id).expect("puppets are made for objects in the level");
        let actor = w.actor_data(id).cloned().unwrap_or_else(|| Actor {
            name: e.name.clone(),
            class: e.class.clone(),
            props: Default::default(),
            brush: None,
        });
        let p = ActorProps::new(&actor, &w.classes);
        let or = |key: &str, other: &str, fallback: &str| {
            let v = p.text(key);
            if !v.is_empty() {
                return v;
            }
            let v = p.text(other);
            if v.is_empty() { fallback.to_owned() } else { v }
        };
        let movement = Movement {
            ground_speed: p.f32("GroundSpeed", 320.0),
            step_height: p.f32("MaxStepHeight", 25.0),
            ..Movement::default()
        };
        let yaw = e.rot[1] as f32;
        let mut body = Body::new(e.pos, e.radius, e.height);
        body.mode = crate::walk::Mode::Walking;
        Self {
            kind,
            mode: Mode::Free,
            saved: None,
            restart: false,
            notify: None,
            idle_anim: or("CutIdleAnim", "idleAnimName", "Breath"),
            own_idle: or("idleAnimName", "AnimSequence", ""),
            walk_anim: or("CutWalkAnim", "walkAnimName", "Walk"),
            talk_anim: p.text("CutTalkAnim"),
            talk_start: p.text("CutTalkStartAnim"),
            talk_end: p.text("CutTalkEndAnim"),
            emotes: (0..6)
                .filter_map(|i| {
                    let key =
                        |f: &str| if i == 0 { format!("Emotes.{f}") } else { format!("Emotes[{i}].{f}") };
                    let name = p.text(&key("name"));
                    let e = Emote {
                        start: p.text(&key("start")),
                        looping: p.text(&key("loop")),
                        stop: p.text(&key("stop")),
                    };
                    (!name.is_empty() || e != Emote::default()).then_some((name, e))
                })
                .collect(),
            emote: Emote::default(),
            emoting: false,
            emote_left: 0.0,
            emote_back: None,
            movement,
            turn_rate: p.rotator("RotationRate")[1].abs() as f32,
            body,
            physics: true,
            yaw,
            want_yaw: yaw,
            anim: e.anim.clone().unwrap_or_else(|| Animation::looping(&p.text("AnimSequence"))),
        }
    }

    /// Whether the cutscene drives it (rather than its own behaviour).
    pub fn busy(&self) -> bool {
        self.mode != Mode::Free
    }

    /// Takes over from its own behaviour: start from where it is and what it plays.
    fn wake(&mut self, w: &World, id: ActorId) {
        if self.mode != Mode::Free {
            return;
        }
        let Some(e) = w.get(id) else { return };
        self.body.pos = e.pos;
        self.body.vel = Vec3::ZERO;
        self.yaw = e.rot[1] as f32;
        self.want_yaw = self.yaw;
        if let Some(a) = &e.anim {
            self.anim = a.clone();
        }
    }

    /// Loops `name`; one it doesn't have, or the one already looping, changes nothing.
    fn loop_anim(&mut self, w: &World, id: ActorId, name: &str) {
        if !w.has_anim(id, name) || (self.anim.looping && self.anim.name.eq_ignore_ascii_case(name)) {
            return;
        }
        self.anim = Animation::looping(name);
    }

    /// Plays `name` once; returns how long until it ends. One it doesn't have leaves the
    /// current animation playing to the end of its loop, and that is what is waited for.
    fn play_once(&mut self, w: &World, id: ActorId, name: &str) -> f32 {
        if w.has_anim(id, name) {
            self.anim = Animation { name: name.to_owned(), time: 0.0, looping: false };
            return w.sequence(id, name).map_or(0.0, |s| s.once());
        }
        let Some(s) = w.sequence(id, &self.anim.name) else { return 0.0 };
        if self.anim.looping {
            self.anim.time %= s.cycle();
            self.anim.looping = false;
        }
        (s.once() - self.anim.time).max(0.0)
    }

    /// Stands still looping its idle animation (Harry: `breath`, dropped onto the floor).
    fn idle(&mut self, w: &World, id: ActorId) {
        match self.kind {
            Kind::Character => {
                let idle = self.idle_anim.clone();
                self.loop_anim(w, id, &idle);
            }
            Kind::Harry => {
                self.physics = false;
                self.body.pos =
                    move_smooth(&w.collider(Some(id)), self.body.pos, Vec3::Z * -100.0, self.body.extent);
                self.loop_anim(w, id, "breath");
            }
        }
        self.mode = Mode::Still;
    }

    fn walk(&mut self, w: &World, id: ActorId, dest: ActorId, dt: f32) -> bool {
        let Some(goal) = w.get(dest).map(|e| e.pos) else { return true };
        let collider = w.collider(Some(id));
        let extent = self.body.extent;
        let cur = self.body.pos;
        let flat = Vec3::new(goal.x, goal.y, cur.z);
        let heading = (flat - cur).normalize_or_zero();
        if let Some(y) = yaw_to(cur, flat) {
            self.want_yaw = y;
        }
        let mut delta = heading * self.movement.ground_speed * dt;
        // A slight lift helps up stairs.
        delta.z += 0.1;
        let reach = delta.length() >= (flat - cur).length();
        let step = if reach { flat - cur } else { delta };
        // Up enough to clear a stair, across, and back down.
        let mut p = move_smooth(&collider, cur, Vec3::Z * 15.0, extent);
        p = move_smooth(&collider, p, step, extent);
        self.body.pos = move_smooth(&collider, p, Vec3::Z * -15.0, extent);
        reach
    }

    /// One tick; returns the cue to give when a move or turn is done.
    pub(crate) fn tick(&mut self, w: &mut World, id: ActorId, dt: f32) -> Option<(ActorId, String)> {
        if self.mode == Mode::Free {
            // One with no behaviour of its own still turns back after an emote.
            if w.get(id).is_some_and(|e| e.behaviour.is_none()) && turn(self.yaw, self.want_yaw).abs() > 0.5 {
                self.turn_towards(dt);
                if let Some(e) = w.get_mut(id) {
                    e.rot = [0, self.yaw.round() as i32 & 0xffff, 0];
                }
            }
            return None;
        }
        // Whatever moved it since (a cutscene's TELEPORT) counts.
        if let Some(e) = w.get(id) {
            self.body.pos = e.pos;
        }
        let mut cue = None;
        match self.mode {
            Mode::Free | Mode::Still => {}
            Mode::Walk { dest, mut timeout, mut arrived, mut poll } => {
                if !arrived {
                    arrived = self.walk(w, id, dest, dt);
                    timeout -= dt;
                    if timeout < 0.0 && !arrived {
                        // Taking too long: put it there and onto the floor.
                        if let Some(goal) = w.get(dest).map(|e| e.pos) {
                            let c = w.collider(Some(id));
                            self.body.pos = move_smooth(&c, goal, Vec3::Z * -100.0, self.body.extent);
                        }
                        arrived = true;
                    }
                }
                poll -= dt;
                self.mode = Mode::Walk { dest, timeout, arrived, poll };
                if poll <= 0.0 {
                    if arrived {
                        cue = self.notify.take();
                        self.idle(w, id);
                    } else {
                        self.mode = Mode::Walk { dest, timeout, arrived, poll: WALK_POLL };
                    }
                }
            }
            Mode::Turn { target } => {
                if let Some(y) = w.get(target).and_then(|t| yaw_to(self.body.pos, t.pos)) {
                    self.want_yaw = y;
                }
                self.turn_towards(dt);
                if turn(self.yaw, self.want_yaw).abs() < FACING {
                    cue = self.notify.take();
                    self.mode = Mode::Still;
                    if self.kind == Kind::Character {
                        let idle = self.idle_anim.clone();
                        self.loop_anim(w, id, &idle);
                    }
                }
            }
            Mode::Animate { left, back } => {
                if left > dt {
                    self.mode = Mode::Animate { left: left - dt, back };
                } else {
                    match back {
                        Back::Still => self.idle(w, id),
                        Back::Free => self.mode = Mode::Free,
                    }
                }
            }
            Mode::Talk { left, phase, phase_left, poll } => {
                let left = left - dt;
                self.mode = Mode::Talk { left, phase, phase_left: phase_left - dt, poll: poll - dt };
                match phase {
                    TalkPhase::Start if phase_left - dt <= 0.0 => {
                        let a = self.talk_anim.clone();
                        self.loop_anim(w, id, &a);
                        self.mode = Mode::Talk { left, phase: TalkPhase::Loop, phase_left: 0.0, poll: 0.0 };
                    }
                    TalkPhase::Loop if poll - dt <= 0.0 => {
                        if left > 0.0 {
                            self.mode = Mode::Talk { left, phase, phase_left: 0.0, poll: TALK_POLL };
                        } else {
                            let a = self.talk_end.clone();
                            let end = self.play_once(w, id, &a);
                            self.mode =
                                Mode::Talk { left, phase: TalkPhase::End, phase_left: end, poll: 0.0 };
                        }
                    }
                    TalkPhase::End if phase_left - dt <= 0.0 => self.idle(w, id),
                    _ => {}
                }
            }
            Mode::Emote { phase, turning } => self.emote_tick(w, id, phase, turning, dt),
        }
        if self.physics {
            let c = w.collider(Some(id));
            self.body.tick(&c, &self.movement, Vec3::ZERO, false, dt);
        }
        if !matches!(self.mode, Mode::Turn { .. } | Mode::Emote { turning: true, .. }) {
            self.turn_towards(dt);
        }
        self.anim.time += dt;
        if !self.anim.looping
            && let Some(s) = w.sequence(id, &self.anim.name)
        {
            self.anim.time = self.anim.time.min(s.once());
        }
        self.write(w, id);
        cue
    }

    /// One tick of an emote (see the spec's "Emotes").
    fn emote_tick(&mut self, w: &mut World, id: ActorId, mut phase: EmotePhase, mut turning: bool, dt: f32) {
        self.emote_left -= dt;
        if turning {
            let harry = w.player.and_then(|h| w.get(h)).map(|e| e.pos);
            if let Some(y) = harry.and_then(|h| yaw_to(self.body.pos, h)) {
                self.want_yaw = y;
            }
            self.turn_towards(dt);
            turning = turn(self.yaw, self.want_yaw).abs() >= FACING;
        }
        let mut done = false;
        phase = match phase {
            EmotePhase::Start { left } if turning || left - dt > 0.0 => EmotePhase::Start { left: left - dt },
            EmotePhase::Start { .. } if !self.emote.looping.is_empty() => {
                turning = true;
                EmotePhase::Loop
            }
            EmotePhase::Start { .. } => EmotePhase::Wait { poll: 0.0 },
            EmotePhase::Loop if turning => EmotePhase::Loop,
            EmotePhase::Loop => {
                let a = self.emote.looping.clone();
                self.loop_anim(w, id, &a);
                EmotePhase::Wait { poll: 0.0 }
            }
            EmotePhase::Wait { poll } if poll - dt > 0.0 => EmotePhase::Wait { poll: poll - dt },
            EmotePhase::Wait { .. } if self.emote_left > 0.0 => EmotePhase::Wait { poll: TALK_POLL },
            EmotePhase::Wait { .. } => {
                self.emoting = false;
                if self.emote.stop.is_empty() {
                    done = true;
                    EmotePhase::Wait { poll: 0.0 }
                } else {
                    let a = self.emote.stop.clone();
                    turning = true;
                    EmotePhase::Stop { left: self.play_once(w, id, &a) }
                }
            }
            EmotePhase::Stop { left } if turning || left - dt > 0.0 => EmotePhase::Stop { left: left - dt },
            EmotePhase::Stop { .. } => {
                done = true;
                phase
            }
        };
        self.mode = Mode::Emote { phase, turning };
        if done {
            // Back to what it was doing, turning back to its old facing.
            let (mode, physics, yaw) =
                self.emote_back.take().unwrap_or((Mode::Still, self.physics, self.yaw));
            self.want_yaw = yaw;
            self.physics = physics;
            match mode {
                Mode::Free => {
                    self.mode = Mode::Free;
                    self.restart = true;
                    // Without a behaviour of its own (not patrolling) it stands idle again.
                    if w.get(id).is_some_and(|e| e.behaviour.is_none()) {
                        let a = self.own_idle.clone();
                        self.loop_anim(w, id, &a);
                        self.write(w, id);
                    }
                }
                Mode::Still => self.idle(w, id),
                other => self.mode = other,
            }
        }
    }

    /// Puts its position, facing and animation onto the object.
    fn write(&self, w: &mut World, id: ActorId) {
        if let Some(e) = w.get_mut(id) {
            e.pos = self.body.pos;
            e.rot = [0, self.yaw.round() as i32 & 0xffff, 0];
            e.anim = Some(self.anim.clone());
        }
    }

    fn turn_towards(&mut self, dt: f32) {
        let most = self.turn_rate * dt;
        self.yaw = (self.yaw + turn(self.yaw, self.want_yaw).clamp(-most, most)).rem_euclid(65536.0);
    }
}

// ---------------------------------------------------------------------------------------------
// What cutscenes tell them to do

/// Which kind of puppet an object can be, if any.
pub fn kind_of(w: &World, id: ActorId) -> Option<Kind> {
    let e = w.get(id)?;
    if Some(id) == w.player || w.classes.is_a(&e.class, "HPBase.baseHarry") {
        Some(Kind::Harry)
    } else if w.classes.is_a(&e.class, "HPBase.baseChar") {
        Some(Kind::Character)
    } else {
        None
    }
}

/// Runs `f` on object `id`'s puppet, making it first if needed. Does nothing for objects that
/// can't be puppets.
fn with(w: &mut World, id: ActorId, f: impl FnOnce(&mut Puppet, &mut World)) {
    let mut p = match w.puppets.remove(&id) {
        Some(p) => p,
        None => {
            let Some(kind) = kind_of(w, id) else { return };
            Puppet::new(w, id, kind)
        }
    };
    p.wake(w, id);
    f(&mut p, w);
    if p.busy() {
        p.write(w, id);
    }
    w.puppets.insert(id, p);
}

pub fn capture(w: &mut World, id: ActorId) {
    with(w, id, |p, w| match p.kind {
        // A character only remembers what it was doing.
        Kind::Character => p.saved = Some(if p.mode == Mode::Free { Back::Free } else { Back::Still }),
        Kind::Harry => p.idle(w, id),
    });
}

pub fn release(w: &mut World, id: ActorId) {
    with(w, id, |p, w| match (p.kind, p.saved) {
        (Kind::Harry, _) | (Kind::Character, Some(Back::Free)) => {
            p.mode = Mode::Free;
            p.restart = true;
        }
        (Kind::Character, Some(Back::Still)) => p.idle(w, id),
        // Never captured: it just stops where it is.
        (Kind::Character, None) => p.mode = Mode::Still,
    });
}

/// Walks to `dest`; `notify` is told when it is done.
pub fn walk_to(w: &mut World, id: ActorId, dest: ActorId, notify: (ActorId, String)) {
    with(w, id, |p, w| {
        let Some(goal) = w.get(dest).map(|e| e.pos) else { return };
        let dist = (goal - p.body.pos).xy().length();
        let speed = p.movement.ground_speed;
        let timeout = if speed > 0.0 { dist / speed + 1.0 } else { f32::INFINITY };
        p.notify = Some(notify);
        p.physics = true;
        p.mode = Mode::Walk { dest, timeout, arrived: false, poll: 0.0 };
        let a = if p.kind == Kind::Harry { "run".to_owned() } else { p.walk_anim.clone() };
        p.loop_anim(w, id, &a);
    });
}

/// Turns towards `target`; `notify` is told when it faces it.
pub fn turn_to(w: &mut World, id: ActorId, target: ActorId, notify: (ActorId, String)) {
    with(w, id, |p, _| {
        p.notify = Some(notify);
        p.physics = false;
        p.mode = Mode::Turn { target };
    });
}

/// Faces `target` at once, level.
pub fn face(w: &mut World, id: ActorId, target: Vec3) {
    let Some(pos) = w.get(id).map(|e| e.pos) else { return };
    let Some(yaw) = yaw_to(pos, target) else { return };
    if let Some(e) = w.get_mut(id) {
        e.rot = [0, yaw.round() as i32, 0];
    }
    if let Some(p) = w.puppets.get_mut(&id) {
        p.yaw = yaw;
        p.want_yaw = yaw;
    }
}

pub fn animate(w: &mut World, id: ActorId, name: &str) {
    with(w, id, |p, w| {
        let back = match p.kind {
            Kind::Character => Back::Still,
            Kind::Harry if p.mode == Mode::Free => Back::Free,
            Kind::Harry => Back::Still,
        };
        let left = p.play_once(w, id, name);
        p.mode = Mode::Animate { left, back };
    });
}

/// Talks for `seconds` (characters only).
pub fn talk(w: &mut World, id: ActorId, seconds: f32) {
    if kind_of(w, id) != Some(Kind::Character) {
        return;
    }
    with(w, id, |p, w| {
        let a = p.talk_start.clone();
        let start = p.play_once(w, id, &a);
        p.mode = Mode::Talk { left: seconds, phase: TalkPhase::Start, phase_left: start, poll: 0.0 };
    });
}

/// Whether a character is emoting (its stop animation not begun yet).
pub fn is_emoting(w: &World, id: ActorId) -> bool {
    w.puppets.get(&id).is_some_and(|p| p.emoting)
}

/// Starts emote `name` lasting `seconds` (characters only; "none" or "" does nothing).
pub fn emote(w: &mut World, id: ActorId, name: &str, seconds: f32) {
    if name.is_empty() || name.eq_ignore_ascii_case("none") || kind_of(w, id) != Some(Kind::Character) {
        return;
    }
    with(w, id, |p, w| {
        if p.emoting {
            return;
        }
        if let Some((_, e)) = p.emotes.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            p.emote = e.clone();
        }
        p.emote_back = Some((p.mode, p.physics, p.yaw));
        p.emoting = true;
        p.emote_left = seconds;
        p.physics = false;
        let phase = if p.emote.start.is_empty() {
            if p.emote.looping.is_empty() { EmotePhase::Wait { poll: 0.0 } } else { EmotePhase::Loop }
        } else {
            let a = p.emote.start.clone();
            EmotePhase::Start { left: p.play_once(w, id, &a) }
        };
        let turning = !matches!(phase, EmotePhase::Wait { .. });
        p.mode = Mode::Emote { phase, turning };
    });
}

pub fn set_idle(w: &mut World, id: ActorId, name: &str) {
    with(w, id, |p, _| p.idle_anim = name.to_owned());
}

pub fn set_walk(w: &mut World, id: ActorId, name: &str) {
    with(w, id, |p, _| p.walk_anim = name.to_owned());
}
