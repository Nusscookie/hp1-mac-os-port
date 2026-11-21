//! Developer tools for the HP1 rewrite.
//!
//!   hp1-tools info <package>...            header and export counts per class
//!   hp1-tools exports <package> [class]    list exports, optionally of one class
//!   hp1-tools props <package> <object>     an object's property values
//!   hp1-tools check-data <game folder>     read every package and object property list
//!   hp1-tools convert <game folder> <out>  write the game in the rewrite's own formats
//!   hp1-tools render-mesh <converted> <mesh> <out.png> [sequence] [0..1]   software preview
//!   hp1-tools light-maps <converted> <map> <out.png>   a level's light maps side by side
//!   hp1-tools actors <converted> <map> [filter]          placed objects with class and position
//!   hp1-tools fire <converted> <texture> <out.png>       a fire texture after 1, 2 and 4 seconds
//!   hp1-tools check-audio <converted>                   decode every sound and music file
//!   hp1-tools walk <converted> <map> x,y,z x,y,z [radius] a character walking from A to B
//!   hp1-tools patrol <converted> <map>                  patrol routes as the level links them
//!   hp1-tools cutscenes <converted> <map> [filter]      plays each cutscene Harry walks into

mod preview;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use ue1_read::{ObjRef, Package, Reader, Value, read_properties};

