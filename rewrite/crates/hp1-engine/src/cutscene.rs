//! HP1 cutscenes (HPBase.CutScene): up to seven cast members each run a little script of
//! commands, waiting for each other through cues (`specs/cutscenes/CutScene.md`).

use glam::Vec3;
use hp1_data::DataValue;

use crate::camera::{NORMAL_PROX, NORMAL_SPEED, SavedCamera, rotation_towards, turn_shortest};
use crate::props::ActorProps;
use crate::puppet::{self, Kind};
use crate::world::{ActorId, Behaviour, Output, World};

/// Cast slots that run (the map may name more, which only `TALKn` can reach).
const RUN: usize = 7;
const CAST: usize = 12;
const LINES: usize = 40;
const PLACES: usize = 40;
/// Cues remembered per scene.
const MAX_CUES: usize = 60;

#[derive(Clone, Debug, Default)]
struct Member {
    actor: Option<ActorId>,
    alias: String,
    step: usize,
    delay: f32,
    waiting: Option<String>,
    finished: bool,
}

pub struct CutScene {
    play_once: bool,
    touch_starts: bool,
    trigger_starts: bool,
    level_load_starts: bool,
    can_play: bool,
    playing: bool,
    /// Actor names are looked up once the level runs.
    ready: bool,
    cast_names: Vec<String>,
    place_names: Vec<String>,
    scripts: Vec<Vec<String>>,
    cast: Vec<Member>,
    places: Vec<(Option<ActorId>, String)>,
    cues: Vec<String>,
    cue_count: usize,
    preface: Option<ActorId>,
}

/// A setting by name, ignoring letter case (the maps spell struct settings either way).
fn setting<'a>(p: &'a ActorProps, key: &str) -> Option<&'a DataValue> {
    p.get(key).or_else(|| p.actor.props.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v))
}

fn text(p: &ActorProps, key: &str) -> String {
    setting(p, key)
        .and_then(DataValue::as_str)
        .filter(|s| !s.eq_ignore_ascii_case("None"))
        .unwrap_or("")
        .to_owned()
}

fn indexed(key: &str, i: usize) -> String {
    if i == 0 { key.to_owned() } else { format!("{key}[{i}]") }
}

/// The number at the start of `s`, like the original's string-to-int (0 if there is none).
fn leading_int(s: &str) -> i64 {
    let s = s.trim_start();
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    digits[..end].parse::<i64>().map_or(0, |n| sign * n)
}

/// The number at the start of `s`, like the original's string-to-float (0 if there is none).
fn leading_float(s: &str) -> f32 {
    let s = s.trim_start();
    let end = s
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && i == 0)))
        .map_or(s.len(), |(i, _)| i);
    s[..end].parse().unwrap_or(0.0)
}

/// The text between the first `open` and the first `close` after it.
fn between(line: &str, open: char, close: char) -> Option<&str> {
    let start = line.find(open)?;
    let end = line.find(close)?;
    (end > start).then(|| &line[start + 1..end])
}

