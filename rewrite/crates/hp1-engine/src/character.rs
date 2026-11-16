//! HP1 characters (students, teachers, ghosts) that walk their patrol route, pause at points and
//! stand idle at the end (`specs/characters/Patrol.md`).

use glam::{Vec2, Vec3, Vec3Swizzles};
use hp1_data::DataValue;

use crate::patrol::PatrolNet;
use crate::props::ActorProps;
use crate::units::ROT_TO_RAD;
use crate::walk::{Body, Movement};
use crate::world::{ActorId, Animation, Behaviour, World};

/// The longest step one tick may take, so a slow frame doesn't throw a character off its route.
const MAX_DT: f32 = 0.1;
/// Arrived when this close to a point (sideways).
const ARRIVED: f32 = 1.0;
/// How often an idle character with a looping route checks whether to start over.
const IDLE_CHECK: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Start,
    Walk {
        to: usize,
        /// Along the curve (0..1) when `curve`.
        t: f32,
        curve: bool,
        /// Straight walks: the direction the leg started in.
        dir: Vec2,
        /// Seconds on this leg, to give up if something is in the way for good.
        time: f32,
    },
    Pause {
        left: f32,
        next: Option<usize>,
    },
    Idle {
        left: f32,
    },
}

pub struct Character {
    movement: Movement,
    walk_anim: String,
    idle_anim: String,
    splines: bool,
    loop_path: bool,
    first_tag: String,
    first_name: String,
    /// Turning speed, rotation units per second.
    turn_rate: f32,
    body: Option<Body>,
    net: PatrolNet,
    first: Option<usize>,
    /// The point it was at last.
    last: Option<usize>,
    state: State,
    yaw: f32,
    want_yaw: f32,
    anim: Animation,
}

fn or(name: String, fallback: &str) -> String {
    if name.is_empty() { fallback.to_owned() } else { name }
}

/// Shortest signed turn from `a` to `b`, in rotation units.
fn turn(a: f32, b: f32) -> f32 {
    (b - a + 32768.0).rem_euclid(65536.0) - 32768.0
}

impl Character {
    pub fn new(p: &ActorProps) -> Self {
        let movement = Movement {
            ground_speed: p.f32("GroundSpeed", 200.0),
            step_height: p.f32("MaxStepHeight", 25.0),
            ..Movement::default()
        };
        let idle_anim = or(p.text("idleAnimName"), "Breath");
        let turn_rate = p.rotator("RotationRate")[1].abs() as f32;
        Self {
            movement,
            walk_anim: or(p.text("walkAnimName"), "Walk"),
            idle_anim: idle_anim.clone(),
            splines: p.bool("bUseFraySplines"),
            loop_path: p.bool("bLoopPath"),
            first_tag: p.text("firstPatrolPointTag"),
            first_name: p.text("firstPatrolPointObjectName"),
            turn_rate: if turn_rate > 0.0 { turn_rate } else { 20000.0 },
            body: None,
            net: PatrolNet::default(),
            first: None,
            last: None,
            state: State::Start,
            yaw: p.rotator("Rotation")[1] as f32,
            want_yaw: p.rotator("Rotation")[1] as f32,
            anim: Animation { name: idle_anim, time: 0.0 },
        }
    }

    fn play(&mut self, name: &str) {
        if !self.anim.name.eq_ignore_ascii_case(name) {
            self.anim = Animation { name: name.to_owned(), time: 0.0 };
        }
    }

    fn start(&mut self, w: &mut World, me: ActorId) {
        self.net = PatrolNet::build(w);
        self.first = self.net.find(&self.first_tag, &self.first_name);
        let Some(e) = w.get_mut(me) else { return };
        e.is_pawn = true;
        e.collide_actors = true;
        let mut body = Body::new(e.pos, e.radius, e.height);
        body.settle(&w.collider(Some(me)));
        self.body = Some(body);
        self.go(self.first);
    }

    /// Sets off for point `to`, or stands idle if there is none.
    fn go(&mut self, to: Option<usize>) {
        let Some(to) = to else {
            self.state = State::Idle { left: IDLE_CHECK };
            let idle = self.idle_anim.clone();
            self.play(&idle);
            return;
        };
        let pos = self.body.map_or(Vec3::ZERO, |b| b.pos);
        let point = &self.net.points[to];
        // The curve only exists along the chain, coming from the point before.
        let curve =
            self.splines && self.last.is_some() && point.prev == self.last && self.net.in_len(to) > 1.0;
        let dir = (point.pos - pos).xy().normalize_or_zero();
        self.state = State::Walk { to, t: 0.0, curve, dir, time: 0.0 };
        let walk = self.walk_anim.clone();
        self.play(&walk);
    }

