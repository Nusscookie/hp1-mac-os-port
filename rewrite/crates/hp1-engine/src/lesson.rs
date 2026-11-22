//! Spell lessons: tracing a spell's pattern with the wand (`specs/spells/SpellLesson.md`).

use glam::{Vec2, Vec3};
use hp1_data::{DataValue, PropMap};

use crate::camera::rotation_towards;
use crate::gesture::Pattern;
use crate::hud::{Countdown, Popup};
use crate::props::ActorProps;
use crate::units::ROT_TO_RAD;
use crate::world::{ActorId, Behaviour, Output, World};

/// Slots in a drawing.
const SLOTS: usize = 500;
/// How far ahead of the trigger the camera stands.
const CAMERA_DIST: f32 = 20.0;
/// Our rule: wand movement per pixel of mouse movement (the original's mouse scale is native).
pub const WAND_PER_PIXEL: f32 = 1.0 / 2000.0;
/// The original's key movement: wand per second per unit of the key axis.
const KEY_SPEED: f32 = 20.0 / 64000.0;
/// The default key axes: forward, back and sideways.
const KEY_FORWARD: f32 = 300.0;
const KEY_BACK: f32 = 150.0;
const KEY_SIDE: f32 = 150.0;

pub const BEGIN_SOUND: &str = "HPSounds.magic_sfx.spell_learn_begin_02";
pub const TRACE_SOUND: &str = "HPSounds.magic_sfx.spell_learn_trace_08";
pub const ERROR_SOUND: &str = "HPSounds.magic_sfx.spell_learn_error_02";
pub const END_SOUND: &str = "HPSounds.magic_sfx.spell_learn_end_03";
pub const CORRECT_SOUND: &str = "HPSounds.magic_sfx.spell_trace_correct";
pub const INCORRECT_SOUND: &str = "HPSounds.magic_sfx.spell_trace_incorrect";
const TEMPLATE_FX: &str = "HPParticle.SilverSparkle01";
const BAD_POINT_FX: &str = "HPParticle.BadDrawPoint";
const FAIL_FX: &str = "HPParticle.RewardFail";

/// The lesson's settings (from the placed trigger and its spell).
#[derive(Clone, Debug, Default)]
struct Config {
    spell: String,
    gesture: String,
    incantation: String,
    hit_fx: String,
    teacher: String,
    event: String,
    draw_time: f32,
    fov_ratio: f32,
    template_dist: f32,
    wand_dist: f32,
    accuracy: f32,
    pass_marks: Vec<f32>,
    house_points: Vec<i32>,
    vgood: f32,
    vbad: f32,
    intro: Vec<String>,
    judge_vbad: Vec<String>,
    judge_failure: Vec<String>,
    judge_success: Vec<String>,
    judge_vgood: Vec<String>,
    repeat_failure: Vec<String>,
    repeat_success: Vec<String>,
    full_marks: String,
    points: Vec<String>,
    wand_class: String,
    wand_fx: String,
    draw_fx: String,
    win_fx: String,
}

fn indexed(key: &str, i: usize) -> String {
    if i == 0 { key.to_owned() } else { format!("{key}[{i}]") }
}

fn name_value(p: &ActorProps, key: &str) -> String {
    let t = p.text(key);
    if t.eq_ignore_ascii_case("None") { String::new() } else { t }
}

/// The filled entries of a string array, in order until the first empty one.
fn lines(p: &ActorProps, key: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| name_value(p, &indexed(key, i))).take_while(|s| !s.is_empty()).collect()
}

/// An object's own name from a reference such as `Lev_Tut1.Tut1Quirrell0`.
fn object_name(r: &str) -> &str {
    r.rsplit('.').next().unwrap_or(r)
}

/// The spell lesson's steps; each waits `wait` seconds (or for the replay) before the next.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    /// Waiting for Harry to settle on his place.
    Settle,
    /// The teacher's opening line of a round.
    Show,
    Trace,
    /// The replay of the drawing; checks whether it's over every second.
    Counting,
    /// One more second after the replay.
    Counted,
    /// After the pass or fail effects, before the teacher's verdict.
    Verdict,
    /// After the verdict line.
    VerdictSaid,
    /// After the house points line; `last` when the lesson ends then.
    PointsSaid {
        last: bool,
    },
    /// The final popup.
    Final,
}

