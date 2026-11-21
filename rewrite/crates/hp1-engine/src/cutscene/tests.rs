use glam::Vec3Swizzles;

use super::*;
use crate::behaviours::tests::{Placed, s, world_with};
use crate::dialog::Lines;

fn v(x: f32, y: f32, z: f32) -> DataValue {
    DataValue::Vector { x, y, z }
}

fn f(x: f32) -> DataValue {
    DataValue::Float(x as f64)
}

/// A scene that starts with the level: `cast` is (actor name, alias), `scripts` one list of
/// lines per member, `places` (actor name, alias).
fn scene<'a>(cast: &[(&str, &str)], scripts: &[&[&str]], places: &[(&str, &str)]) -> Placed<'a> {
    let mut props = vec![("bLevelLoadStarts", DataValue::Bool(true))];
    let mut owned: Vec<(String, DataValue)> = Vec::new();
    for (i, (actor, alias)) in cast.iter().enumerate() {
        // The maps spell the first entry in lower case.
        let key = if i == 0 { "cast".to_owned() } else { format!("Cast[{i}]") };
        owned.push((format!("{key}.actor"), s(&format!("Test.{actor}"))));
        owned.push((format!("{key}.alias"), s(alias)));
    }
    for (c, lines) in scripts.iter().enumerate() {
        for (l, line) in lines.iter().enumerate() {
            owned.push((indexed(&format!("Cast{c}Script"), l), s(line)));
        }
    }
    for (i, (actor, alias)) in places.iter().enumerate() {
        owned.push((format!("{}.actor", indexed("Locs", i)), s(&format!("Test.{actor}"))));
        owned.push((format!("{}.alias", indexed("Locs", i)), s(alias)));
    }
    // The test world takes &str keys; these live for the whole test.
    for (k, v) in owned {
        props.push((Box::leak(k.into_boxed_str()), v));
    }
    ("Scene", "HPBase.CutScene", props)
}

fn mark<'a>(name: &'a str, at: DataValue) -> Placed<'a> {
    (name, "HPBase.CutMark", vec![("Location", at)])
}

fn dumbledore<'a>(at: DataValue) -> Placed<'a> {
    (
        "Dum",
        "HPBase.baseChar",
        vec![
            ("Location", at),
            ("CollisionRadius", f(22.0)),
            ("CollisionHeight", f(42.0)),
            ("GroundSpeed", f(100.0)),
            ("RotationRate", DataValue::Rotator { pitch: 0, yaw: 50000, roll: 3072 }),
            ("CutIdleAnim", s("Breathe")),
            ("CutTalkAnim", s("talk2")),
        ],
    )
}

/// A music event tagged `bell`, so a script's `TRIGGER bell` shows up in the outputs.
fn bell<'a>() -> Placed<'a> {
    ("Bell", "Engine.MusicEvent", vec![("Tag", s("bell")), ("Song", s("Bell"))])
}

fn rang(w: &World) -> bool {
    w.outputs.iter().any(|o| matches!(o, Output::Music { .. }))
}

fn floor_world(actors: Vec<Placed>) -> World {
    let mut w = world_with(actors);
    w.level.model = crate::collision::tests::floor();
    w
}

fn anim(w: &World, id: ActorId) -> String {
    w.get(id).and_then(|e| e.anim.as_ref()).map_or(String::new(), |a| a.name.clone())
}

#[test]
fn members_run_a_line_per_tick_and_wait_for_cues() {
    let mut w = floor_world(vec![
        scene(&[("A", "a"), ("B", "b")], &[&["WAITFOR Go", "TRIGGER bell"], &["Sleep 0.25", "cue go"]], &[]),
        mark("A", v(0.0, 0.0, 0.0)),
        mark("B", v(0.0, 0.0, 0.0)),
        bell(),
    ]);
    // Tick 1: A waits, B sleeps. B's sleep is over in tick 4, where it gives the cue after A's
    // turn, so A goes on in tick 5.
    for _ in 0..4 {
        w.tick(0.1);
    }
    assert!(!rang(&w));
    w.tick(0.1);
    assert!(rang(&w));
}