    /// Reached point `at`.
    fn arrive(&mut self, w: &mut World, me: ActorId, at: usize) {
        let p = self.net.points[at].clone();
        if !p.event.is_empty() {
            w.trigger_event(&p.event, Some(me));
        }
        if !p.sound.is_empty() {
            w.sound(me, &p.sound);
        }
        if p.destroy {
            w.remove(me);
            return;
        }
        let next = p.next.or_else(|| self.net.nearest(p.pos, [Some(at), self.last]));
        self.last = Some(at);
        if p.pause > 0.0 {
            self.state = State::Pause { left: p.pause, next };
            let anim = if p.pause_anim.is_empty() { self.idle_anim.clone() } else { p.pause_anim };
            self.play(&anim);
            if p.look {
                self.want_yaw = p.rot[1] as f32;
            }
        } else {
            self.go(next);
        }
    }

    fn walk(&mut self, w: &mut World, me: ActorId, dt: f32) {
        let State::Walk { to, t, curve, dir, time } = self.state else { return };
        let Some(mut body) = self.body else { return };
        let speed = self.movement.ground_speed;
        let target = self.net.points[to].pos;
        let near = (target - body.pos).xy();
        let (vel, t, done) = if curve {
            let t = (t + speed * dt / self.net.in_len(to)).min(1.0);
            let want = self.net.curve(to, t).unwrap_or(target);
            let mut step = (want - body.pos).xy();
            let most = 1.5 * speed * dt;
            if step.length() > most {
                step *= most / step.length();
            }
            (step / dt, t, t >= 1.0 && near.length() < ARRIVED)
        } else {
            // Straight at it, without overshooting; passing it counts as arriving.
            let step = if near.length() < speed * dt { near / dt } else { dir * speed };
            (step, 0.0, near.length() < ARRIVED || near.dot(dir) <= 0.0)
        };
        if done {
            self.arrive(w, me, to);
            return;
        }
        // Something is in the way for good: count the point as reached.
        let expected = self.net.points[to].pos.distance(body.pos) / speed.max(1.0);
        if time > 2.0 * expected + 3.0 && time > 5.0 {
            self.arrive(w, me, to);
            return;
        }
        let before = body.pos;
        body.drive(&w.collider(Some(me)), &self.movement, vel, dt);
        let moved = (body.pos - before).xy();
        if moved.length() > 0.01 {
            self.want_yaw = moved.y.atan2(moved.x) / ROT_TO_RAD;
        }
        self.body = Some(body);
        self.state = State::Walk { to, t, curve, dir, time: time + dt };
    }

    /// Writes position, facing and animation back to the object.
    fn sync(&self, w: &mut World, me: ActorId) {
        let Some(e) = w.get_mut(me) else { return };
        if let Some(b) = self.body {
            e.pos = b.pos;
            e.base = b.base;
        }
        e.rot = [0, self.yaw.round() as i32, 0];
        e.anim = Some(self.anim.clone());
    }
}

impl Behaviour for Character {
    fn tick(&mut self, w: &mut World, me: ActorId, dt: f32) {
        let dt = dt.min(MAX_DT);
        if self.state == State::Start {
            self.start(w, me);
        }
        // Ride whatever it stands on.
        if let Some(b) = &mut self.body
            && let Some(base) = b.base.and_then(|id| w.get(id))
        {
            b.pos += base.moved;
        }
        match self.state {
            State::Start => {}
            State::Walk { .. } => self.walk(w, me, dt),
            State::Pause { left, next } => {
                if left > dt {
                    self.state = State::Pause { left: left - dt, next };
                } else {
                    self.go(next);
                }
            }
            State::Idle { left } => {
                if left > dt {
                    self.state = State::Idle { left: left - dt };
                } else if self.loop_path && self.first.is_some() {
                    self.last = None;
                    self.go(self.first);
                } else {
                    self.state = State::Idle { left: IDLE_CHECK };
                }
            }
        }
        let d = turn(self.yaw, self.want_yaw);
        let most = self.turn_rate * dt;
        self.yaw = (self.yaw + d.clamp(-most, most)).rem_euclid(65536.0);
        self.anim.time += dt;
        self.sync(w, me);
    }
}

// ---------------------------------------------------------------------------------------------
// Spawning characters onto a patrol route

/// HP1's `TriggerSpwnBsChrOnPPnt`: when triggered or touched, puts one character, picked at
/// random from its list, on a patrol point and sends it along the route from there.
/// Touches count only from what its TriggerType allows (the player by default).
pub struct SpawnOnPatrol {
    /// Class and walking speed (0 = the class's own).
    kinds: Vec<(String, f32)>,
    start_tag: String,
    start_name: String,
    count: usize,
    /// Who may set it off by touching.
    touch: crate::behaviours::Trigger,
}

