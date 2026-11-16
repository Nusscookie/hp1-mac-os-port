//! Behaviours of the engine's own object classes: movers, triggers, dispatchers, counters and
//! music events (`specs/engine/Mover.md`, `specs/engine/Triggers.md`).

use glam::Vec3;
use hp1_data::{ClassTable, DataValue};

use crate::props::ActorProps;
use crate::world::{ActorId, Behaviour, Output, World};

/// The behaviour for an object of `class`, if the engine has one.
pub fn make(class: &str, p: &ActorProps, classes: &ClassTable) -> Option<Box<dyn Behaviour>> {
    let is = |c: &str| classes.is_a(class, c);
    if is("Engine.Mover") {
        Some(Box::new(Mover::new(p)))
    } else if is("Engine.Trigger") {
        Some(Box::new(Trigger::new(p)))
    } else if is("Engine.Dispatcher") {
        Some(Box::new(Dispatcher::new(p)))
    } else if is("Engine.Counter") {
        Some(Box::new(Counter { left: p.byte("NumToCount", 2) as u32 }))
    } else if is("Engine.MusicEvent") {
        Some(Box::new(MusicEvent::new(p)))
    } else {
        None
    }
}

fn text(p: &ActorProps, key: &str) -> String {
    p.get(key)
        .and_then(DataValue::as_str)
        .filter(|s| !s.eq_ignore_ascii_case("None"))
        .unwrap_or("")
        .to_owned()
}

fn indexed(key: &str, i: usize) -> String {
    if i == 0 { key.to_owned() } else { format!("{key}[{i}]") }
}

// ---------------------------------------------------------------------------------------------
// Mover

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MoverMode {
    TriggerToggle,
    TriggerOpenTimed,
    TriggerControl,
    TriggerPound,
    BumpOpenTimed,
    StandOpenTimed,
    /// Anything else: only moves when told to by code.
    Still,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    /// Waiting before opening.
    Delay(f32),
    /// Moving from one key to the next; `t` runs 0..1.
    Leg {
        from: usize,
        to: usize,
        t: f32,
    },
    /// Open, waiting to close by itself.
    StayOpen(f32),
}

pub struct Mover {
    keys: Vec<(Vec3, [i32; 3])>,
    base_pos: Vec3,
    base_rot: [i32; 3],
    key: usize,
    phase: Phase,
    opening: bool,
    mode: MoverMode,
    move_time: f32,
    stay_open: f32,
    delay: f32,
    glide: bool,
    encroach: u8,
    once: bool,
    done_for_good: bool,
    /// Triggered and not untriggered yet (TriggerControl, TriggerPound).
    held: bool,
    sounds: [String; 5],
    started: bool,
}

const OPENING: usize = 0;
const OPENED: usize = 1;
const CLOSING: usize = 2;
const CLOSED: usize = 3;
const MOVING: usize = 4;

impl Mover {
    pub fn new(p: &ActorProps) -> Self {
        let count = (p.byte("NumKeys", 2) as usize).clamp(1, 8);
        let keys = (0..count)
            .map(|i| {
                let pos = p.get(&indexed("KeyPos", i)).and_then(DataValue::as_vec3).unwrap_or([0.0; 3]);
                let rot = p.get(&indexed("KeyRot", i)).and_then(DataValue::as_rotator).unwrap_or([0; 3]);
                (Vec3::from(pos), rot)
            })
            .collect();
        let mode = match text(p, "InitialState").to_ascii_lowercase().as_str() {
            "triggertoggle" => MoverMode::TriggerToggle,
            "triggeropentimed" => MoverMode::TriggerOpenTimed,
            "triggercontrol" => MoverMode::TriggerControl,
            "triggerpound" => MoverMode::TriggerPound,
            "bumpopentimed" => MoverMode::BumpOpenTimed,
            "standopentimed" => MoverMode::StandOpenTimed,
            _ => MoverMode::Still,
        };
        Self {
            keys,
            base_pos: Vec3::from(p.vec3("BasePos")),
            base_rot: p.rotator("BaseRot"),
            key: (p.byte("KeyNum", 0) as usize).min(count - 1),
            phase: Phase::Idle,
            opening: false,
            mode,
            move_time: p.f32("MoveTime", 1.0).max(0.01),
            stay_open: p.f32("StayOpenTime", 4.0),
            delay: p.f32("DelayTime", 0.0),
            glide: p.byte("MoverGlideType", 1) == 1,
            encroach: p.byte("MoverEncroachType", 1),
            once: p.bool("bTriggerOnceOnly"),
            done_for_good: false,
            held: false,
            sounds: ["OpeningSound", "OpenedSound", "ClosingSound", "ClosedSound", "MoveAmbientSound"]
                .map(|k| text(p, k)),
            started: false,
        }
    }