/// What a running lesson made and where it stands.
#[derive(Debug)]
struct Run {
    pattern: Pattern,
    origin: Vec3,
    yaw: f32,
    trigger: ActorId,
    harry: ActorId,
    teacher: Option<ActorId>,
    /// The trail's own emission rate, switched off while the button is up.
    draw_rate: crate::particles::Range,
    blackboard: Option<ActorId>,
    template: Option<ActorId>,
    wand: Option<ActorId>,
    wand_fx: Option<ActorId>,
    draw_fx: Option<ActorId>,
    redraw_fx: Option<ActorId>,
    template_dist: f32,
    template_time: f32,
    round: usize,
    first_time: bool,
    total_points: i32,
    wand_pos: Vec2,
    drawn: Vec<Option<Vec2>>,
    time_drawn: f32,
    started: bool,
    success: f32,
    passed: bool,
    replayed: f32,
    replay_done: bool,
    step: Step,
    wait: f32,
}

pub struct SpellLesson {
    cfg: Config,
    run: Option<Run>,
    done: bool,
}

impl SpellLesson {
    pub fn new(p: &ActorProps, classes: &hp1_data::ClassTable) -> Self {
        let spell = name_value(p, "Spell");
        let spell_defaults = classes.effective_defaults(&spell);
        let spell_text = |k: &str| spell_defaults.get(k).and_then(DataValue::as_str).unwrap_or("").to_owned();
        let cfg = Config {
            gesture: spell_text("Gesture"),
            incantation: spell_text("SpellIncantation"),
            hit_fx: spell_text("hitParticleEffectClass"),
            spell,
            teacher: object_name(&name_value(p, "Teacher")).to_owned(),
            event: name_value(p, "Event"),
            draw_time: p.f32("DrawTime", 6.0),
            fov_ratio: p.f32("FOVRatio", 0.8),
            template_dist: p.f32("TemplateDist", 105.0),
            wand_dist: p.f32("WandDist", 100.0),
            accuracy: p.f32("fAccuracy", 0.03),
            pass_marks: (0..10).map(|i| p.f32(&indexed("PassMark", i), 1.0)).collect(),
            house_points: (0..10).map(|i| p.int(&indexed("iNumHousePoints", i), 0) as i32).collect(),
            vgood: p.f32("fVGoodThreshold", 0.8),
            vbad: p.f32("fVBadThreshold", 0.6),
            intro: lines(p, "LessonIntro", 10),
            judge_vbad: lines(p, "LessonJudgeVBad", 10),
            judge_failure: lines(p, "LessonJudgeFailure", 10),
            judge_success: lines(p, "LessonJudgeSuccess", 10),
            judge_vgood: lines(p, "LessonJudgeVGood", 10),
            repeat_failure: lines(p, "LessonRepeatFailure", 10),
            repeat_success: lines(p, "LessonRepeatSuccess", 10),
            full_marks: name_value(p, "LessonFullMarks"),
            points: (0..4).map(|i| name_value(p, &indexed("LessonPoints", i))).collect(),
            wand_class: name_value(p, "WandClass"),
            wand_fx: name_value(p, "WandFXClass"),
            draw_fx: name_value(p, "DrawFXClass"),
            win_fx: name_value(p, "WinFXClass[2]"),
        };
        Self { cfg, run: None, done: false }
    }

    /// Whether the lesson is running.
    pub fn running(&self) -> bool {
        self.run.is_some()
    }
}

// ---------------------------------------------------------------------------------------------
// Places

fn forward(yaw: f32) -> Vec3 {
    let a = yaw * ROT_TO_RAD;
    Vec3::new(a.cos(), a.sin(), 0.0)
}

fn right(yaw: f32) -> Vec3 {
    Vec3::Z.cross(forward(yaw))
}

impl Run {
    fn camera(&self) -> Vec3 {
        self.origin + forward(self.yaw) * CAMERA_DIST
    }

    /// Straight ahead of the camera, `TemplateDist` away: where the template and effects go.
    fn template_place(&self) -> Vec3 {
        self.camera() + forward(self.yaw) * self.template_dist
    }

