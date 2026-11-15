//! Developer tools for the HP1 rewrite.
//!
//!   hp1-tools info <package>...            header and export counts per class
//!   hp1-tools exports <package> [class]    list exports, optionally of one class
//!   hp1-tools props <package> <object>     an object's property values
//!   hp1-tools check-data <game folder>     read every package and object property list

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use ue1_read::{ObjRef, Package, Reader, Value, read_properties};

const USAGE: &str = "usage:
  hp1-tools info <package>...
  hp1-tools exports <package> [class]
  hp1-tools props <package> <object>
  hp1-tools check-data <game folder>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("info") if args.len() > 1 => cmd_info(&args[1..]),
        Some("exports") if args.len() > 1 => cmd_exports(&args[1], args.get(2).map(String::as_str)),
        Some("props") if args.len() == 3 => cmd_props(&args[1], &args[2]),
        Some("check-data") if args.len() == 2 => cmd_check_data(Path::new(&args[1])),
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

const PACKAGE_EXTENSIONS: &[&str] = &["u", "unr", "utx", "uax", "umx", "usx"];

fn find_packages(game: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for dir in ["System", "Maps", "Textures", "Sounds", "Music"] {
        let dir = game.join(dir);
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries {
            let path = entry?.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            // Language variants are named like `AllDialog.ger_uax` or `MenuArt.fre_utx`.
            let base_ext = ext.rsplit('_').next().unwrap_or("");
            if path.is_file()
                && (PACKAGE_EXTENSIONS.contains(&ext.as_str()) || PACKAGE_EXTENSIONS.contains(&base_ext))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    if out.is_empty() {
        bail!("no Unreal packages found under {}", game.display());
    }
    Ok(out)
}

/// Opens every package and reads the property list of every ordinary object. Script
/// definitions (classes, functions, states, …) are skipped: the rewrite never uses script code.
fn cmd_check_data(game: &Path) -> Result<ExitCode> {
    let paths = find_packages(game)?;
    let (mut objects, mut props, mut failures) = (0usize, 0usize, 0usize);
    let mut failed_packages = 0usize;
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
    Ok(if failed_packages == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}