    fn last(&self) -> usize {
        self.keys.len() - 1
    }

    fn is_closed(&self) -> bool {
        self.key == 0 && matches!(self.phase, Phase::Idle)
    }

    fn pose(&self, from: usize, to: usize, t: f32) -> (Vec3, [i32; 3]) {
        let a = if self.glide { t * t * (3.0 - 2.0 * t) } else { t };
        let (p0, r0) = self.keys[from];
        let (p1, r1) = self.keys[to];
        let rot = [0, 1, 2].map(|k| self.base_rot[k] + r0[k] + ((r1[k] - r0[k]) as f32 * a).round() as i32);
        (self.base_pos + p0.lerp(p1, a), rot)
    }

    fn start_leg(&mut self, w: &mut World, me: ActorId, opening: bool) {
        // Turning round in the middle of a leg goes back the way it came.
        if let Phase::Leg { from, to, t } = self.phase {
            if self.opening != opening {
                self.phase = Phase::Leg { from: to, to: from, t: 1.0 - t };
            }
        } else {
            let to = if opening { (self.key + 1).min(self.last()) } else { self.key.saturating_sub(1) };
            if to == self.key {
                return;
            }
            self.phase = Phase::Leg { from: self.key, to, t: 0.0 };
            if let Some(e) = w.get(me) {
                let (path, at, radius, volume) =
                    (&self.sounds[MOVING], e.pos, e.sound_radius, e.sound_volume);
                if !path.is_empty() {
                    w.outputs.push(Output::AmbientStart {
                        actor: me,
                        path: path.clone(),
                        at,
                        radius,
                        volume,
                    });
                }
            }
        }
        self.opening = opening;
        w.sound(me, &self.sounds[if opening { OPENING } else { CLOSING }].clone());
        if !opening && let Some(event) = w.get(me).map(|e| e.event.clone()) {
            w.untrigger_event(&event, None);
        }
    }

    fn open(&mut self, w: &mut World, me: ActorId) {
        if self.delay > 0.0 && !matches!(self.phase, Phase::Leg { .. }) {
            self.phase = Phase::Delay(self.delay);
        } else {
            self.start_leg(w, me, true);
        }
    }

    fn close(&mut self, w: &mut World, me: ActorId) {
        self.start_leg(w, me, false);
    }

    /// A leg ended at `key`: chain to the next one or finish.
    fn reached(&mut self, w: &mut World, me: ActorId) {
        self.phase = Phase::Idle;
        if self.opening && self.key < self.last() || !self.opening && self.key > 0 {
            let to = if self.opening { self.key + 1 } else { self.key - 1 };
            self.phase = Phase::Leg { from: self.key, to, t: 0.0 };
            return;
        }
        w.outputs.push(Output::AmbientStop { actor: me });
        if self.opening {
            w.sound(me, &self.sounds[OPENED].clone());
            if let Some(event) = w.get(me).map(|e| e.event.clone()) {
                w.trigger_event(&event, None);
            }
            match self.mode {
                MoverMode::TriggerOpenTimed | MoverMode::BumpOpenTimed | MoverMode::StandOpenTimed => {
                    if self.once && self.mode == MoverMode::TriggerOpenTimed {
                        self.done_for_good = true;
                    } else {
                        self.phase = Phase::StayOpen(self.stay_open);
                    }
                }
                MoverMode::TriggerPound => self.close(w, me),
                MoverMode::TriggerControl if !self.held => self.close(w, me),
                _ => {}
            }
        } else {
            w.sound(me, &self.sounds[CLOSED].clone());
            if self.mode == MoverMode::TriggerPound && self.held {
                self.open(w, me);
            }
        }
    }