#[test]
fn a_cue_given_before_anyone_waits_still_counts() {
    let mut w = floor_world(vec![
        scene(&[("A", "a"), ("B", "b")], &[&["", "", "WAITFOR early", "TRIGGER bell"], &["CUE Early"]], &[]),
        mark("A", v(0.0, 0.0, 0.0)),
        mark("B", v(0.0, 0.0, 0.0)),
        bell(),
    ]);
    for _ in 0..5 {
        w.tick(0.1);
    }
    assert!(rang(&w));
}

#[test]
fn characters_walk_to_a_mark_then_stand_idle_and_the_member_goes_on() {
    let mut w = floor_world(vec![
        scene(&[("Dum", "Dumbledore")], &[&["Capture", "Moveto Spot", "Trigger bell"]], &[("Spot", "spot")]),
        dumbledore(v(0.0, 0.0, 42.0)),
        mark("Spot", v(200.0, 0.0, 60.0)),
        bell(),
    ]);
    let dum = 1;
    for _ in 0..15 {
        w.tick(0.1);
    }
    let e = w.get(dum).unwrap();
    assert!((e.pos.x - 130.0).abs() < 15.0 && e.pos.y.abs() < 1.0, "{:?}", e.pos);
    assert!((e.pos.z - 42.0).abs() < 1.0, "on the floor: {:?}", e.pos);
    assert_eq!(anim(&w, dum), "Walk");
    assert!(w.puppet_busy(dum));
    assert!(!rang(&w));
    for _ in 0..10 {
        w.tick(0.1);
    }
    let e = w.get(dum).unwrap();
    assert!((e.pos.xy() - glam::Vec2::new(200.0, 0.0)).length() < 1.0, "{:?}", e.pos);
    assert_eq!(anim(&w, dum), "Breathe");
    assert!(rang(&w));
}

#[test]
fn turning_finishes_when_nearly_facing_the_target() {
    let mut w = floor_world(vec![
        scene(&[("Dum", "Dumbledore")], &[&["Turnto Spot", "Trigger bell"]], &[("Spot", "spot")]),
        dumbledore(v(0.0, 0.0, 42.0)),
        mark("Spot", v(0.0, 200.0, 42.0)),
        bell(),
    ]);
    // A quarter turn at 50000 units/s takes about a third of a second.
    for _ in 0..3 {
        w.tick(0.1);
    }
    assert!(!rang(&w));
    for _ in 0..3 {
        w.tick(0.1);
    }
    assert!(rang(&w));
    let yaw = w.get(1).unwrap().rot[1];
    assert!((yaw - 16384).abs() < 2000, "{yaw}");
}

struct ShortLines;
impl Lines for ShortLines {
    fn text(&self, id: &str) -> String {
        id.to_owned()
    }
    fn voice(&mut self, id: &str) -> Option<(String, f32)> {
        Some((format!("AllDialog.{id}"), 1.45))
    }
}

#[test]
fn talking_waits_for_the_line_and_animates_the_speaker() {
    let mut w = floor_world(vec![
        scene(
            &[("Dum", "Dumbledore"), ("Spot", "x")],
            &[&["Capture"], &["Talk0 hello", "Trigger bell"]],
            &[],
        ),
        dumbledore(v(0.0, 0.0, 42.0)),
        mark("Spot", v(0.0, 0.0, 0.0)),
        bell(),
    ]);
    w.lines = Box::new(ShortLines);
    w.tick(0.1);
    assert!(w.outputs.contains(&Output::Voice { path: "AllDialog.HELLO".into() }));
    w.tick(0.1);
    assert_eq!(anim(&w, 1), "talk2");
    // The line lasts 1.45 + 0.5 s, so the next line runs in tick 21.
    for _ in 0..18 {
        w.tick(0.1);
    }
    assert!(!rang(&w));
    w.tick(0.1);
    assert!(rang(&w));
    for _ in 0..5 {
        w.tick(0.1);
    }
    assert_eq!(anim(&w, 1), "Breathe");
}

