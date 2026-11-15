//! Sounds and music: the original audio files are written out unchanged, plus a JSON list per package.

use std::sync::Arc;

use hp1_data::{AudioInfo, SectionReport};
use rayon::prelude::*;
use ue1_read::sound::read_sound;
use ue1_read::{ObjRef, Package};

use crate::{Converter, Result, file_name, path_in_package, prop_map, write_json};

/// Converts every export of `class` (`Sound` or `Music`) into `<folder>/`.
pub fn convert_audio(conv: &Converter, class: &str, folder: &str) -> Result<SectionReport> {
    let reports: Vec<SectionReport> = conv
        .lib
        .package_names()
        .par_iter()
        .map(|name| match conv.lib.package(name) {
            Ok(pkg) => convert_package(conv, &pkg, class, folder),
            Err(e) => SectionReport { errors: vec![format!("{name}: {e}")], ..Default::default() },
        })
        .collect();
    let mut total = SectionReport::default();
    for r in reports {
        total.items += r.items;
        total.errors.extend(r.errors);
        total.warnings.extend(r.warnings);
    }
    Ok(total)
}

fn convert_package(conv: &Converter, pkg: &Arc<Package>, class: &str, folder: &str) -> SectionReport {
    let mut infos = Vec::new();
    let mut report = SectionReport::default();
    for (i, e) in pkg.exports.iter().enumerate() {
        if e.size == 0 || !pkg.class_name(e).eq_ignore_ascii_case(class) {
            continue;
        }
        match convert_one(conv, pkg, i, folder) {
            Ok(info) => infos.push(info),
            Err(err) => report.errors.push(format!("{}: {err}", pkg.ref_path(ObjRef::Export(i)))),
        }
    }
    report.items = infos.len();
    if !infos.is_empty() {
        infos.sort_by(|a, b| a.path.cmp(&b.path));
        let list = conv.out.join(folder).join(format!("{}.json", pkg.name));
        if let Err(e) = write_json(&list, &infos) {
            report.errors.push(format!("{}: {e}", list.display()));
        }
    }
    report
}

fn convert_one(conv: &Converter, pkg: &Arc<Package>, index: usize, folder: &str) -> Result<AudioInfo> {
    let snd = read_sound(pkg, index)?;
    let path = pkg.ref_path(ObjRef::Export(index));
    let ext = if snd.format.is_empty() { "bin".to_owned() } else { file_name(&snd.format) };
    if ext == "wav" && !snd.data.starts_with(b"RIFF") {
        return Err("WAV data without a RIFF header".into());
    }
    let file = format!("{folder}/{}/{}.{ext}", pkg.name, file_name(path_in_package(&path)));
    let dest = conv.out.join(&file);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&dest, &snd.data)?;
    Ok(AudioInfo { path, file, format: snd.format, bytes: snd.data.len(), props: prop_map(pkg, &snd.props) })
}