const USAGE: &str = "usage:
  hp1-tools info <package>...
  hp1-tools exports <package> [class]
  hp1-tools props <package> <object>
  hp1-tools check-data <game folder>
  hp1-tools convert <game folder> <output folder>
  hp1-tools render-mesh <converted folder> <mesh> <out.png> [sequence] [time 0..1]
  hp1-tools light-maps <converted folder> <map> <out.png>
  hp1-tools actors <converted folder> <map> [name or class filter] [--props]
  hp1-tools fire <converted folder> <texture> <out.png>
  hp1-tools check-audio <converted folder>
  hp1-tools collide <converted folder> <map>
  hp1-tools walk <converted folder> <map> <from x,y,z> <to x,y,z> [radius]
  hp1-tools patrol <converted folder> <map>
  hp1-tools cutscenes <converted folder> <map> [cutscene name]
  hp1-tools sequences <converted folder> <anim file, e.g. anims/HarryPotter/skharryAnims.anim>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("info") if args.len() > 1 => cmd_info(&args[1..]),
        Some("exports") if args.len() > 1 => cmd_exports(&args[1], args.get(2).map(String::as_str)),
        Some("props") if args.len() == 3 => cmd_props(&args[1], &args[2]),
        Some("check-data") if args.len() == 2 => cmd_check_data(Path::new(&args[1])),
        Some("convert") if args.len() == 3 => cmd_convert(Path::new(&args[1]), Path::new(&args[2])),
        Some("render-mesh") if (4..=6).contains(&args.len()) => preview::render_mesh(
            Path::new(&args[1]),
            &args[2],
            Path::new(&args[3]),
            args.get(4).map(String::as_str),
            args.get(5).and_then(|t| t.parse().ok()).unwrap_or(0.0),
        )
        .map(|_| ExitCode::SUCCESS),
        Some("light-maps") if args.len() == 4 => {
            preview::light_maps(Path::new(&args[1]), &args[2], Path::new(&args[3])).map(|_| ExitCode::SUCCESS)
        }
        Some("actors") if (3..=5).contains(&args.len()) => cmd_actors(
            Path::new(&args[1]),
            &args[2],
            args.get(3).map(String::as_str),
            args.get(4).is_some_and(|a| a == "--props"),
        ),
        Some("fire") if args.len() == 4 => {
            preview::fire(Path::new(&args[1]), &args[2], Path::new(&args[3])).map(|_| ExitCode::SUCCESS)
        }
        Some("check-audio") if args.len() == 2 => cmd_check_audio(Path::new(&args[1])),
        Some("cutscenes") if (3..=4).contains(&args.len()) => {
            cmd_cutscenes(Path::new(&args[1]), &args[2], args.get(3).map(String::as_str))
        }
        Some("collide") if args.len() == 3 => cmd_collide(Path::new(&args[1]), &args[2]),
        Some("patrol") if args.len() == 3 => cmd_patrol(Path::new(&args[1]), &args[2]),
        Some("walk") if (5..=6).contains(&args.len()) => cmd_walk(
            Path::new(&args[1]),
            &args[2],
            &args[3],
            &args[4],
            args.get(5).and_then(|r| r.parse().ok()).unwrap_or(22.0),
        ),
        Some("sequences") if args.len() == 3 => cmd_sequences(&Path::new(&args[1]).join(&args[2])),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_info(paths: &[String]) -> Result<ExitCode> {
    for p in paths {
        let pkg = Package::open(p)?;
        println!(
            "{}: version={} licensee={} flags={:#x} names={} imports={} exports={}",
            pkg.name,
            pkg.version,
            pkg.licensee,
            pkg.flags,
            pkg.names.len(),
            pkg.imports.len(),
            pkg.exports.len()
        );
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for e in &pkg.exports {
            *kinds.entry(pkg.class_name(e)).or_default() += 1;
        }
        let mut kinds: Vec<_> = kinds.into_iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        for (k, n) in kinds {
            println!("  {n:6} {k}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_exports(path: &str, class: Option<&str>) -> Result<ExitCode> {
    let pkg = Package::open(path)?;
    for (i, e) in pkg.exports.iter().enumerate() {
        let cls = pkg.class_name(e);
        if class.is_some_and(|c| !c.eq_ignore_ascii_case(cls)) {
            continue;
        }
        let outer = pkg.ref_name(e.outer).unwrap_or("");
        println!("{:6} {cls:20} {outer:24} {:40} size={}", i + 1, pkg.name(e.name), e.size);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_props(path: &str, object: &str) -> Result<ExitCode> {
    let pkg = Package::open(path)?;
    let idx = pkg.find_export(object, None).with_context(|| format!("no export named {object}"))?;
    let e = &pkg.exports[idx];
    if pkg.class_name(e) == "Class" {
        let c = ue1_read::class::read_class(&pkg, idx)?;
        println!(
            "class {} extends {} (config {}){}",
            c.name,
            pkg.ref_path(c.parent),
            c.config_name,
            if c.located_from_end { ", defaults located from the end" } else { "" }
        );
        for p in c.defaults.iter() {
            let index = if p.array_index > 0 { format!("[{}]", p.array_index) } else { String::new() };
            println!("  {}{index} = {}", p.name, show_value(&pkg, &p.value));
        }
        return Ok(ExitCode::SUCCESS);
    }
    if pkg.is_field_export(e) {
        bail!("{object} is a {} (a script definition), not an object with properties", pkg.class_name(e));
    }
    let data = pkg.export_data(e);
    let mut r = Reader::new(data);
    println!("{} ({}), {} bytes", pkg.ref_path(ObjRef::Export(idx)), pkg.class_name(e), e.size);
    if let Some(frame) = pkg.read_state_frame(e, &mut r)? {
        println!("  state: {}", pkg.ref_name(frame.state).unwrap_or("-"));
    }
    for p in read_properties(&pkg, &mut r)? {
        let index = if p.array_index > 0 { format!("[{}]", p.array_index) } else { String::new() };
        println!("  {}{index} = {}", p.name, show_value(&pkg, &p.value));
    }
    println!("  ({} bytes of object data follow the properties)", r.remaining());
    Ok(ExitCode::SUCCESS)
}

fn show_value(pkg: &Package, v: &Value) -> String {
    match v {
        Value::Object(r) if r.is_none() => "None".into(),
        Value::Object(r) => pkg.ref_path(*r),
        Value::Name(n) => n.clone(),
        Value::Str(s) => format!("{s:?}"),
        Value::Struct(name, raw) => format!("{name} ({} bytes)", raw.len()),
        Value::Raw(raw) => format!("<{} bytes>", raw.len()),
        other => format!("{other:?}"),
    }
}

fn find_packages(game: &Path) -> Result<Vec<PathBuf>> {
    let out = ue1_read::package_files(game)?;
    if out.is_empty() {
        bail!("no Unreal packages found under {}", game.display());
    }
    Ok(out)
}

/// Converts the installed game into the rewrite's own formats.
fn cmd_convert(game: &Path, out: &Path) -> Result<ExitCode> {
    let start = std::time::Instant::now();
    let conv = hp1_convert::Converter::new(game, out).map_err(|e| anyhow::anyhow!("{e}"))?;
    let manifest = conv.convert_all().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut errors = 0;
    for (name, section) in &manifest.sections {
        println!(
            "{name}: {} converted, {} errors, {} warnings",
            section.items,
            section.errors.len(),
            section.warnings.len()
        );
        for e in section.errors.iter().take(10) {
            println!("    {e}");
        }
        errors += section.errors.len();
    }
    println!("written to {} in {:.1}s", out.display(), start.elapsed().as_secs_f32());
    Ok(if errors == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// Opens every package and reads the property list of every ordinary object. Script
/// definitions (classes, functions, states, …) are skipped: the rewrite never uses script code.
fn cmd_check_data(game: &Path) -> Result<ExitCode> {
    let paths = find_packages(game)?;
    let (mut objects, mut props, mut failures) = (0usize, 0usize, 0usize);
    let mut failed_packages = 0usize;
    let (mut classes, mut from_end) = (0usize, 0usize);
    let mut from_end_names = Vec::new();
    for path in &paths {
        let pkg = match Package::open(path) {
            Ok(p) => p,
            Err(e) => {
                println!("FAIL {}: {e}", path.display());
                failed_packages += 1;
                continue;
            }
        };
        let mut pkg_failures = Vec::new();
        for (i, e) in pkg.exports.iter().enumerate() {
            if pkg.class_name(e) == "Class" {
                classes += 1;
                match ue1_read::class::read_class(&pkg, i) {
                    Ok(c) if c.located_from_end => {
                        from_end += 1;
                        from_end_names.push(c.name);
                    }
                    Ok(_) => {}
                    Err(err) => pkg_failures.push(format!("class {}: {err}", pkg.name(e.name))),
                }
                continue;
            }
            if e.size == 0 || pkg.is_field_export(e) {
                continue;
            }
            objects += 1;
            let mut r = Reader::new(pkg.export_data(e));
            let res = pkg.read_state_frame(e, &mut r).and_then(|_| read_properties(&pkg, &mut r));
            match res {
                Ok(list) => props += list.len(),
                Err(err) => pkg_failures.push(format!(
                    "{} ({}): {err}",
                    pkg.ref_path(ObjRef::Export(i)),
                    pkg.class_name(e)
                )),
            }
        }
        if !pkg_failures.is_empty() {
            failed_packages += 1;
            failures += pkg_failures.len();
            println!("FAIL {}: {} objects", pkg.name, pkg_failures.len());
            for f in pkg_failures.iter().take(5) {
                println!("     {f}");
            }
        }
    }
    println!(
        "{} packages, {objects} objects, {props} properties; {failed_packages} packages with errors, {failures} objects failed",
        paths.len()
    );
    println!("{classes} classes read, {from_end} with class-level code (defaults located from the end)");
    if !from_end_names.is_empty() {
        println!("    {}", from_end_names.join(", "));
    }
    Ok(if failed_packages == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// Drops a Harry-sized box from every player start and every 50th placed object, traces lines
/// outwards from the first start and walks Harry from there: a quick check of collision on a map.
/// Every patrol point with where it leads, as linked when the level starts.
fn cmd_patrol(data: &Path, map: &str) -> Result<ExitCode> {
    let level: hp1_data::level::LevelFile =
        hp1_data::mesh::load_binary(&data.join("levels").join(format!("{map}.level")))?;
    let classes: hp1_data::ClassTable = serde_json::from_slice(&std::fs::read(data.join("classes.json"))?)?;
    let world = hp1_engine::world::World::new(level, classes);
    let net = hp1_engine::patrol::PatrolNet::build(&world);
    let name = |i: Option<usize>| i.map_or("-".to_owned(), |i| net.points[i].name.clone());
    for p in &net.points {
        let mut notes = Vec::new();
        if p.pause > 0.0 {
            notes.push(format!("pause {:.1}", p.pause));
        }
        if !p.event.is_empty() {
            notes.push(format!("event {}", p.event));
        }
        if p.destroy {
            notes.push("removes".to_owned());
        }
        println!(
            "{:16} {:6.0} {:6.0} {:6.0}  next {:16} prev {:16} {}",
            p.name,
            p.pos.x,
            p.pos.y,
            p.pos.z,
            name(p.next),
            name(p.prev),
            notes.join(", ")
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn parse_point(s: &str) -> Result<glam::Vec3> {
    let n: Vec<f32> = s.split(',').map(|v| v.trim().parse()).collect::<Result<_, _>>()?;
    let [x, y, z] = n[..] else { bail!("want x,y,z, got {s}") };
    Ok(glam::Vec3::new(x, y, z))
}

/// Walks a character (height 42) straight from one point to another for up to 10 seconds and
/// prints where it gets, plus the floor height under the line between the points.
fn cmd_walk(data: &Path, map: &str, from: &str, to: &str, radius: f32) -> Result<ExitCode> {
    use glam::{Vec3, Vec3Swizzles};
    use hp1_engine::collision::Collider;
    let (from, to) = (parse_point(from)?, parse_point(to)?);
    let level: hp1_data::level::LevelFile =
        hp1_data::mesh::load_binary(&data.join("levels").join(format!("{map}.level")))?;
    let classes: hp1_data::ClassTable = serde_json::from_slice(&std::fs::read(data.join("classes.json"))?)?;
    let world = hp1_engine::world::World::new(level, classes);
    let c = world.collider(None);
    let extent = Vec3::new(radius, radius, 42.0);
    println!("floor under the line (feet height):");
    for i in 0..=20 {
        let p = from.lerp(to, i as f32 / 20.0);
        let top = Vec3::new(p.x, p.y, from.z.max(to.z) + 200.0);
        let floor = c.trace(top, top - Vec3::Z * 1000.0, extent).map(|h| top.z - 1000.0 * h.time - 42.0);
        println!("  {:6.0} {:6.0}  {}", p.x, p.y, floor.map_or("none".into(), |z| format!("{z:7.1}")));
    }
    let mut body = hp1_engine::walk::Body::new(from, radius, 42.0);
    body.settle(&c);
    let m = hp1_engine::walk::Movement::default();
    for step in 0..600 {
        let dir = (to - body.pos).xy();
        if dir.length() < 2.0 {
            println!("arrived after {:.2} s at {:.1}", step as f32 / 60.0, body.pos);
            return Ok(ExitCode::SUCCESS);
        }
        let vel = dir.normalize() * m.ground_speed.min(dir.length() * 60.0);
        body.drive(&c, &m, vel, 1.0 / 60.0);
        if step % 15 == 0 {
            println!("{:5.2} s  {:.1}  {:?}", step as f32 / 60.0, body.pos, body.mode);
        }
    }
    println!("did not arrive; stopped at {:.1}", body.pos);
    let ahead = (to - body.pos).xy().normalize_or_zero().extend(0.0) * 5.0;
    let n = Vec3::new(0.0, 0.447, 0.894);
    let slide = ahead - n * ahead.dot(n);
    for (what, d) in [
        ("ahead", ahead),
        ("up 25", Vec3::Z * 25.0),
        ("down 5", Vec3::Z * -5.0),
        ("slide", slide),
        ("slide+up", slide + Vec3::Z * 0.5),
    ] {
        match c.trace(body.pos, body.pos + d, extent) {
            Some(h) => println!(
                "  {what}: hit at {:.3} normal {:.3} node {} actor {:?}",
                h.time, h.normal, h.node, h.actor
            ),
            None => println!("  {what}: free"),
        }
    }
    Ok(ExitCode::FAILURE)
}

fn cmd_collide(data: &Path, map: &str) -> Result<ExitCode> {
    use glam::Vec3;
    use hp1_engine::collision::trace_model as trace;
    let level: hp1_data::level::LevelFile =
        hp1_data::mesh::load_binary(&data.join("levels").join(format!("{map}.level")))?;
    let model = &level.model;
    let hulls = model.nodes.iter().filter(|n| n.collision_bound >= 0).count();
    println!("{} nodes, {hulls} with collision hulls", model.nodes.len());
    let extent = Vec3::new(16.0, 16.0, 40.0);
    let t = std::time::Instant::now();
    let mut traces = 0;
    let mut first = None;
    for (i, a) in level.actors.iter().enumerate() {
        let Some(a) = a else { continue };
        let start = a.class.ends_with(".PlayerStart");
        if !start && i % 50 != 0 {
            continue;
        }
        let Some(loc) = a.props.get("Location").and_then(hp1_data::DataValue::as_vec3) else { continue };
        let from = Vec3::from(loc);
        let to = from - Vec3::Z * 2000.0;
        traces += 1;
        match trace(model, from, to, extent) {
            Some(h) => {
                let p = from.lerp(to, h.time);
                println!(
                    "{:28} from z {:7.1} lands on z {:7.1} (feet {:7.1}) normal {:5.2} {:5.2} {:5.2}",
                    a.name,
                    from.z,
                    p.z,
                    p.z - extent.z,
                    h.normal.x,
                    h.normal.y,
                    h.normal.z
                );
            }
            None => println!("{:28} from z {:7.1} falls through", a.name, from.z),
        }
        if start && first.is_none() {
            first = Some(from);
        }
    }
    if let Some(from) = first {
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let to = from + Vec3::new(a.cos(), a.sin(), 0.0) * 5000.0;
            traces += 2;
            let line = trace(model, from, to, Vec3::ZERO).map(|h| h.time * 5000.0);
            let boxed = trace(model, from, to, extent).map(|h| h.time * 5000.0);
            println!("direction {:3}°: line hits at {line:?}, box at {boxed:?}", k * 45);
        }
    }
    if let Some(from) = first {
        use hp1_engine::walk::{Body, Mode, Movement};
        let m = Movement::default();
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let mut b = Body::new(from, 15.0, 42.0);
            b.settle(model);
            let settled = b.pos;
            let mut falls = 0;
            for _ in 0..180 {
                b.tick(model, &m, Vec3::new(a.cos(), a.sin(), 0.0), false, 1.0 / 60.0);
                falls += (b.mode == Mode::Falling) as u32;
            }
            println!(
                "walk {:3}° for 3 s from {:?}: ended at {:6.0} {:6.0} {:6.1}, moved {:5.0}, {falls} ticks falling",
                k * 45,
                settled.round(),
                b.pos.x,
                b.pos.y,
                b.pos.z,
                (b.pos - settled).length()
            );
        }
    }
    {
        let classes: hp1_data::ClassTable =
            serde_json::from_slice(&std::fs::read(data.join("classes.json"))?)?;
        let g = hp1_engine::nav::NavGraph::new(&level, &classes);
        let mut groups = g.groups();
        groups.sort_unstable_by(|a, b| b.cmp(a));
        println!(
            "{} navigation points, {} links, {} linked groups (largest {:?})",
            g.len(),
            g.link_count(),
            groups.len(),
            &groups[..groups.len().min(5)]
        );
    }
    println!("{traces} traces in {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);
    Ok(ExitCode::SUCCESS)
}

fn cmd_sequences(file: &Path) -> Result<ExitCode> {
    let anim: hp1_data::mesh::AnimFile = hp1_data::mesh::load_binary(file)?;
    for s in &anim.sequences {
        println!(
            "{:24} {:4} frames {:5.1} fps {:6.2} s",
            s.info.name, s.info.frames, s.info.rate, s.duration
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_actors(data: &Path, map: &str, filter: Option<&str>, props: bool) -> Result<ExitCode> {
    let level: hp1_data::level::LevelFile =
        hp1_data::mesh::load_binary(&data.join("levels").join(format!("{map}.level")))?;
    let filter = filter.map(str::to_ascii_lowercase);
    for (i, a) in level.actors.iter().enumerate() {
        let Some(a) = a else { continue };
        if let Some(f) = &filter
            && !a.name.to_ascii_lowercase().contains(f.as_str())
            && !a.class.to_ascii_lowercase().contains(f.as_str())
        {
            continue;
        }
        let loc = a.props.get("Location").and_then(hp1_data::DataValue::as_vec3).unwrap_or([0.0; 3]);
        let seq = a.props.get("AnimSequence").and_then(hp1_data::DataValue::as_str).unwrap_or("");
        println!("{i:5} {:28} {:32} {:8.0} {:8.0} {:8.0} {seq}", a.name, a.class, loc[0], loc[1], loc[2]);
        if props {
            if let Some(b) = a.brush.as_ref().and_then(|k| level.brushes.get(k)) {
                let m = &b.model;
                println!(
                    "        brush: {} polys, {} nodes, {} surfaces, {} light maps, {} hull ints",
                    b.polys.len(),
                    m.nodes.len(),
                    m.surfaces.len(),
                    m.light_maps.len(),
                    m.leaf_hulls.len()
                );
            }
            for (k, v) in &a.props {
                println!("        {k} = {}", serde_json::to_string(v)?);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_check_audio(data: &Path) -> Result<ExitCode> {
    let mut failed = 0;
    for section in ["sounds", "music"] {
        let (mut ok, mut seconds) = (0, 0.0f64);
        for e in std::fs::read_dir(data.join(section))? {
            let p = e?.path();
            if p.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let list: Vec<hp1_data::AudioInfo> = serde_json::from_slice(&std::fs::read(&p)?)?;
            for a in list {
                let bytes = std::fs::read(data.join(&a.file))?;
                match hp1_audio::decode(bytes, &a.format) {
                    Ok(clip) if clip.frames() > 0 => {
                        ok += 1;
                        seconds += clip.seconds() as f64;
                    }
                    Ok(_) => {
                        failed += 1;
                        println!("{}: no samples", a.path);
                    }
                    Err(err) => {
                        failed += 1;
                        println!("{}: {err}", a.path);
                    }
                }
            }
        }
        println!("{section}: {ok} decoded, {:.0} minutes", seconds / 60.0);
    }
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// Dialogue text without voices (every line then lasts 3 s).
struct TextLines(std::collections::HashMap<String, String>);

impl hp1_engine::dialog::Lines for TextLines {
    fn text(&self, id: &str) -> String {
        self.0.get(&id.to_ascii_lowercase()).cloned().unwrap_or_default()
    }
    fn voice(&mut self, _id: &str) -> Option<(String, f32)> {
        None
    }
}

fn text_lines(data: &Path) -> TextLines {
    let read = |name: &str| -> Option<serde_json::Value> {
        serde_json::from_slice(&std::fs::read(data.join("text").join(name)).ok()?).ok()
    };
    let lang = read("default.ini.json")
        .and_then(|v| v["Engine.Engine"]["Language"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "int".into());
    let all = read(&format!("hpdialog.{lang}.json")).or_else(|| read("hpdialog.int.json"));
    TextLines(
        all.and_then(|v| v.get("all").and_then(|a| a.as_object()).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.as_str()?.to_owned())))
            .collect(),
    )
}

/// For each cutscene that starts when Harry walks into it: a fresh level, the intro played out,
/// then Harry put inside the scene and the scene played for up to two minutes.
fn cmd_cutscenes(data: &Path, map: &str, filter: Option<&str>) -> Result<ExitCode> {
    use hp1_engine::world::{Output, World};
    let level: hp1_data::level::LevelFile =
        hp1_data::mesh::load_binary(&data.join("levels").join(format!("{map}.level")))?;
    let classes: hp1_data::ClassTable = serde_json::from_slice(&std::fs::read(data.join("classes.json"))?)?;
    let fresh = || {
        let mut w = World::new(level.clone(), classes.clone());
        w.player = w.placed_harry();
        if let Some(e) = w.player.and_then(|h| w.get_mut(h)) {
            (e.is_pawn, e.is_player, e.collide_actors) = (true, true, true);
        }
        w.lines = Box::new(text_lines(data));
        w
    };
    const DT: f32 = 1.0 / 30.0;
    // Plays until cutscene mode has been on and is off again (or `limit` seconds).
    let trace = std::env::var_os("HP1_TRACE_CUTSCENES").is_some();
    // (cutscene mode came on, when it went off again, lines the scene ran)
    let play = |w: &mut World, limit: f32, report: bool| -> (bool, Option<f32>, usize) {
        let (mut started, mut t, mut lines) = (false, 0.0, 0);
        while t < limit {
            w.tick(DT);
            t += DT;
            for o in w.outputs.drain(..) {
                match o {
                    Output::Message { text, .. } if report => println!("    {t:5.1} s  \"{text}\""),
                    Output::Note(n) if report && n.contains(" member ") => {
                        lines += 1;
                        if trace {
                            println!("    {t:5.1} s  {n}");
                        }
                    }
                    Output::Note(n) if report => println!("    {t:5.1} s  note: {n}"),
                    Output::ChangeLevel { map } if report => println!("    {t:5.1} s  go to level {map}"),
                    _ => {}
                }
            }
            started |= w.cut_mode;
            if started && !w.cut_mode {
                return (true, Some(t), lines);
            }
        }
        (started, None, lines)
    };
    // (object, name, place, started by: None = level start, Some("") = walking in, Some(tag))
    let scenes: Vec<(usize, String, glam::Vec3, Option<String>)> = {
        let w = World::new(level.clone(), classes.clone());
        level
            .actors
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let a = a.as_ref()?;
                if !a.class.eq_ignore_ascii_case("HPBase.CutScene") {
                    return None;
                }
                let p = hp1_engine::props::ActorProps::new(a, &w.classes);
                let start = if p.bool("bLevelLoadStarts") {
                    None
                } else if p.bool_or("bTouchStarts", true) {
                    Some(String::new())
                } else {
                    Some(w.get(i)?.tag.clone())
                };
                Some((i, a.name.clone(), w.get(i)?.pos, start))
            })
            .collect()
    };
    for (id, name, pos, start) in scenes {
        if filter.is_some_and(|f| !name.eq_ignore_ascii_case(f)) {
            continue;
        }
        let Some(tag) = start else {
            println!("{name}: starts with the level");
            continue;
        };
        let mut w = fresh();
        play(&mut w, 120.0, false);
        // Let the rest of the intro (camera, Dumbledore) finish too.
        for _ in 0..(10.0 / DT) as usize {
            w.tick(DT);
        }
        w.outputs.clear();
        w.trace_cutscenes = true;
        let Some(harry) = w.player else { break };
        // Triggered scenes start near Harry in the game too.
        if let Some(e) = w.get_mut(harry) {
            e.pos = pos;
        }
        if tag.is_empty() {
            println!("{name} (object {id}) at {pos:.0}, Harry walks in:");
        } else {
            w.trigger_event(&tag, None);
            println!("{name} (object {id}) at {pos:.0}, triggered by {tag}:");
        }
        match play(&mut w, 60.0, true) {
            (false, _, 0) => println!("    did not start"),
            (false, _, n) => println!("    ran {n} lines without taking Harry (no cutscene mode)"),
            (true, None, _) => println!("    still in cutscene mode after 60 s"),
            (true, Some(t), _) => println!("    finished after {t:.1} s"),
        }
    }
    Ok(ExitCode::SUCCESS)
}