impl SpawnOnPatrol {
    pub fn new(p: &ActorProps) -> Self {
        let key = |k: &str, i: usize| if i == 0 { k.to_owned() } else { format!("{k}[{i}]") };
        let kinds = (0..8)
            .map(|i| (p.text(&key("BaseCharToSpawn", i)), p.f32(&key("BaseCharGroundSpeed", i), 0.0)))
            .take_while(|(c, _)| !c.is_empty())
            .collect();
        Self {
            kinds,
            start_tag: p.text("StartPatrolPoint_Tag"),
            start_name: p.text("StartPatrolPoint_ObjectName"),
            count: 0,
            touch: crate::behaviours::Trigger::new(p),
        }
    }

    fn spawn(&mut self, w: &mut World, me: ActorId) {
        if self.kinds.is_empty() {
            return;
        }
        let net = PatrolNet::build(w);
        let Some(start) = net.find(&self.start_tag, &self.start_name) else { return };
        let point = &net.points[start];
        let toward = point.next.map_or(Vec3::X, |n| net.points[n].pos - point.pos);
        let yaw = (toward.y.atan2(toward.x) / ROT_TO_RAD).round() as i32;
        let (class, speed) = self.kinds[w.rand(self.kinds.len())].clone();
        let short = class.rsplit('.').next().unwrap_or(&class);
        let me_name = w.get(me).map_or("", |e| e.name.as_str());
        let mut props = hp1_data::PropMap::new();
        let [x, y, z] = point.pos.to_array();
        props.insert("Location".into(), DataValue::Vector { x, y, z });
        props.insert("Rotation".into(), DataValue::Rotator { pitch: 0, yaw, roll: 0 });
        props.insert("bFollowPatrolPoints".into(), DataValue::Bool(true));
        props.insert("firstPatrolPointObjectName".into(), DataValue::Str(point.name.clone()));
        if speed != 0.0 {
            props.insert("GroundSpeed".into(), DataValue::Float(speed as f64));
        }
        let name = format!("{short}_{me_name}_{}", self.count);
        self.count += 1;
        w.spawn_actor(hp1_data::level::Actor { name, class, props, brush: None });
    }
}

impl Behaviour for SpawnOnPatrol {
    fn trigger(&mut self, w: &mut World, me: ActorId, _instigator: Option<ActorId>) {
        self.spawn(w, me);
    }