    /// Whether moving to the pose would push into a pawn that isn't standing on the mover.
    fn blocked(&self, w: &mut World, me: ActorId, pos: Vec3, rot: [i32; 3]) -> bool {
        let Some(e) = w.get_mut(me) else { return false };
        let (old_pos, old_rot) = (e.pos, e.rot);
        (e.pos, e.rot) = (pos, rot);
        let pawns: Vec<(Vec3, Vec3)> = w
            .actors
            .iter()
            .flatten()
            .filter(|p| p.is_pawn && p.base != Some(me))
            .map(|p| (p.pos, Vec3::new(p.radius, p.radius, p.height)))
            .collect();
        let hit = {
            use crate::collision::Collider;
            let c = w.brush_collider(me);
            pawns.iter().any(|&(p, ext)| c.overlaps(p, ext))
        };
        if let Some(e) = w.get_mut(me) {
            (e.pos, e.rot) = (old_pos, old_rot);
        }
        hit
    }
}

impl Behaviour for Mover {
    fn tick(&mut self, w: &mut World, me: ActorId, dt: f32) {
        if !self.started {
            // Start at the current key.
            self.started = true;
            let (pos, rot) = self.pose(self.key, self.key, 0.0);
            if let Some(e) = w.get_mut(me) {
                (e.pos, e.rot) = (pos, rot);
            }
        }
        match self.phase {
            Phase::Idle => {
                if self.mode == MoverMode::StandOpenTimed
                    && self.is_closed()
                    && w.actors.iter().flatten().any(|p| p.is_pawn && p.base == Some(me))
                {
                    self.open(w, me);
                }
            }
            Phase::Delay(left) => {
                self.phase = if left > dt { Phase::Delay(left - dt) } else { Phase::Idle };
                if self.phase == Phase::Idle {
                    self.start_leg(w, me, true);
                }
            }
            Phase::StayOpen(left) => {
                if left > dt {
                    self.phase = Phase::StayOpen(left - dt);
                } else {
                    self.phase = Phase::Idle;
                    self.close(w, me);
                }
            }
            Phase::Leg { from, to, t } => {
                let t2 = (t + dt / self.move_time).min(1.0);
                let (pos, rot) = self.pose(from, to, t2);
                if self.blocked(w, me, pos, rot) {
                    match self.encroach {
                        // Stop where it is.
                        0 => return,
                        // Go back the other way.
                        1 => {
                            let opening = !self.opening;
                            self.start_leg(w, me, opening);
                            return;
                        }
                        // Keep going; the pawn gets pushed out.
                        _ => {}
                    }
                }
                if let Some(e) = w.get_mut(me) {
                    e.moved = pos - e.pos;
                    (e.pos, e.rot) = (pos, rot);
                }
                self.phase = Phase::Leg { from, to, t: t2 };
                if t2 >= 1.0 {
                    self.key = to;
                    self.reached(w, me);
                }
            }
        }
    }

    fn trigger(&mut self, w: &mut World, me: ActorId, _instigator: Option<ActorId>) {
        if self.done_for_good {
            return;
        }
        self.held = true;
        match self.mode {
            MoverMode::TriggerToggle => {
                let closed_or_closing = self.key == 0 && !matches!(self.phase, Phase::Leg { .. })
                    || matches!(self.phase, Phase::Leg { .. }) && !self.opening;
                if closed_or_closing { self.open(w, me) } else { self.close(w, me) }
            }
            MoverMode::TriggerOpenTimed if self.is_closed() => self.open(w, me),
            MoverMode::TriggerControl | MoverMode::TriggerPound => self.open(w, me),
            _ => {}
        }
    }

