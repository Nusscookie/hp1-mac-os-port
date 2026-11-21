//! HP1 characters (students, teachers, ghosts) that walk their patrol route, pause at points and
//! stand idle at the end (`specs/characters/Patrol.md`).
//!
//! How the walking ends and how a point counts as reached follow the original engine's own
//! moving rules, checked against SurrealEngine's re-implementation of them.

use glam::{Vec2, Vec3, Vec3Swizzles};
use hp1_data::DataValue;

use crate::patrol::PatrolNet;
use crate::props::ActorProps;
use crate::units::ROT_TO_RAD;
use crate::walk::{Body, Movement};
use crate::world::{ActorId, Animation, Behaviour, World};

/// The longest step one tick may take (the original caps its own walking the same way).
const MAX_DT: f32 = 0.1;
/// How often an idle character checks whether to start its route over.
const IDLE_CHECK: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Walk {
    /// The engine's ordinary walk to a spot: speed up towards it, done when close or when the
    /// walk has taken too long, repeated until within the character's reach of the point.
    Plain,
    /// HP1's straight walk: a fixed step towards the point each tick, done once past it.
    Straight,
    /// HP1's curve through the points.
    Curve,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Start,
    Walk {
        to: usize,
        how: Walk,
        /// Along the curve (0..1).
        t: f32,
        /// Plain walks: seconds left before the walk gives up and is tried again.
        timer: f32,
    },
    Pause {
        /// The point it pauses at.
        at: usize,
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
    straight: bool,
    loop_path: bool,
    first_tag: String,
    first_name: String,
    /// A point counts as reached this close (`fNavPointColRadius`).
    reach: f32,
    /// Turning speed, rotation units per second.
    turn_rate: f32,
    body: Option<Body>,
    net: PatrolNet,
    first: Option<usize>,
    /// The point it was at last.
    last: Option<usize>,
    /// Only patrol points with this link tag are picked as nearest (from the first point).
    link_tag: String,
    state: State,
    yaw: f32,
    want_yaw: f32,
    anim: Animation,
    /// Position and facing last written to the object, to notice when something else (a
    /// cutscene) moved or turned it.
    synced: Option<(Vec3, i32)>,
}

/// Shortest signed turn from `a` to `b`, in rotation units.
fn turn(a: f32, b: f32) -> f32 {
    (b - a + 32768.0).rem_euclid(65536.0) - 32768.0
}

fn yaw_of(v: Vec2) -> f32 {
    v.y.atan2(v.x) / ROT_TO_RAD
}

impl Character {
    pub fn new(p: &ActorProps) -> Self {
        let movement = Movement {
            ground_speed: p.f32("GroundSpeed", 200.0),
            accel_rate: p.f32("AccelRate", 1024.0),
            step_height: p.f32("MaxStepHeight", 25.0),
            ..Movement::default()
        };
        let turn_rate = p.rotator("RotationRate")[1].abs() as f32;
        let yaw = p.rotator("Rotation")[1] as f32;
        Self {
            movement,
            walk_anim: p.text("walkAnimName"),
            idle_anim: p.text("idleAnimName"),
            splines: p.bool("bUseFraySplines"),
            straight: p.bool("bUseFrayMoveTo"),
            loop_path: p.bool("bLoopPath"),
            first_tag: p.text("firstPatrolPointTag"),
            first_name: p.text("firstPatrolPointObjectName"),
            reach: p.f32("fNavPointColRadius", 50.0),
            turn_rate: if turn_rate > 0.0 { turn_rate } else { 20000.0 },
            body: None,
            net: PatrolNet::default(),
            first: None,
            last: None,
            link_tag: String::new(),
            state: State::Start,
            yaw,
            want_yaw: yaw,
            anim: Animation::looping(&p.text("AnimSequence")),
            synced: None,
        }
    }

    /// Loops an animation. Like the original, a missing name keeps the current one.
    fn play(&mut self, name: &str) {
        if !name.is_empty() && !self.anim.name.eq_ignore_ascii_case(name) {
            self.anim = Animation::looping(name);
        }
    }

    fn start(&mut self, w: &mut World, me: ActorId) {
        self.net = PatrolNet::build(w);
        self.first = self.net.find(&self.first_tag, &self.first_name);
        if let Some(f) = self.first {
            self.link_tag = self.net.points[f].link_tag.clone();
        }
        self.last = self.first;
        let Some(e) = w.get_mut(me) else { return };
        e.is_pawn = true;
        e.collide_actors = true;
        let mut body = Body::new(e.pos, e.radius, e.height);
        body.settle(&w.collider(Some(me)));
        self.body = Some(body);
        let walk = self.walk_anim.clone();
        self.play(&walk);
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
        let point = &self.net.points[to];
        // The original also curves into points that have no point before them, reading a
        // missing point as the origin; those are walked to without the curve instead.
        let how = if self.splines && point.has_curve && point.prev.is_some() {
            Walk::Curve
        } else if self.straight {
            Walk::Straight
        } else {
            Walk::Plain
        };
        self.state = State::Walk { to, how, t: 0.0, timer: self.walk_time(to) };
    }

    /// How long a plain walk to point `to` may take: 1 s plus 1.3 times the time at full speed.
    fn walk_time(&self, to: usize) -> f32 {
        let pos = self.body.map_or(Vec3::ZERO, |b| b.pos);
        let speed = self.movement.ground_speed;
        if speed > 0.0 { 1.0 + 1.3 * pos.distance(self.net.points[to].pos) / speed } else { 0.5 }
    }

    /// Reached point `at`.
    fn arrive(&mut self, w: &mut World, me: ActorId, at: usize) {
        let p = self.net.points[at].clone();
        if !p.event.is_empty() {
            w.trigger_event(&p.event, Some(me));
        }
        let pos = self.body.map_or(p.pos, |b| b.pos);
        let next = p.next.or_else(|| self.net.nearest(pos, [self.last, Some(at)], &self.link_tag));
        self.last = Some(at);
        if p.destroy {
            w.remove(me);
            return;
        }
        if p.pause > 0.0 {
            self.pause(at, next);
        } else {
            self.go(next);
        }
    }

    fn pause(&mut self, at: usize, next: Option<usize>) {
        let p = &self.net.points[at];
        let (left, anim, look, yaw) = (p.pause, p.pause_anim.clone(), p.look, p.rot[1] as f32);
        self.state = State::Pause { at, left, next };
        self.play(&anim);
        if look {
            self.want_yaw = yaw;
        }
    }

    /// Back from a cutscene: what it was doing starts over from its beginning, from where the
    /// cutscene left it.
    fn restart(&mut self, w: &World, me: ActorId) {
        self.adopt(w, me);
        match self.state {
            State::Start => {}
            State::Walk { to, .. } => {
                let walk = self.walk_anim.clone();
                self.play(&walk);
                self.go(Some(to));
            }
            State::Pause { at, next, .. } => self.pause(at, next),
            State::Idle { .. } => self.go(None),
        }
    }

    /// Takes the object's position, facing and animation as its own.
    fn adopt(&mut self, w: &World, me: ActorId) {
        let Some(e) = w.get(me) else { return };
        if let Some(b) = &mut self.body {
            b.pos = e.pos;
            b.vel = Vec3::ZERO;
        }
        self.yaw = e.rot[1] as f32;
        self.want_yaw = self.yaw;
        if let Some(a) = &e.anim {
            self.anim = a.clone();
        }
        self.synced = Some((e.pos, e.rot[1]));
    }

    fn walk(&mut self, w: &mut World, me: ActorId, dt: f32) {
        let State::Walk { to, how, t, timer } = self.state else { return };
        let Some(mut body) = self.body else { return };
        let speed = self.movement.ground_speed;
        let target = self.net.points[to].pos;
        let before = body.pos;
        let collider = w.collider(Some(me));
        let mut next = State::Walk { to, how, t, timer: timer - dt };
        let mut arrived = false;
        match how {
            Walk::Plain => {
                let d = (target - body.pos).xy();
                let over = timer <= 0.0
                    || d.length_squared() < 1.0
                    || d.length_squared() < body.vel.length_squared() * 0.05;
                if over {
                    if target.distance(body.pos) < self.reach {
                        arrived = true;
                    } else {
                        // Not close enough: walk there again.
                        next = State::Walk { to, how, t, timer: self.walk_time(to) };
                    }
                    body.tick(&collider, &self.movement, Vec3::ZERO, false, dt);
                } else {
                    body.tick(&collider, &self.movement, d.normalize().extend(0.0), false, dt);
                    self.want_yaw = yaw_of(d);
                }
            }
            Walk::Straight => {
                let d = (target - body.pos).xy();
                body.drive(&collider, &self.movement, d.normalize_or_zero() * speed, dt);
                // Done once the last step went past the point.
                let crossed = (target - before).xy().dot((target - body.pos).xy()) <= 0.0;
                arrived = crossed && target.distance(body.pos) < self.reach;
            }
            Walk::Curve => {
                let t = (t + speed * dt / self.net.in_len(to).max(1.0)).min(1.0);
                let want = self.net.curve(to, t).unwrap_or(target);
                let mut step = (want - body.pos).xy();
                let most = 1.5 * speed * dt;
                if step.length() > most {
                    step *= most / step.length();
                }
                body.drive(&collider, &self.movement, step / dt, dt);
                arrived = (body.pos - target).xy().length() < 1.0;
                next = State::Walk { to, how, t, timer };
            }
        }
        if how != Walk::Plain {
            let moved = (body.pos - before).xy();
            if moved.length() > 0.01 {
                self.want_yaw = yaw_of(moved);
            }
        }
        self.body = Some(body);
        self.state = next;
        if arrived {
            self.arrive(w, me, to);
        }
    }

    /// Standing still: friction and gravity go on.
    fn stand(&mut self, w: &World, me: ActorId, dt: f32) {
        if let Some(b) = &mut self.body {
            b.tick(&w.collider(Some(me)), &self.movement, Vec3::ZERO, false, dt);
        }
    }

    /// Writes position, facing and animation back to the object.
    fn sync(&mut self, w: &mut World, me: ActorId) {
        let Some(e) = w.get_mut(me) else { return };
        if let Some(b) = self.body {
            e.pos = b.pos;
            e.base = b.base;
        }
        e.rot = [0, self.yaw.round() as i32, 0];
        e.anim = Some(self.anim.clone());
        self.synced = Some((e.pos, e.rot[1]));
    }
}

impl Behaviour for Character {
    fn tick(&mut self, w: &mut World, me: ActorId, dt: f32) {
        let dt = dt.min(MAX_DT);
        if self.state == State::Start {
            self.start(w, me);
        }
        // A cutscene has it.
        if w.puppet_busy(me) {
            return;
        }
        if w.take_restart(me) {
            self.restart(w, me);
        } else if let Some(e) = w.get(me)
            && self.synced.is_some_and(|s| s != (e.pos, e.rot[1]))
        {
            // Teleported or turned by a cutscene.
            let anim = self.anim.clone();
            self.adopt(w, me);
            self.anim = anim;
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
            State::Pause { at, left, next } => {
                self.stand(w, me, dt);
                if left > dt {
                    self.state = State::Pause { at, left: left - dt, next };
                } else {
                    // Back to patrolling, which starts with the walk animation.
                    let walk = self.walk_anim.clone();
                    self.play(&walk);
                    self.go(next);
                }
            }
            State::Idle { left } => {
                self.stand(w, me, dt);
                if left > dt {
                    self.state = State::Idle { left: left - dt };
                } else if self.loop_path && self.first.is_some() {
                    // Starts over without switching back to the walk animation, as the
                    // original does.
                    self.go(self.first);
                } else {
                    self.state = State::Idle { left: IDLE_CHECK };
                }
            }
        }
        if w.get(me).is_none() {
            return;
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
/// random from its list, on a patrol point and sends it along the route from there. The spawn
/// fails while another character still stands on the point.
pub struct SpawnOnPatrol {
    /// Class and walking speed (0 = the class's own).
    kinds: Vec<(String, f32)>,
    start_tag: String,
    start_name: String,
    count: usize,
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
        let yaw = yaw_of(toward.xy()).round() as i32;
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
        if w.spawn_actor(hp1_data::level::Actor { name, class, props, brush: None }).is_some() {
            self.count += 1;
        }
    }
}

impl Behaviour for SpawnOnPatrol {
    fn trigger(&mut self, w: &mut World, me: ActorId, _instigator: Option<ActorId>) {
        self.spawn(w, me);
    }

    fn touch(&mut self, w: &mut World, me: ActorId, _other: ActorId) {
        self.spawn(w, me);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behaviours::tests::{s, world_with};

    fn v(x: f32, y: f32, z: f32) -> DataValue {
        DataValue::Vector { x, y, z }
    }

    fn f(x: f32) -> DataValue {
        DataValue::Float(x as f64)
    }

    type Placed<'a> = (&'a str, &'a str, Vec<(&'a str, DataValue)>);

    fn point<'a>(name: &'a str, at: DataValue, next: &str) -> Placed<'a> {
        let mut props = vec![("Location", at)];
        if !next.is_empty() {
            props.push(("NextPatrol_ObjectName", s(next)));
        }
        (name, "Engine.PatrolPoint", props)
    }

    /// The settings HP1's students get from their classes.
    fn char_props<'a>() -> Vec<(&'a str, DataValue)> {
        vec![
            ("CollisionRadius", f(22.0)),
            ("CollisionHeight", f(42.0)),
            ("bBlockPlayers", DataValue::Bool(true)),
            ("GroundSpeed", f(100.0)),
            ("AccelRate", f(1024.0)),
            ("fNavPointColRadius", f(50.0)),
            ("walkAnimName", s("run")),
            ("idleAnimName", s("fidget_1")),
        ]
    }

    fn walker<'a>(extra: Vec<(&'a str, DataValue)>) -> Placed<'a> {
        let mut props = char_props();
        props.extend([
            ("Location", v(0.0, 0.0, 43.0)),
            ("bFollowPatrolPoints", DataValue::Bool(true)),
            ("firstPatrolPointObjectName", s("P0")),
        ]);
        props.extend(extra);
        ("Kid", "HPBase.baseChar", props)
    }

    fn floor_world(actors: Vec<Placed>) -> World {
        let mut w = world_with(actors);
        w.level.model = crate::collision::tests::floor();
        w
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

    fn anim(w: &World, id: ActorId) -> String {
        w.get(id).and_then(|e| e.anim.as_ref()).map_or(String::new(), |a| a.name.clone())
    }

    #[test]
    fn plain_walkers_cut_corners_and_stand_idle_at_the_end() {
        let mut w = floor_world(vec![
            point("P0", v(300.0, 0.0, 50.0), "P1"),
            point("P1", v(300.0, 300.0, 50.0), ""),
            walker(vec![]),
        ]);
        let path = run(&mut w, 2, 1.0);
        assert_eq!(anim(&w, 2), "run");
        let path = [path, run(&mut w, 2, 9.0)].concat();
        // Reached within its reach of each point, not on it.
        let at_p0 = closest(&path, Vec3::new(300.0, 0.0, 0.0));
        assert!(at_p0 > 2.0 && at_p0 < 50.0, "{at_p0}");
        let e = w.get(2).unwrap();
        assert!((e.pos - Vec3::new(300.0, 300.0, 50.0)).length() < 60.0, "{:?}", e.pos);
        assert!((e.pos.z - 42.0).abs() < 1.0, "stays on the floor: {:?}", e.pos);
        assert_eq!(anim(&w, 2), "fidget_1");
    }

    #[test]
    fn straight_walkers_pass_through_every_point() {
        let mut w = floor_world(vec![
            point("P0", v(200.0, 0.0, 50.0), "P1"),
            point("P1", v(200.0, 200.0, 50.0), ""),
            walker(vec![("bUseFrayMoveTo", DataValue::Bool(true))]),
        ]);
        let path = run(&mut w, 2, 6.0);
        assert!(closest(&path, Vec3::new(200.0, 0.0, 0.0)) < 4.0);
        assert!(closest(&path, Vec3::new(200.0, 200.0, 0.0)) < 4.0);
    }

    #[test]
    fn curves_pass_through_every_point() {
        let mut w = floor_world(vec![
            point("P0", v(200.0, 0.0, 50.0), "P1"),
            point("P1", v(400.0, 200.0, 50.0), "P2"),
            point("P2", v(600.0, 0.0, 50.0), ""),
            walker(vec![("bUseFraySplines", DataValue::Bool(true))]),
        ]);
        let path = run(&mut w, 3, 15.0);
        for p in [Vec3::new(400.0, 200.0, 0.0), Vec3::new(600.0, 0.0, 0.0)] {
            assert!(closest(&path, p) < 1.5, "missed {p}");
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
        let mut w = floor_world(vec![
            (
                "P0",
                "Engine.PatrolPoint",
                vec![
                    ("Location", v(150.0, 0.0, 50.0)),
                    ("NextPatrol_ObjectName", s("P1")),
                    ("pausetime", f(2.0)),
                    ("PauseAnim", s("look")),
                    ("EventToSend", s("song")),
                ],
            ),
            (
                "P1",
                "Engine.PatrolPoint",
                vec![("Location", v(400.0, 0.0, 50.0)), ("bDestroyPawn", DataValue::Bool(true))],
            ),
            ("MusicEvent0", "Engine.MusicEvent", vec![("Tag", s("song")), ("Song", s("Tune"))]),
            walker(vec![]),
        ]);
        run(&mut w, 3, 1.5);
        assert_eq!(w.outputs.len(), 1, "event sent on arrival");
        assert_eq!(anim(&w, 3), "look");
        let x = w.get(3).unwrap().pos.x;
        run(&mut w, 3, 1.0);
        assert!((w.get(3).unwrap().pos.x - x).abs() < 15.0, "stays while pausing");
        run(&mut w, 3, 4.0);
        assert!(w.get(3).is_none(), "removed at the last point");
    }

    fn spawner_world(collide: bool) -> World {
        let mut kid_class = char_props();
        kid_class.retain(|(k, _)| *k != "GroundSpeed");
        let mut w = floor_world(vec![
            point("P0", v(0.0, 0.0, 50.0), "P1"),
            point("P1", v(0.0, 400.0, 50.0), ""),
            (
                "Spawner",
                "HPBase.TriggerSpwnBsChrOnPPnt",
                vec![
                    ("Tag", s("kids")),
                    ("Location", v(0.0, -20.0, 40.0)),
                    ("CollisionRadius", f(40.0)),
                    ("CollisionHeight", f(40.0)),
                    ("bCollideActors", DataValue::Bool(collide)),
                    ("BaseCharToSpawn", s("Test.Student")),
                    ("BaseCharGroundSpeed", f(150.0)),
                    ("StartPatrolPoint_ObjectName", s("P0")),
                ],
            ),
        ]);
        let defaults = kid_class.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        w.classes.classes.insert(
            "Test.Student".into(),
            hp1_data::ClassEntry { parent: Some("HPBase.baseChar".into()), defaults },
        );
        w
    }

    #[test]
    fn spawn_trigger_sends_a_character_down_the_route() {
        let mut w = spawner_world(false);
        w.trigger_event("kids", None);
        assert_eq!(w.spawned.len(), 1);
        let kid = w.spawned[0];
        assert_eq!(w.get(kid).unwrap().rot[1], 16384, "faces the next point");
        // A second one can't be put where the first still stands.
        w.trigger_event("kids", None);
        assert_eq!(w.spawned.len(), 1);
        let path = run(&mut w, kid, 4.0);
        assert!(closest(&path, Vec3::new(0.0, 400.0, 0.0)) < 50.0);
        w.trigger_event("kids", None);
        assert_eq!(w.spawned.len(), 2, "the point is free again");
    }

    #[test]
    fn a_character_spawned_on_its_trigger_does_not_set_it_off_again() {
        let mut w = spawner_world(true);
        w.trigger_event("kids", None);
        let kid = w.spawned[0];
        run(&mut w, kid, 3.0);
        assert_eq!(w.spawned.len(), 1);
    }

    #[test]
    fn turning_takes_the_short_way() {
        assert_eq!(turn(65000.0, 500.0), 1036.0);
        assert_eq!(turn(500.0, 65000.0), -1036.0);
    }
}