#[test]
fn the_camera_flies_to_its_spot_looking_at_the_preface_target() {
    let mut w = floor_world(vec![
        scene(
            &[("Cam", "cam")],
            &[&["Capture", "Preface Target", "Moveto A", "Trigger bell"]],
            &[("A", "a"), ("Target", "target")],
        ),
        ("Cam", "HPBase.BaseCam", vec![("Location", v(0.0, 0.0, 100.0))]),
        mark("A", v(1000.0, 0.0, 100.0)),
        mark("Target", v(1000.0, 500.0, 100.0)),
        bell(),
    ]);
    w.tick(0.1);
    assert!(w.camera.active);
    // Each tick it covers 2 × 0.1 of the way left; within 15 units after about 19 ticks.
    for _ in 0..17 {
        w.tick(0.1);
    }
    assert!(!rang(&w), "{:?}", w.camera.pos);
    for _ in 0..5 {
        w.tick(0.1);
    }
    assert!(rang(&w), "{:?}", w.camera.pos);
    assert!((w.camera.rot[1] - 16384).abs() < 300, "{:?}", w.camera.rot);
    // It doesn't stop at 15: it keeps closing in on the spot.
    assert!(w.camera.pos.distance(Vec3::new(1000.0, 0.0, 100.0)) < 10.0);
}

#[test]
fn releasing_the_camera_gives_back_its_settings_and_leaves_cutscene_mode() {
    let mut w = floor_world(vec![
        scene(&[("Cam", "cam")], &[&["Capture", "CamSpeed 0.5 0.25", "CamProx 40", "Release"]], &[]),
        ("Cam", "HPBase.BaseCam", vec![("Location", v(0.0, 0.0, 100.0))]),
    ]);
    for _ in 0..3 {
        w.tick(0.1);
    }
    assert!(w.camera.active);
    assert_eq!(w.camera.speed, 1.0);
    assert_eq!(w.camera.rate, [5000.0; 3]);
    assert_eq!(w.camera.prox, 40.0);
    w.tick(0.1);
    assert!(!w.camera.active && w.camera.left);
    assert_eq!((w.camera.speed, w.camera.rate, w.camera.prox), (NORMAL_SPEED, [20000.0; 3], NORMAL_PROX));
}

#[test]
fn capturing_harry_starts_cutscene_mode_until_released() {
    let mut w = floor_world(vec![
        scene(&[("harry0", "Harry")], &[&["Capture", "Sleep 0.5", "Release"]], &[]),
        (
            "harry0",
            "HPBase.baseHarry",
            vec![("Location", v(0.0, 0.0, 60.0)), ("CollisionRadius", f(15.0)), ("CollisionHeight", f(42.0))],
        ),
    ]);
    w.player = w.placed_harry();
    w.tick(0.1);
    assert!(w.cut_mode && w.puppet_busy(1));
    // Dropped onto the floor, playing breath.
    assert!((w.get(1).unwrap().pos.z - 42.0).abs() < 1.0);
    assert_eq!(anim(&w, 1), "breath");
    for _ in 0..7 {
        w.tick(0.1);
    }
    assert!(!w.cut_mode && !w.puppet_busy(1));
    assert!(w.take_restart(1));
}

#[test]
fn targets_are_found_by_alias_place_and_number() {
    let mut w = floor_world(vec![
        scene(&[("Dum", "Dumbledore")], &[&[]], &[("A", "first"), ("B", "")]),
        dumbledore(v(0.0, 0.0, 42.0)),
        mark("A", v(1.0, 0.0, 0.0)),
        mark("B", v(2.0, 0.0, 0.0)),
    ]);
    w.player = Some(1);
    let mut cs = CutScene::new(&ActorProps::new(w.level.actors[0].as_ref().unwrap(), &w.classes));
    cs.resolve(&w);
    assert_eq!(cs.lookup(&mut w, "HARRY"), Some(1));
    assert_eq!(cs.lookup(&mut w, "dumbledore"), Some(1));
    assert_eq!(cs.lookup(&mut w, "FIRST"), Some(2));
    assert_eq!(cs.lookup(&mut w, "CAST0"), Some(1));
    assert_eq!(cs.lookup(&mut w, "LOC1"), Some(3));
    assert_eq!(cs.lookup(&mut w, "nobody"), None);
    assert!(matches!(w.outputs.last(), Some(Output::Note(_))));
}

