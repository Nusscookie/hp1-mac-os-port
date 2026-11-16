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
  hp1-tools actors <converted folder> <map> [name or class filter]
  hp1-tools fire <converted folder> <texture> <out.png>
  hp1-tools check-audio <converted folder>";

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
        Some("actors") if (3..=4).contains(&args.len()) => {
            cmd_actors(Path::new(&args[1]), &args[2], args.get(3).map(String::as_str))
        }
        Some("fire") if args.len() == 4 => {
            preview::fire(Path::new(&args[1]), &args[2], Path::new(&args[3])).map(|_| ExitCode::SUCCESS)
        }
        Some("check-audio") if args.len() == 2 => cmd_check_audio(Path::new(&args[1])),
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

fn cmd_actors(data: &Path, map: &str, filter: Option<&str>) -> Result<ExitCode> {
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