    /// The look point (`TemplateDist` ahead of the trigger).
    fn look(&self, cfg: &Config) -> Vec3 {
        self.origin + forward(self.yaw) * cfg.template_dist
    }

    fn wand_place(&self, cfg: &Config) -> Vec3 {
        let (f, r) = (forward(self.yaw), right(self.yaw));
        let w = self.wand_pos * cfg.fov_ratio;
        self.camera() + (f + r * w.x + Vec3::Z * w.y) * cfg.wand_dist
    }

    fn rot(&self) -> [i32; 3] {
        [0, self.yaw as i32, 0]
    }
}

/// A pattern point as a wand position.
fn wand_of(p: Vec2) -> Vec2 {
    Vec2::new(p.x - 0.5, 0.5 - p.y)
}

fn spawn(w: &mut World, class: &str, name: String, at: Vec3, rot: [i32; 3]) -> Option<ActorId> {
    if class.is_empty() {
        return None;
    }
    let mut props = PropMap::new();
    props.insert("Location".into(), DataValue::Vector { x: at.x, y: at.y, z: at.z });
    props.insert("Rotation".into(), DataValue::Rotator { pitch: rot[0], yaw: rot[1], roll: rot[2] });
    let id = w.spawn_actor(hp1_data::level::Actor { name, class: class.to_owned(), props, brush: None })?;
    // Spawned pieces stay where the lesson puts them (no physics), but they move.
    if let Some(e) = w.get_mut(id) {
        e.behaviour.get_or_insert_with(|| Box::new(Held));
    }
    Some(id)
}

/// A spawned piece the lesson moves around.
struct Held;
impl Behaviour for Held {}

fn remove(w: &mut World, id: &mut Option<ActorId>) {
    if let Some(id) = id.take() {
        w.remove(id);
    }
}

fn set_pos(w: &mut World, id: Option<ActorId>, pos: Vec3) {
    if let Some(e) = id.and_then(|id| w.get_mut(id)) {
        e.moved = pos - e.pos;
        e.pos = pos;
    }
}

fn system(w: &mut World, id: Option<ActorId>) -> Option<&mut crate::particles::System> {
    w.particles.get_mut(&id?)
}

/// A line from `list` picked at random; how long it lasts (voice + 1 s, else `fallback`).
fn say_one(w: &mut World, list: &[String], fallback: f32) -> f32 {
    if list.is_empty() {
        return fallback;
    }
    let id = list[w.rand(list.len())].clone();
    say(w, &id, fallback)
}

/// Says line `id` with its subtitle; lasts its voice + 1 s, else `fallback`.
fn say(w: &mut World, id: &str, fallback: f32) -> f32 {
    let text = w.lines.text(id);
    let seconds = match w.lines.voice(id) {
        Some((path, length)) => {
            w.outputs.push(Output::Voice { path });
            length + 1.0
        }
        None => fallback,
    };
    w.hud.show_line(&text, seconds);
    w.outputs.push(Output::Message { text, seconds });
    seconds
}

fn teacher_loop(w: &mut World, run: &Run, anim: &str) {
    if let Some(t) = run.teacher {
        crate::puppet::loop_anim(w, t, anim);
    }
}

impl SpellLesson {
    fn start(&mut self, w: &mut World, me: ActorId) {
        let Some(harry) = w.player else { return };
        let Some(e) = w.get(me) else { return };
        let (origin, yaw) = (e.pos, e.rot[1] as f32);
        let pattern = w.gestures.get(&self.cfg.gesture).cloned().unwrap_or_default();
        if pattern.points.is_empty() {
            w.note(format!("{}: no pattern {}", e.name, self.cfg.gesture));
        }
        let teacher = w.find_named(&self.cfg.teacher);
        let mut run = Run {
            pattern,
            origin,
            yaw,
            trigger: me,
            harry,
            teacher,
            draw_rate: Default::default(),
            blackboard: None,
            template: None,
            wand: None,
            wand_fx: None,
            draw_fx: None,
            redraw_fx: None,
            template_dist: self.cfg.template_dist,
            template_time: self.cfg.draw_time / 4.0,
            round: 0,
            first_time: true,
            total_points: 0,
            wand_pos: Vec2::ZERO,
            drawn: vec![None; SLOTS],
            time_drawn: 0.0,
            started: false,
            success: 0.0,
            passed: false,
            replayed: 0.0,
            replay_done: false,
            step: Step::Settle,
            wait: 2.0,
        };
        // Harry stops; the camera holds still in front of the trigger, looking ahead.
        crate::puppet::capture(w, harry);
        crate::cutscene::hold_camera(w, run.camera(), rotation_towards(run.look(&self.cfg) - origin));
        let board = origin + forward(yaw) * (self.cfg.template_dist + 50.0);
        run.blackboard = spawn(w, "HPBase.SpellBlackboard", "SpellBlackboard".into(), board, [0; 3]);
        if let Some(t) = teacher {
            crate::puppet::capture(w, t);
        }
        crate::puppet::put(w, harry, origin);
        self.run = Some(run);
    }