impl CutScene {
    pub fn new(p: &ActorProps) -> Self {
        let cast_names = (0..CAST).map(|i| text(p, &format!("{}.actor", indexed("Cast", i)))).collect();
        let place_names = (0..PLACES).map(|i| text(p, &format!("{}.actor", indexed("Locs", i)))).collect();
        let cast = (0..CAST)
            .map(|i| Member { alias: text(p, &format!("{}.alias", indexed("Cast", i))), ..Member::default() })
            .collect();
        let places = (0..PLACES).map(|i| (None, text(p, &format!("{}.alias", indexed("Locs", i))))).collect();
        let scripts = (0..RUN)
            .map(|c| (0..LINES).map(|l| text(p, &indexed(&format!("Cast{c}Script"), l))).collect())
            .collect();
        Self {
            play_once: p.bool_or("bPlayOnce", true),
            touch_starts: p.bool_or("bTouchStarts", true),
            trigger_starts: p.bool_or("bTriggerStarts", true),
            level_load_starts: p.bool("bLevelLoadStarts"),
            can_play: p.bool_or("bCanPlay", true),
            playing: false,
            ready: false,
            cast_names,
            place_names,
            scripts,
            cast,
            places,
            cues: Vec::new(),
            cue_count: 0,
            preface: None,
        }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Finds the cast's and places' actors ("Lev_Tut1.harry0" is the object named harry0).
    fn resolve(&mut self, w: &World) {
        let find = |name: &str| {
            let short = name.rsplit('.').next().unwrap_or(name);
            (!short.is_empty()).then(|| w.find_named(short)).flatten()
        };
        for (m, name) in self.cast.iter_mut().zip(&self.cast_names) {
            m.actor = find(name);
        }
        for (p, name) in self.places.iter_mut().zip(&self.place_names) {
            p.0 = find(name);
        }
    }

    fn start(&mut self) {
        if !self.playing && self.can_play {
            if self.play_once {
                self.can_play = false;
            }
            self.playing = true;
        }
    }

    fn reset(&mut self) {
        self.can_play = true;
        self.playing = false;
        for m in &mut self.cast {
            *m = Member { actor: m.actor, alias: std::mem::take(&mut m.alias), ..Member::default() };
        }
        self.cues.clear();
        self.cue_count = 0;
    }

    fn give_cue(&mut self, cue: &str) {
        let cue = cue.to_ascii_uppercase();
        if self.cue_count < MAX_CUES {
            self.cues.push(cue.clone());
        }
        self.cue_count += 1;
        for m in &mut self.cast[..RUN] {
            if m.waiting.as_deref() == Some(cue.as_str()) {
                m.waiting = None;
            }
        }
    }

    /// The actor a script argument names (see the spec for the order).
    fn lookup(&self, w: &mut World, arg: &str) -> Option<ActorId> {
        let s = arg.to_ascii_uppercase();
        if s == "HARRY" {
            return w.player;
        }
        if let Some(m) = self.cast[..RUN].iter().find(|m| m.alias.to_ascii_uppercase() == s) {
            return m.actor;
        }
        if let Some(p) = self.places.iter().find(|p| p.0.is_some() && p.1.to_ascii_uppercase() == s) {
            return p.0;
        }
        if let Some(rest) = s.strip_prefix("CAST") {
            let n = leading_int(&rest.chars().take(4).collect::<String>());
            if (0..6).contains(&n) {
                return self.cast[n as usize].actor;
            }
        }
        if let Some(rest) = s.strip_prefix("LOC") {
            let n = leading_int(&rest.chars().take(3).collect::<String>());
            if (0..PLACES as i64).contains(&n) {
                return self.places[n as usize].0;
            }
        }
        w.note(format!("cutscene: no target named {arg}"));
        None
    }

    /// A member's speaker for `TALKn` / `EMOTEn`: cast slot n when the character at `at` in
    /// the line is a digit, else the member itself.
    fn speaker(&self, me: Option<ActorId>, line: &str, at: usize) -> Option<ActorId> {
        let rest = line.get(at..).unwrap_or("");
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            let n = leading_int(&rest.chars().take(at).collect::<String>()) as usize;
            self.cast.get(n).and_then(|m| m.actor)
        } else {
            me
        }
    }

    fn run(&mut self, w: &mut World, me: ActorId, dt: f32) {
        let mut all_finished = true;
        for i in 0..RUN {
            if self.cast[i].finished {
                continue;
            }
            all_finished = false;
            self.run_member(w, me, i, dt);
        }
        if all_finished {
            self.playing = false;
            if !self.play_once {
                self.reset();
            }
        }
    }