#[test]
fn reading_numbers_like_the_original() {
    assert_eq!(leading_int("12ab"), 12);
    assert_eq!(leading_int(" 3 11"), 3);
    assert_eq!(leading_int("x"), 0);
    assert_eq!(leading_float("0.4"), 0.4);
    assert_eq!(leading_float("2.5s"), 2.5);
    assert_eq!(between("(Talk)<Hello>[go]", '<', '>'), Some("Hello"));
    assert_eq!(between(">x<", '<', '>'), None);
}

#[test]
fn a_patrolling_character_holds_still_for_the_scene_and_walks_on_when_released() {
    let mut w = floor_world(vec![
        scene(&[("Kid", "kid")], &[&["Capture", "Animate wave", "Sleep 1.0", "Release"]], &[]),
        (
            "Kid",
            "HPBase.baseChar",
            vec![
                ("Location", v(0.0, 0.0, 43.0)),
                ("CollisionRadius", f(22.0)),
                ("CollisionHeight", f(42.0)),
                ("GroundSpeed", f(100.0)),
                ("bFollowPatrolPoints", DataValue::Bool(true)),
                ("firstPatrolPointObjectName", s("P0")),
                ("walkAnimName", s("run")),
            ],
        ),
        ("P0", "Engine.PatrolPoint", vec![("Location", v(300.0, 0.0, 50.0))]),
    ]);
    let kid = 1;
    // Capture alone changes nothing; the animation takes over.
    w.tick(0.1);
    w.tick(0.1);
    assert!(w.puppet_busy(kid));
    let held = w.get(kid).unwrap().pos;
    for _ in 0..8 {
        w.tick(0.1);
    }
    assert!(w.get(kid).unwrap().pos.distance(held) < 1.0, "stands still while held");
    for _ in 0..40 {
        w.tick(0.1);
    }
    assert!(!w.puppet_busy(kid));
    assert_eq!(anim(&w, kid), "run");
    assert!(w.get(kid).unwrap().pos.x > held.x + 100.0, "walks on: {:?}", w.get(kid).unwrap().pos);
}

#[test]
fn an_emote_turns_to_harry_plays_its_animations_and_turns_back() {
    let mut dum = dumbledore(v(0.0, 0.0, 42.0));
    dum.2.extend([
        ("idleAnimName", s("Breathe")),
        ("Emotes.name", s("Wave")),
        ("Emotes.start", s("wave_start")),
        ("Emotes.loop", s("wave")),
        ("Emotes.stop", s("wave_stop")),
    ]);
    let mut w = floor_world(vec![
        scene(&[("Dum", "Dumbledore")], &[&["Capture", "Emote0 <hello> (wave) [bell]"]], &[]),
        dum,
        ("Harry", "HPBase.baseHarry", vec![("Location", v(-200.0, 0.0, 42.0))]),
        bell(),
    ]);
    w.player = Some(2);
    w.lines = Box::new(ShortLines);
    w.tick(0.1);
    w.tick(0.1);
    assert!(rang(&w));
    assert!(puppet::is_emoting(&w, 1));
    assert_eq!(anim(&w, 1), "wave_start");
    // Harry is straight behind: half a turn at 50000 a second, then the loop animation.
    for _ in 0..8 {
        w.tick(0.1);
    }
    assert_eq!(anim(&w, 1), "wave");
    let yaw = w.get(1).unwrap().rot[1];
    assert!((yaw - 32768).abs() < 2000, "{yaw}");
    // The line lasts 1.45 + 0.5 s; then the stop animation and back to standing, facing ahead.
    let mut seen_stop = false;
    for _ in 0..20 {
        w.tick(0.1);
        seen_stop |= anim(&w, 1) == "wave_stop";
    }
    assert!(seen_stop);
    assert!(!puppet::is_emoting(&w, 1));
    assert_eq!(anim(&w, 1), "Breathe");
    let yaw = w.get(1).unwrap().rot[1];
    assert!(!(2000..=63536).contains(&yaw), "{yaw}");
}