    /// The start of each round: the teacher's line and the template drawing itself.
    fn show(&mut self, w: &mut World) {
        let cfg = &self.cfg;
        let Some(run) = &mut self.run else { return };
        w.hud.lesson_score = None;
        w.hud.lesson_pass = cfg.pass_marks.get(run.round).copied().unwrap_or(1.0);
        run.drawn = vec![None; SLOTS];
        w.hud.popup = None;
        w.cut_mode = true;
        teacher_loop(w, run, "talk");
        let seconds = if run.round > 0 {
            say_one(w, &cfg.repeat_success, 5.0)
        } else if run.first_time {
            say_one(w, &cfg.intro, 5.0)
        } else {
            say_one(w, &cfg.repeat_failure, 5.0)
        };
        run.first_time = false;
        if let Some(t) = run.teacher {
            let anim = match cfg.spell.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
                "spellflip" => Some("flipendo"),
                "spellincendio" => Some("Incendio"),
                "spelllev" => Some("Wingardium"),
                "spelllumas" => Some("Lumos"),
                _ => None,
            };
            if let Some(a) = anim {
                crate::puppet::animate(w, t, a);
            }
        }
        // The template: sparkles spread along the pattern over a quarter of the drawing time.
        let at = run.template_place();
        run.template = spawn(w, TEMPLATE_FX, "SpellTemplate".into(), at, run.rot());
        let size = 3.0 * cfg.accuracy / 0.01;
        let scale = run.template_dist * cfg.fov_ratio;
        if let Some(s) = system(w, run.template) {
            s.settings.lifetime = crate::particles::Range { base: 99_999_999.0, rand: 0.0 };
            s.settings.width.base = size;
            s.settings.length.base = size;
            s.settings.draw_scale = scale;
            s.settings.period = crate::particles::Range::default();
            s.pattern = Some(run.pattern.clone());
        }
        run.step = Step::Show;
        run.wait = seconds;
    }

    fn begin_trace(&mut self, w: &mut World) {
        let cfg = &self.cfg;
        let Some(run) = &mut self.run else { return };
        teacher_loop(w, run, "breathe");
        w.cut_mode = false;
        w.hud.popup = Some(Popup::Grade {
            grade: w.lines.localized("Pickup", "all", "lesson_text_02"),
            pass: w.lines.localized("Pickup", "all", "lesson_text_01"),
        });
        run.started = false;
        run.wand_pos = run.pattern.points.first().map_or(Vec2::ZERO, |&p| wand_of(p));
        // The original spawns the wand without the field-of-view squeeze.
        let (f, r) = (forward(run.yaw), right(run.yaw));
        let first = run.camera() + (f + r * run.wand_pos.x + Vec3::Z * run.wand_pos.y) * cfg.wand_dist;
        run.wand = spawn(w, &cfg.wand_class, "SpellWand".into(), first, [-5000, 0, 5000]);
        run.wand_fx = spawn(w, &cfg.wand_fx, "WandFX".into(), first, run.rot());
        run.draw_fx = spawn(w, &cfg.draw_fx, "DrawFX".into(), first, run.rot());
        if let Some(s) = system(w, run.draw_fx) {
            s.settings.width.base = 5.0;
            s.settings.length.base = 5.0;
            run.draw_rate = s.settings.per_sec;
        }
        run.time_drawn = 0.0;
        run.step = Step::Trace;
        w.wand_input = Default::default();
        w.wand_wanted = true;
    }

    fn trace(&mut self, w: &mut World, dt: f32) {
        let cfg = &self.cfg;
        let Some(run) = &mut self.run else { return };
        let input = std::mem::take(&mut w.wand_input);
        w.wand_input.draw = input.draw;
        let key_y = input.keys.y * if input.keys.y > 0.0 { KEY_FORWARD } else { KEY_BACK };
        let moved = Vec2::new(input.mouse.x, -input.mouse.y) * WAND_PER_PIXEL
            + Vec2::new(input.keys.x * KEY_SIDE, key_y) * KEY_SPEED * dt;
        run.wand_pos = (run.wand_pos + moved).clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
        w.wand_at = Some((Vec2::new(run.wand_pos.x + 0.5, 0.5 - run.wand_pos.y), cfg.gesture.clone()));
        let at = run.wand_place(cfg);
        for id in [run.wand, run.wand_fx, run.draw_fx] {
            set_pos(w, id, at);
        }
        if run.time_drawn == 0.0 {
            w.hud.countdown = Some(Countdown::new(cfg.draw_time));
        }
        if input.draw && run.time_drawn <= cfg.draw_time {
            if !run.started {
                run.started = true;
                w.sound(run.trigger, BEGIN_SOUND);
                if let Some(wand) = run.wand {
                    w.sound(wand, TRACE_SOUND);
                }
            }
            let rate = run.draw_rate;
            if let Some(s) = system(w, run.draw_fx) {
                s.settings.per_sec = rate;
            }
            let n = (SLOTS as f32 * run.time_drawn / cfg.draw_time) as usize;
            if n < SLOTS {
                run.drawn[n] = Some(Vec2::new(run.wand_pos.x + 0.5, 0.5 - run.wand_pos.y));
            }
        } else if (run.time_drawn > 0.0 && run.started) || run.time_drawn > cfg.draw_time {
            run.time_drawn += dt;
            return self.judge(w);
        } else if let Some(s) = system(w, run.draw_fx) {
            s.settings.per_sec.base = 0.0;
            s.restart_trail();
        }
        run.time_drawn += dt;
    }

    fn judge(&mut self, w: &mut World) {
        let cfg = &self.cfg;
        let Some(run) = &mut self.run else { return };
        w.wand_wanted = false;
        w.wand_at = None;
        w.hud.countdown = None;
        w.sound(run.trigger, END_SOUND);
        if let Some(wand) = run.wand {
            w.sound(wand, TRACE_SOUND);
        }
        run.replayed = 0.0;
        run.time_drawn /= 2.0;
        run.success = run.pattern.score(&run.drawn, cfg.accuracy);
        w.hud.lesson_score = Some(run.success);
        run.wand_pos = run.pattern.points.first().map_or(Vec2::ZERO, |&p| wand_of(p));
        let at = run.wand_place(cfg);
        for id in [run.wand, run.wand_fx] {
            set_pos(w, id, at);
        }
        remove(w, &mut run.draw_fx);
        run.redraw_fx = spawn(w, BAD_POINT_FX, "ReDrawFX".into(), at, run.rot());
        run.draw_fx = spawn(w, &cfg.draw_fx, "DrawFX".into(), at, run.rot());
        w.hud.score_count = Some((run.time_drawn, run.time_drawn));
        run.replay_done = false;
        run.step = Step::Counting;
        run.wait = 1.0;
    }

    /// The replay of the drawing at double speed.
    fn replay(&mut self, w: &mut World, dt: f32) {
        let cfg = &self.cfg;
        let Some(run) = &mut self.run else { return };
        if run.replay_done {
            return;
        }
        if !run.started {
            run.replay_done = true;
            return;
        }
        let half = cfg.draw_time / 2.0;
        let from = (SLOTS as f32 * run.replayed / half) as usize;
        let to = (SLOTS as f32 * (run.replayed + dt) / half) as usize;
        for n in from..to.min(SLOTS) {
            let Some(p) = run.drawn[n] else { continue };
            run.wand_pos = wand_of(p);
            let off = run.pattern.distance(p) > cfg.accuracy;
            let (bad, good) = (run.redraw_fx, run.draw_fx);
            let bad_on = system(w, bad).is_some_and(|s| s.settings.parent_blend == 0.0);
            if off != bad_on {
                if let Some(s) = system(w, bad) {
                    s.settings.parent_blend = if off { 0.0 } else { 1.0 };
                }
                if let Some(s) = system(w, good) {
                    s.settings.parent_blend = if off { 1.0 } else { 0.0 };
                }
                if let Some(wand) = run.wand {
                    w.sound(wand, if off { ERROR_SOUND } else { TRACE_SOUND });
                }
            }
        }
        let at = run.wand_place(cfg);
        for id in [run.wand, run.wand_fx, run.redraw_fx, run.draw_fx] {
            set_pos(w, id, at);
        }
        if run.replayed >= run.time_drawn {
            run.replay_done = true;
        }
        run.replayed += dt;
    }

    fn destroy_wand(w: &mut World, run: &mut Run) {
        remove(w, &mut run.wand);
        remove(w, &mut run.wand_fx);
        remove(w, &mut run.draw_fx);
        remove(w, &mut run.redraw_fx);
    }

    /// An effect spread over the whole pattern, in front of the camera.
    fn pattern_fx(w: &mut World, run: &Run, cfg: &Config, class: &str) -> Option<ActorId> {
        let id = spawn(w, class, "LessonFX".into(), run.template_place(), run.rot());
        if let Some(s) = system(w, id) {
            s.settings.draw_scale = run.template_dist * cfg.fov_ratio;
            s.settings.period = crate::particles::Range { base: 0.0, rand: 1.0 };
            s.pattern = Some(run.pattern.clone());
        }
        id
    }

    fn verdict(&mut self, w: &mut World, me: ActorId) {
        let cfg = &self.cfg;
        let Some(run) = &mut self.run else { return };
        Self::destroy_wand(w, run);
        run.passed = run.success >= cfg.pass_marks.get(run.round).copied().unwrap_or(1.0);
        if run.passed {
            if !cfg.incantation.is_empty()
                && let Some((path, _)) = w.lines.voice(&cfg.incantation)
            {
                w.outputs.push(Output::Voice { path });
            }
            w.sound(me, CORRECT_SOUND);
            let hit = Self::pattern_fx(w, run, cfg, &cfg.hit_fx.clone());
            if let Some(s) = system(w, hit) {
                s.settings.width.base = 12.0;
                s.settings.length.base = 12.0;
                s.settings.lifetime.base = 3.0;
            }
            Self::pattern_fx(w, run, cfg, &cfg.win_fx.clone());
            teacher_loop(w, run, "talk_pos");
        } else {
            w.sound(me, INCORRECT_SOUND);
            Self::pattern_fx(w, run, cfg, FAIL_FX);
            teacher_loop(w, run, "talk_neg");
        }
        run.step = Step::Verdict;
        run.wait = 3.0;
    }

    fn say_verdict(&mut self, w: &mut World) {
        let cfg = &self.cfg;
        let Some(run) = &mut self.run else { return };
        if run.passed {
            teacher_loop(w, run, "breathe");
        }
        w.hud.popup = None;
        w.cut_mode = true;
        run.wait = if !run.passed {
            if run.success > cfg.vbad {
                say_one(w, &cfg.judge_failure, 5.0)
            } else {
                say_one(w, &cfg.judge_vbad, 5.0)
            }
        } else if run.success >= 1.0 {
            say(w, &cfg.full_marks, 3.0)
        } else if run.success > cfg.vgood {
            say_one(w, &cfg.judge_vgood, 5.0)
        } else {
            say_one(w, &cfg.judge_success, 5.0)
        };
        run.step = Step::VerdictSaid;
    }

    fn final_popup(&mut self, w: &mut World) {
        let Some(run) = &mut self.run else { return };
        let points = match run.total_points {
            5 => "lesson_text_04",
            15 => "lesson_text_05",
            30 => "lesson_text_06",
            50 => "lesson_text_07",
            _ => "",
        };
        w.hud.popup = Some(Popup::Final {
            title: w.lines.localized("Pickup", "all", "lesson_text_03"),
            rounds: run.round,
            points: if points.is_empty() {
                String::new()
            } else {
                w.lines.localized("Pickup", "all", points)
            },
        });
        run.step = Step::Final;
        run.wait = 5.0;
    }

    fn after_verdict(&mut self, w: &mut World) {
        let cfg = &self.cfg;
        let Some(run) = &mut self.run else { return };
        teacher_loop(w, run, "breathe");
        Self::destroy_wand(w, run);
        remove(w, &mut run.template);
        if !run.passed {
            w.cut_mode = false;
            return if run.round == 0 { self.show(w) } else { self.final_popup(w) };
        }
        run.round += 1;
        let last = run.round > 3;
        if last {
            w.hud.popup = None;
        }
        let points = cfg.house_points.get(run.round - 1).copied().unwrap_or(0);
        w.stats.house_points += points;
        run.total_points += points;
        w.cut_mode = true;
        let line = cfg.points.get(run.round - 1).cloned().unwrap_or_default();
        run.wait = say(w, &line, 5.0);
        run.step = Step::PointsSaid { last };
    }

    /// The lesson is over: everything goes back and the next event is triggered.
    fn end(&mut self, w: &mut World, me: ActorId) {
        let Some(mut run) = self.run.take() else { return };
        self.done = true;
        w.hud.popup = None;
        w.hud.countdown = None;
        w.wand_wanted = false;
        crate::cutscene::release_camera(w);
        crate::puppet::release(w, run.harry);
        Self::destroy_wand(w, &mut run);
        remove(w, &mut run.template);
        remove(w, &mut run.blackboard);
        if let Some(t) = run.teacher {
            crate::puppet::loop_anim(w, t, "breathe");
        }
        if !self.cfg.event.is_empty() {
            let event = self.cfg.event.clone();
            w.trigger_event(&event, Some(me));
        }
    }
}