    fn touch(&mut self, w: &mut World, me: ActorId, other: ActorId) {
        // Only who the trigger is set up for, or the characters it spawns would set it off again.
        if self.touch.cares(w, other) {
            self.spawn(w, me);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behaviours::tests::{s, world_with};

    fn v(x: f32, y: f32, z: f32) -> DataValue {
        DataValue::Vector { x, y, z }
    }

    fn point<'a>(name: &'a str, at: DataValue, next: &str) -> (&'a str, &'a str, Vec<(&'a str, DataValue)>) {
        let mut props = vec![("Location", at)];
        if !next.is_empty() {
            props.push(("NextPatrol_ObjectName", s(next)));
        }
        (name, "Engine.PatrolPoint", props)
    }

    fn walker(extra: Vec<(&str, DataValue)>) -> (&str, &str, Vec<(&str, DataValue)>) {
        let mut props = vec![
            ("Location", v(0.0, 0.0, 43.0)),
            ("CollisionRadius", DataValue::Float(15.0)),
            ("CollisionHeight", DataValue::Float(42.0)),
            ("bFollowPatrolPoints", DataValue::Bool(true)),
            ("firstPatrolPointObjectName", s("P0")),
            ("GroundSpeed", DataValue::Float(100.0)),
        ];
        props.extend(extra);
        ("Kid", "HPBase.baseChar", props)
    }

    /// Runs the world and returns the character's positions.
    fn run(w: &mut World, kid: ActorId, seconds: f32) -> Vec<Vec3> {
        let mut out = Vec::new();
        for _ in 0..(seconds * 30.0) as usize {
            w.tick(1.0 / 30.0);
            if let Some(e) = w.get(kid) {
                out.push(e.pos);
            }
        }
        out
    }

    fn closest(path: &[Vec3], to: Vec3) -> f32 {
        path.iter().map(|p| (*p - to).xy().length()).fold(f32::MAX, f32::min)
    }

    #[test]
    fn walks_the_chain_and_stands_idle_at_the_end() {
        let mut w = world_with(vec![
            point("P0", v(200.0, 0.0, 50.0), "P1"),
            point("P1", v(200.0, 200.0, 50.0), ""),
            walker(vec![]),
        ]);
        w.level.model = crate::collision::tests::floor();
        let path = run(&mut w, 2, 8.0);
        assert!(closest(&path, Vec3::new(200.0, 0.0, 0.0)) < ARRIVED + 0.5);
        let e = w.get(2).unwrap();
        assert!((e.pos.xy() - glam::Vec2::new(200.0, 200.0)).length() < ARRIVED + 0.5, "{:?}", e.pos);
        assert!((e.pos.z - 42.0).abs() < 1.0, "stays on the floor: {:?}", e.pos);
        assert_eq!(e.anim.as_ref().unwrap().name, "Breath");
    }

    #[test]
    fn curves_pass_through_every_point() {
        let mut w = world_with(vec![
            point("P0", v(200.0, 0.0, 50.0), "P1"),
            point("P1", v(400.0, 200.0, 50.0), "P2"),
            point("P2", v(600.0, 0.0, 50.0), ""),
            walker(vec![("bUseFraySplines", DataValue::Bool(true))]),
        ]);
        w.level.model = crate::collision::tests::floor();
        let path = run(&mut w, 3, 15.0);
        for p in [Vec3::new(200.0, 0.0, 0.0), Vec3::new(400.0, 200.0, 0.0), Vec3::new(600.0, 0.0, 0.0)] {
            assert!(closest(&path, p) < ARRIVED + 0.5, "missed {p}");
        }
        // Curving: halfway along the P0 → P1 leg it is off the straight line.
        let off = path
            .iter()
            .filter(|p| (p.x - 300.0).abs() < 3.0)
            .map(|p| (p.y - 100.0).abs())
            .fold(0.0, f32::max);
        assert!(off > 1.0, "{off}");
    }

    #[test]
    fn pauses_sends_its_event_and_can_be_removed() {
        let mut w = world_with(vec![
            (
                "P0",
                "Engine.PatrolPoint",
                vec![
                    ("Location", v(100.0, 0.0, 50.0)),
                    ("NextPatrol_ObjectName", s("P1")),
                    ("pausetime", DataValue::Float(2.0)),
                    ("EventToSend", s("song")),
                ],
            ),
            (
                "P1",
                "Engine.PatrolPoint",
                vec![("Location", v(200.0, 0.0, 50.0)), ("bDestroyPawn", DataValue::Bool(true))],
            ),
            ("MusicEvent0", "Engine.MusicEvent", vec![("Tag", s("song")), ("Song", s("Tune"))]),
            walker(vec![]),
        ]);
        w.level.model = crate::collision::tests::floor();
        run(&mut w, 3, 1.5);
        assert_eq!(w.outputs.len(), 1, "event sent on arrival");
        run(&mut w, 3, 1.0);
        let x = w.get(3).unwrap().pos.x;
        assert!((x - 100.0).abs() < 2.0, "still pausing: {x}");
        run(&mut w, 3, 3.0);
        assert!(w.get(3).is_none(), "removed at the last point");
    }

    #[test]
    fn spawn_trigger_sends_a_character_down_the_route() {
        let mut w = world_with(vec![
            point("P0", v(0.0, 0.0, 50.0), "P1"),
            point("P1", v(0.0, 300.0, 50.0), ""),
            (
                "Spawner",
                "HPBase.TriggerSpwnBsChrOnPPnt",
                vec![
                    ("Tag", s("kids")),
                    ("bCollideActors", DataValue::Bool(false)),
                    ("BaseCharToSpawn", s("HPBase.baseChar")),
                    ("BaseCharGroundSpeed", DataValue::Float(150.0)),
                    ("StartPatrolPoint_ObjectName", s("P0")),
                ],
            ),
        ]);
        w.level.model = crate::collision::tests::floor();
        w.trigger_event("kids", None);
        assert_eq!(w.spawned.len(), 1);
        let kid = w.spawned[0];
        assert_eq!(w.get(kid).unwrap().rot[1], 16384, "faces the next point");
        let path = run(&mut w, kid, 3.0);
        assert!(closest(&path, Vec3::new(0.0, 300.0, 0.0)) < ARRIVED + 0.5);
        assert_eq!(w.get(kid).unwrap().anim.as_ref().unwrap().name, "Breath");
    }

    #[test]
    fn turning_takes_the_short_way() {
        assert_eq!(turn(65000.0, 500.0), 1036.0);
        assert_eq!(turn(500.0, 65000.0), -1036.0);
    }
}