    fn run_member(&mut self, w: &mut World, me: ActorId, i: usize, dt: f32) {
        let actor = self.cast[i].actor.filter(|&a| w.get(a).is_some());
        let Some(actor) = actor else {
            self.cast[i].finished = true;
            return;
        };
        if let Some(c) = &self.cast[i].waiting
            && self.cues.iter().any(|x| x.eq_ignore_ascii_case(c))
        {
            self.cast[i].waiting = None;
        }
        if self.cast[i].waiting.is_some() {
            return;
        }
        self.cast[i].delay -= dt;
        if self.cast[i].delay > 0.0 {
            return;
        }
        let line = self.scripts[i][self.cast[i].step].clone();
        self.command(w, me, i, actor, &line);
        self.cast[i].step += 1;
        if self.cast[i].step >= LINES {
            self.cast[i].finished = true;
        }
    }

    fn command(&mut self, w: &mut World, me: ActorId, i: usize, actor: ActorId, line: &str) {
        let upper = line.to_ascii_uppercase();
        let (op, arg) = match upper.find(' ') {
            Some(p) => (&upper[..p], &upper[p + 1..]),
            None => (upper.as_str(), ""),
        };
        let kind = puppet::kind_of(w, actor);
        let camera = w.camera.actor == Some(actor);
        let pos = |w: &World, id: Option<ActorId>| id.and_then(|id| w.get(id)).map(|e| e.pos);
        // The cue a move or turn gives when done: line number and actor name.
        let auto = format!("{}{}", self.cast[i].step, w.get(actor).map_or("", |e| e.name.as_str()));
        match op {
            "" | "NONE" | "SOUND" => {}
            "TRIGGER" => w.trigger_event(arg, None),
            "GOTO" | "TELEPORT" => {
                let target = self.lookup(w, arg);
                // A missing target would put the actor at the map's origin in the original.
                let Some(to) = pos(w, target) else { return };
                if camera {
                    if let Some(p) = self.preface.take() {
                        w.camera.direction = Some(p);
                        if let Some(look) = pos(w, Some(p)) {
                            w.camera.rot = rotation_towards(look - to);
                            w.camera.desired = w.camera.rot;
                        }
                    }
                    w.camera.position = target;
                    set_camera_pos(w, to);
                } else if let Some(e) = w.get_mut(actor) {
                    e.pos = to;
                }
            }
            "MOVETO" => {
                let target = self.lookup(w, arg);
                self.cast[i].waiting = Some(auto.clone());
                if camera {
                    w.camera.position = target;
                    w.camera.notify = Some((me, auto.clone()));
                    if let Some(p) = self.preface.take() {
                        w.camera.direction = Some(p);
                    }
                    if target.is_none() {
                        w.camera.notify = None;
                        self.give_cue(&auto);
                    }
                } else if kind.is_some() {
                    match target {
                        Some(t) => puppet::walk_to(w, actor, t, (me, auto)),
                        None => self.give_cue(&auto),
                    }
                }
            }
            "TURNTO" => {
                let target = self.lookup(w, arg);
                if camera {
                    w.camera.direction = target;
                } else if kind.is_some() {
                    self.cast[i].waiting = Some(auto.clone());
                    match target {
                        Some(t) => puppet::turn_to(w, actor, t, (me, auto)),
                        None => self.give_cue(&auto),
                    }
                }
            }
            "FACE" => {
                let target = self.lookup(w, arg);
                let (Some(to), Some(from)) = (pos(w, target), pos(w, Some(actor))) else { return };
                if camera {
                    w.camera.direction = target;
                    w.camera.rot = rotation_towards(to - from);
                    w.camera.desired = w.camera.rot;
                } else {
                    puppet::face(w, actor, to);
                }
            }
            "PREFACE" => {
                self.preface = self.lookup(w, arg);
                if !camera {
                    w.note(format!("cutscene: PREFACE on {actor}, which isn't the camera"));
                }
            }
            "ANIMATE" => {
                if kind.is_some() {
                    puppet::animate(w, actor, arg);
                }
            }
            "CAPTURE" => {
                if kind == Some(Kind::Harry) {
                    w.cut_mode = true;
                    puppet::capture(w, actor);
                } else if camera {
                    enter_camera(w);
                } else if kind.is_some() {
                    puppet::capture(w, actor);
                }
            }
            "RELEASE" => {
                if kind == Some(Kind::Harry) {
                    w.cut_mode = false;
                    puppet::release(w, actor);
                } else if camera {
                    leave_camera(w, true, false);
                } else if kind.is_some() {
                    puppet::release(w, actor);
                }
            }
            "SAY" => self.cast[i].delay = crate::dialog::deliver(w, arg),
            "TALK" | "TALK0" | "TALK1" | "TALK2" | "TALK3" | "TALK4" | "TALK5" | "TALK6" => {
                let who = self.speaker(Some(actor), line, 4);
                let seconds = crate::dialog::deliver(w, arg);
                if let Some(who) = who {
                    puppet::talk(w, who, seconds);
                }
                self.cast[i].delay = seconds;
            }
            "EMOTE" | "EMOTE0" | "EMOTE1" | "EMOTE2" | "EMOTE3" | "EMOTE4" | "EMOTE5" | "EMOTE6" => {
                let who = self.speaker(Some(actor), line, 5);
                match who.filter(|&w_| puppet::kind_of(w, w_) == Some(Kind::Character)) {
                    Some(who) => self.cast[i].delay = emote_line(w, who, line),
                    None => w.note(format!("cutscene: EMOTE on {who:?}, which isn't a character")),
                }
            }
            "TEXT" => w.outputs.push(Output::Message { text: arg.to_owned(), seconds: 4.0 }),
            "CUE" => self.give_cue(arg),
            "WAITFOR" => self.cast[i].waiting = Some(arg.to_owned()),
            "SLEEP" => self.cast[i].delay = leading_float(arg),
            "CHANGELEVEL" => w.outputs.push(Output::ChangeLevel { map: arg.to_owned() }),
            "SETIDLE" | "SETWALK" => {
                if kind == Some(Kind::Character) {
                    if op == "SETIDLE" {
                        puppet::set_idle(w, actor, arg);
                    } else {
                        puppet::set_walk(w, actor, arg);
                    }
                } else {
                    w.note(format!("cutscene: {op} only works on characters"));
                }
            }
            "CAMRESTORE" => {
                if camera {
                    leave_camera(w, false, true);
                }
            }
            "CAMSPEED" => {
                if camera {
                    let (speed, turn) = match arg.find(' ') {
                        Some(p) => (&arg[..p], Some(&arg[p + 1..])),
                        None => (arg, None),
                    };
                    w.camera.speed = NORMAL_SPEED * leading_float(speed);
                    if let Some(t) = turn {
                        let f = leading_float(t);
                        w.camera.rate = w.camera.normal_rate.map(|r| (r * f).trunc());
                    }
                }
            }
            "CAMPROX" => {
                if camera {
                    w.camera.prox = leading_float(arg);
                }
            }
            "FADEIN" => w.fade.fade_in(leading_float(arg)),
            "FADEOUT" => w.fade.fade_out(leading_float(arg)),
            _ => w.note(format!("cutscene: can't read the line {line:?}")),
        }
    }
}