impl Behaviour for SpellLesson {
    fn trigger(&mut self, w: &mut World, me: ActorId, _instigator: Option<ActorId>) {
        if self.run.is_none() && !self.done {
            self.start(w, me);
        }
    }

    fn tick(&mut self, w: &mut World, me: ActorId, dt: f32) {
        let Some(run) = &mut self.run else { return };
        // A cutscene letting go of Harry or the teacher after starting the lesson doesn't end it.
        for id in std::iter::once(run.harry).chain(run.teacher) {
            if !w.puppet_busy(id) {
                crate::puppet::capture(w, id);
            }
        }
        // The template advances along its pattern (from the template's own tick).
        if let Some(s) = run.template.and_then(|id| w.particles.get_mut(&id)) {
            let p = &mut s.settings.period;
            p.base += p.rand;
            p.rand = if p.base > 1.0 { 0.0 } else { dt / run.template_time };
        }
        match run.step {
            Step::Trace => return self.trace(w, dt),
            Step::Counting | Step::Counted => self.replay(w, dt),
            _ => {}
        }
        let Some(run) = &mut self.run else { return };
        run.wait -= dt;
        if run.wait > 0.0 {
            return;
        }
        match run.step {
            Step::Settle => {
                let look = run.look(&self.cfg);
                crate::puppet::face(w, run.harry, look);
                if let Some(t) = run.teacher {
                    let cam = run.camera();
                    crate::puppet::face(w, t, cam);
                }
                run.template_dist = run.template_dist.max(self.cfg.wand_dist + 5.0);
                self.show(w);
            }
            Step::Show => self.begin_trace(w),
            Step::Trace => {}
            Step::Counting => {
                if run.replay_done {
                    run.step = Step::Counted;
                }
                run.wait = 1.0;
            }
            Step::Counted => self.verdict(w, me),
            Step::Verdict => self.say_verdict(w),
            Step::VerdictSaid => self.after_verdict(w),
            Step::PointsSaid { last } => {
                w.cut_mode = false;
                if last {
                    self.final_popup(w);
                } else {
                    self.show(w);
                }
            }
            Step::Final => self.end(w, me),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::world::{Entity, WandInput};
    use hp1_data::ClassTable;
    use hp1_data::level::LevelFile;

    struct Flag(Rc<Cell<bool>>);
    impl Behaviour for Flag {
        fn trigger(&mut self, _w: &mut World, _me: ActorId, _by: Option<ActorId>) {
            self.0.set(true);
        }
    }

    fn lesson_world() -> (World, ActorId, Rc<Cell<bool>>) {
        let mut w = World::new(LevelFile::default(), ClassTable::default());
        let harry = w.spawn(Entity::pawn("Harry", "HarryPotter.Harry", Vec3::ZERO, 15.0, 42.0, true));
        w.player = Some(harry);
        let points = [Vec2::new(0.1, 0.1), Vec2::new(0.9, 0.1), Vec2::new(0.9, 0.9)];
        w.gestures.insert("Test.Pattern".into(), Pattern { points: points.to_vec() });
        let cfg = Config {
            gesture: "Test.Pattern".into(),
            event: "After".into(),
            draw_time: 6.0,
            fov_ratio: 0.8,
            template_dist: 105.0,
            wand_dist: 100.0,
            accuracy: 0.03,
            pass_marks: vec![0.5, 0.65, 0.8, 0.95],
            house_points: vec![5, 10, 15, 20],
            vgood: 0.8,
            vbad: 0.6,
            ..Default::default()
        };
        let mut e =
            Entity::pawn("Lesson", "HPBase.SpellLearnTrigger", Vec3::new(100.0, 0.0, 0.0), 10.0, 10.0, false);
        (e.is_pawn, e.collide_actors, e.tag) = (false, false, "Lesson".into());
        e.behaviour = Some(Box::new(SpellLesson { cfg, run: None, done: false }));
        w.spawn(e);
        let fired = Rc::new(Cell::new(false));
        let mut after = Entity::pawn("After", "Test.After", Vec3::ZERO, 1.0, 1.0, false);
        (after.is_pawn, after.tag) = (false, "After".into());
        after.behaviour = Some(Box::new(Flag(fired.clone())));
        w.spawn(after);
        (w, harry, fired)
    }

    /// Traces the pattern in `seconds` (as the viewer's automatic tracer does).
    fn trace(w: &mut World, traced: &mut f32, seconds: f32, dt: f32) {
        if !w.wand_wanted {
            *traced = 0.0;
            return;
        }
        let mut input = WandInput { draw: *traced < seconds, ..Default::default() };
        if let Some((at, path)) = &w.wand_at {
            *traced += dt;
            let target = w.gestures[path].along(*traced / seconds);
            input.mouse = (target - *at) / WAND_PER_PIXEL;
        }
        w.wand_input = input;
    }

    #[test]
    fn four_traced_rounds_win_fifty_points_and_end_the_lesson() {
        let (mut w, harry, fired) = lesson_world();
        w.trigger_event("Lesson", None);
        assert!(w.camera.active && w.puppet_busy(harry));
        let (dt, mut traced, mut t) = (1.0 / 60.0, 0.0, 0.0);
        let mut best = 0.0f32;
        while !fired.get() {
            trace(&mut w, &mut traced, 3.0, dt);
            w.tick(dt);
            best = best.max(w.hud.lesson_score.unwrap_or(0.0));
            t += dt;
            assert!(t < 200.0, "the lesson didn't end");
        }
        assert_eq!(best, 1.0);
        assert_eq!(w.stats.house_points, 50);
        assert!(!w.camera.active && !w.puppet_busy(harry) && w.hud.popup.is_none());
        // Spawned pieces are gone: only Harry, the trigger and the flag are left.
        assert_eq!(w.actors.iter().flatten().count(), 3);
    }

    #[test]
    fn not_drawing_fails_round_zero_and_it_starts_again() {
        let (mut w, _, fired) = lesson_world();
        w.trigger_event("Lesson", None);
        let mut traces = 0;
        let mut was_tracing = false;
        for _ in 0..(60 * 60) {
            w.tick(1.0 / 60.0);
            if w.wand_wanted && !was_tracing {
                traces += 1;
            }
            was_tracing = w.wand_wanted;
        }
        assert!(traces >= 2, "{traces}");
        assert_eq!(w.stats.house_points, 0);
        assert!(!fired.get());
    }
}