    fn untrigger(&mut self, w: &mut World, me: ActorId, _instigator: Option<ActorId>) {
        self.held = false;
        if self.mode == MoverMode::TriggerControl {
            self.close(w, me);
        }
    }

    fn bump(&mut self, w: &mut World, me: ActorId, _other: ActorId) {
        if self.mode == MoverMode::BumpOpenTimed && self.is_closed() {
            self.open(w, me);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Trigger

pub struct Trigger {
    kind: u8,
    class: String,
    active: bool,
    once: bool,
    retrigger: f32,
    last: f32,
    switch: Option<u8>,
}

impl Trigger {
    pub fn new(p: &ActorProps) -> Self {
        Self {
            kind: p.byte("TriggerType", 0),
            class: text(p, "ClassProximityType"),
            active: p.bool_or("bInitiallyActive", true),
            once: p.bool("bTriggerOnceOnly"),
            retrigger: p.f32("ReTriggerDelay", 0.0),
            last: f32::NEG_INFINITY,
            switch: match text(p, "InitialState").to_ascii_lowercase().as_str() {
                "othertriggertoggles" => Some(0),
                "othertriggerturnson" => Some(1),
                "othertriggerturnsoff" => Some(2),
                _ => None,
            },
        }
    }

    fn cares(&self, w: &World, other: ActorId) -> bool {
        let Some(o) = w.get(other) else { return false };
        self.active
            && match self.kind {
                0 => o.is_player,
                1 => o.is_pawn,
                2 => !self.class.is_empty() && w.classes.is_a(&o.class, &self.class),
                _ => true,
            }
    }
}

impl Behaviour for Trigger {
    fn touch(&mut self, w: &mut World, me: ActorId, other: ActorId) {
        if !self.cares(w, other) {
            return;
        }
        if self.retrigger > 0.0 {
            if w.time - self.last < self.retrigger {
                return;
            }
            self.last = w.time;
        }
        if let Some(event) = w.get(me).map(|e| e.event.clone()) {
            w.trigger_event(&event, Some(other));
        }
        if self.once {
            w.set_collide_actors(me, false);
        }
    }

    fn untouch(&mut self, w: &mut World, me: ActorId, other: ActorId) {
        if self.cares(w, other)
            && let Some(event) = w.get(me).map(|e| e.event.clone())
        {
            w.untrigger_event(&event, Some(other));
        }
    }

    fn trigger(&mut self, _w: &mut World, _me: ActorId, _instigator: Option<ActorId>) {
        match self.switch {
            Some(0) => self.active = !self.active,
            Some(1) => self.active = true,
            Some(2) => self.active = false,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Dispatcher

pub struct Dispatcher {
    events: Vec<(String, f32)>,
    /// Next event to send and time left before it.
    running: Option<(usize, f32)>,
    instigator: Option<ActorId>,
}

impl Dispatcher {
    pub fn new(p: &ActorProps) -> Self {
        let events = (0..8)
            .map(|i| (text(p, &indexed("OutEvents", i)), p.f32(&indexed("OutDelays", i), 0.0)))
            .collect();
        Self { events, running: None, instigator: None }
    }
}

impl Behaviour for Dispatcher {
    fn trigger(&mut self, _w: &mut World, _me: ActorId, instigator: Option<ActorId>) {
        if self.running.is_none() {
            self.running = Some((0, self.events[0].1));
            self.instigator = instigator;
        }
    }

    fn tick(&mut self, w: &mut World, _me: ActorId, dt: f32) {
        let mut budget = dt;
        while let Some((i, left)) = self.running {
            if left > budget {
                self.running = Some((i, left - budget));
                return;
            }
            budget -= left;
            let event = self.events[i].0.clone();
            if !event.is_empty() {
                w.trigger_event(&event, self.instigator);
            }
            self.running = self.events.get(i + 1).map(|e| (i + 1, e.1));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Counter

pub struct Counter {
    left: u32,
}

impl Behaviour for Counter {
    fn trigger(&mut self, w: &mut World, me: ActorId, instigator: Option<ActorId>) {
        if self.left == 0 {
            return;
        }
        self.left -= 1;
        if self.left == 0
            && let Some(event) = w.get(me).map(|e| e.event.clone())
        {
            w.trigger_event(&event, instigator);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// MusicEvent

pub struct MusicEvent {
    song: Option<String>,
    once: bool,
    used: bool,
}

impl MusicEvent {
    pub fn new(p: &ActorProps) -> Self {
        let song = text(p, "Song");
        Self {
            song: (!p.bool("bSilence") && !song.is_empty()).then_some(song),
            once: p.bool("bOnceOnly"),
            used: false,
        }
    }
}

impl Behaviour for MusicEvent {
    fn trigger(&mut self, w: &mut World, _me: ActorId, _instigator: Option<ActorId>) {
        if self.once && self.used {
            return;
        }
        self.used = true;
        w.outputs.push(Output::Music { song: self.song.clone() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Entity;
    use hp1_data::level::{Actor, LevelFile};

    /// Name, class and own settings of one placed object.
    type Placed<'a> = (&'a str, &'a str, Vec<(&'a str, DataValue)>);

    fn world_with(actors: Vec<Placed>) -> World {
        let level = LevelFile {
            name: "Test".into(),
            actors: actors
                .into_iter()
                .map(|(name, class, props)| {
                    Some(Actor {
                        name: name.into(),
                        class: class.into(),
                        props: props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
                        brush: None,
                    })
                })
                .collect(),
            model: Default::default(),
            brushes: Default::default(),
            reach_specs: Vec::new(),
        };
        let classes: ClassTable = serde_json::from_str(
            r#"{"classes": {
                "Engine.Actor": {"parent": "Core.Object", "defaults": {}},
                "Engine.Triggers": {"parent": "Engine.Actor", "defaults": {"bCollideActors": true}},
                "Engine.Trigger": {"parent": "Engine.Triggers", "defaults": {}},
                "Engine.Brush": {"parent": "Engine.Actor", "defaults": {}},
                "Engine.Mover": {"parent": "Engine.Brush", "defaults": {}},
                "Engine.Dispatcher": {"parent": "Engine.Triggers", "defaults": {}},
                "Engine.Counter": {"parent": "Engine.Triggers", "defaults": {}},
                "Engine.MusicEvent": {"parent": "Engine.Triggers", "defaults": {}}
            }}"#,
        )
        .unwrap();
        World::new(level, classes)
    }

    fn s(v: &str) -> DataValue {
        DataValue::Str(v.into())
    }

    fn player(w: &mut World, at: Vec3) -> ActorId {
        w.spawn(Entity::pawn("Harry", "Test.Harry", at, 15.0, 42.0, true))
    }

    #[test]
    fn walking_into_a_trigger_plays_music_through_a_dispatcher() {
        let mut w = world_with(vec![
            ("Trigger0", "Engine.Trigger", vec![("Event", s("go"))]),
            (
                "Dispatcher0",
                "Engine.Dispatcher",
                vec![("Tag", s("go")), ("OutEvents", s("song")), ("OutDelays[1]", DataValue::Float(0.5))],
            ),
            ("MusicEvent0", "Engine.MusicEvent", vec![("Tag", s("song")), ("Song", s("Tune"))]),
        ]);
        let harry = player(&mut w, Vec3::new(500.0, 0.0, 0.0));
        w.tick(0.1);
        assert!(w.outputs.is_empty());
        w.get_mut(harry).unwrap().pos = Vec3::new(10.0, 0.0, 0.0);
        w.tick(0.1);
        w.tick(0.1);
        assert_eq!(w.outputs, vec![Output::Music { song: Some("Tune".into()) }]);
    }

    /// A 64-unit cube door at the origin that rises 100 units in one second when triggered.
    fn door_world(state: &str) -> (World, ActorId) {
        let mut w = world_with(vec![(
            "Mover0",
            "Engine.Mover",
            vec![
                ("Tag", s("door")),
                ("InitialState", s(state)),
                ("KeyPos[1]", DataValue::Vector { x: 0.0, y: 0.0, z: 100.0 }),
                ("MoveTime", DataValue::Float(1.0)),
                ("StayOpenTime", DataValue::Float(1.0)),
                ("MoverGlideType", DataValue::Int(0)),
            ],
        )]);
        w.level.brushes.insert(
            "Box".into(),
            hp1_data::level::Brush { model: crate::collision::tests::cube(32.0), polys: Vec::new() },
        );
        let e = w.get_mut(0).unwrap();
        e.brush = Some(("Box".into(), Vec3::ZERO));
        e.collide_actors = true;
        (w, 0)
    }

    #[test]
    fn toggle_door_opens_and_closes() {
        use crate::collision::Collider;
        let (mut w, door) = door_world("TriggerToggle");
        let probe = |w: &World| {
            w.collider(None).trace(Vec3::new(-100.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO)
        };
        w.tick(0.1);
        assert!(probe(&w).is_some_and(|h| h.actor == Some(door)), "closed door blocks");
        w.trigger_event("door", None);
        for _ in 0..12 {
            w.tick(0.1);
        }
        assert!((w.get(door).unwrap().pos.z - 100.0).abs() < 0.01);
        assert!(probe(&w).is_none(), "open door lets through");
        w.trigger_event("door", None);
        for _ in 0..12 {
            w.tick(0.1);
        }
        assert!(w.get(door).unwrap().pos.z.abs() < 0.01);
    }

    #[test]
    fn timed_door_closes_by_itself() {
        let (mut w, door) = door_world("TriggerOpenTimed");
        w.trigger_event("door", None);
        let mut top = 0.0f32;
        for _ in 0..40 {
            w.tick(0.1);
            top = top.max(w.get(door).unwrap().pos.z);
        }
        assert!(top > 99.0 && w.get(door).unwrap().pos.z.abs() < 0.01);
    }

    #[test]
    fn lift_carries_a_walker() {
        use crate::walk::{Body, Mode, Movement};
        let (mut w, door) = door_world("TriggerToggle");
        let harry = player(&mut w, Vec3::new(0.0, 0.0, 80.0));
        let mut body = Body::new(Vec3::new(0.0, 0.0, 80.0), 15.0, 42.0);
        let m = Movement::default();
        for i in 0..40 {
            if i == 10 {
                w.trigger_event("door", None);
            }
            w.tick(0.05);
            if let Some(b) = body.base {
                body.pos += w.get(b).unwrap().moved;
            }
            body.tick(&w.collider(Some(harry)), &m, Vec3::ZERO, false, 0.05);
            let e = w.get_mut(harry).unwrap();
            (e.pos, e.base) = (body.pos, body.base);
        }
        assert_eq!(body.mode, Mode::Walking);
        assert_eq!(body.base, Some(door));
        assert!((body.pos.z - (100.0 + 32.0 + 42.0)).abs() < 1.0, "{:?}", body.pos);
    }

    #[test]
    fn counter_fires_on_the_last_count() {
        let mut w = world_with(vec![
            (
                "Counter0",
                "Engine.Counter",
                vec![("Tag", s("c")), ("Event", s("song")), ("NumToCount", DataValue::Int(3))],
            ),
            ("MusicEvent0", "Engine.MusicEvent", vec![("Tag", s("song")), ("Song", s("Tune"))]),
        ]);
        w.trigger_event("c", None);
        w.trigger_event("c", None);
        assert!(w.outputs.is_empty());
        w.trigger_event("c", None);
        assert_eq!(w.outputs.len(), 1);
    }
}