/// A character's emote line: dialogue in `<…>`, an event in `[…]`. Returns how long to wait.
fn emote_line(w: &mut World, who: ActorId, line: &str) -> f32 {
    // A character already emoting skips the whole line.
    if puppet::is_emoting(w, who) {
        return 0.0;
    }
    let mut seconds = 2.0;
    if let Some(id) = between(line, '<', '>') {
        seconds = crate::dialog::deliver(w, id);
    }
    if let Some(name) = between(line, '(', ')') {
        puppet::emote(w, who, name, seconds);
    }
    if let Some(event) = between(line, '[', ']') {
        w.trigger_event(event, Some(who));
    }
    // Room for the emote animation to finish.
    seconds + 0.75
}

impl Behaviour for CutScene {
    fn tick(&mut self, w: &mut World, me: ActorId, dt: f32) {
        if !self.ready {
            self.ready = true;
            self.resolve(w);
            if self.level_load_starts {
                self.start();
            }
        }
        if self.playing {
            self.run(w, me, dt);
        }
    }

    fn trigger(&mut self, _w: &mut World, _me: ActorId, _instigator: Option<ActorId>) {
        if self.trigger_starts {
            self.start();
        }
    }

    fn touch(&mut self, w: &mut World, _me: ActorId, other: ActorId) {
        if self.touch_starts && puppet::kind_of(w, other) == Some(Kind::Harry) {
            self.start();
        }
    }

    fn cue(&mut self, _w: &mut World, _me: ActorId, cue: &str) {
        self.give_cue(cue);
    }
}

// ---------------------------------------------------------------------------------------------
// The camera in cutscene mode

fn set_camera_pos(w: &mut World, pos: Vec3) {
    w.camera.pos = pos;
    if let Some(e) = w.camera.actor.and_then(|id| w.get_mut(id)) {
        e.pos = pos;
    }
}

fn harry(w: &World) -> (Vec3, i32) {
    w.player.and_then(|p| w.get(p)).map_or((Vec3::ZERO, 0), |e| (e.pos, e.rot[1]))
}

fn enter_camera(w: &mut World) {
    let (harry_pos, harry_yaw) = harry(w);
    let c = &mut w.camera;
    c.stack.push(SavedCamera {
        speed: c.speed,
        rate: c.rate,
        position: c.position,
        direction: c.direction,
        offset: c.pos - harry_pos,
        harry_yaw,
        rot: c.rot,
        desired: c.desired,
    });
    c.active = true;
    if let Some(look) = c.direction.and_then(|d| w.get(d)).map(|e| e.pos) {
        w.camera.desired = rotation_towards(look - w.camera.pos);
    }
    if let Some(at) = w.camera.position.and_then(|p| w.get(p)).map(|e| e.pos) {
        set_camera_pos(w, at);
    }
}

/// `RELEASE` (pop the saved settings) or `CAMRESTORE` (keep them, and go back to the saved
/// place behind Harry).
fn leave_camera(w: &mut World, pop: bool, restore_place: bool) {
    let (harry_pos, harry_yaw) = harry(w);
    let c = &mut w.camera;
    if pop {
        c.prox = NORMAL_PROX;
    }
    let saved = c.stack.last().cloned();
    if pop {
        c.stack.pop();
    }
    if let Some(s) = saved {
        c.speed = s.speed;
        c.rate = s.rate;
        c.position = s.position;
        c.direction = s.direction;
        if restore_place {
            let turn = (s.harry_yaw - harry_yaw) as f32 * crate::units::ROT_TO_RAD;
            let (sin, cos) = turn.sin_cos();
            let o = s.offset;
            let pos = harry_pos + Vec3::new(o.x * cos - o.y * sin, o.x * sin + o.y * cos, o.z);
            c.rot = s.rot;
            c.desired = s.desired;
            set_camera_pos(w, pos);
        }
    }
    w.camera.active = false;
    w.camera.left = true;
}

/// One tick of the camera; returns the cue to give when it reaches its spot.
pub(crate) fn tick_camera(w: &mut World, dt: f32) -> Option<(ActorId, String)> {
    if !w.camera.active {
        return None;
    }
    let mut cue = None;
    if let Some(spot) = w.camera.position.and_then(|p| w.get(p)).map(|e| e.pos) {
        let c = &w.camera;
        let pos = c.pos + (spot - c.pos) * (c.speed * dt).min(1.0);
        set_camera_pos(w, pos);
        if pos.distance(spot) < w.camera.prox {
            cue = w.camera.notify.take();
        }
    }
    if let Some(look) = w.camera.direction.and_then(|d| w.get(d)).map(|e| e.pos) {
        w.camera.desired = rotation_towards(look - w.camera.pos);
    }
    let c = &mut w.camera;
    for k in 0..3 {
        c.rot[k] = turn_shortest(c.rot[k], c.desired[k], (c.rate[k] * dt).abs() as i32);
    }
    cue
}

#[cfg(test)]
mod tests;
