//! Textures → PNG plus one JSON list per package.

use std::sync::Arc;

use hp1_data::{SectionReport, Spark, TextureInfo, TextureKind};
use rayon::prelude::*;
use ue1_read::texture::{TextureFormat, mip_to_rgba, read_palette, read_texture};
use ue1_read::{ObjRef, Package};

use crate::{Converter, Result, file_name, path_in_package, prop_map, write_json, write_png};

fn texture_kind(class: &str) -> Option<TextureKind> {
    Some(match class.to_ascii_lowercase().as_str() {
        "texture" => TextureKind::Plain,
        "firetexture" => TextureKind::Fire,
        "wettexture" | "watertexture" | "wavetexture" | "icetexture" => TextureKind::Water,
        c if c.ends_with("texture") => TextureKind::Other,
        _ => return None,
    })
}

pub fn convert_textures(conv: &Converter) -> Result<SectionReport> {
    let names = conv.lib.package_names();
    let results: Vec<SectionReport> = names
        .par_iter()
        .map(|name| match conv.lib.package(name) {
            Ok(pkg) => convert_package(conv, &pkg),
            Err(e) => SectionReport { errors: vec![format!("{name}: {e}")], ..Default::default() },
        })
        .collect();
    let mut report = SectionReport::default();
    for r in results {
        report.items += r.items;
        report.errors.extend(r.errors);
        report.warnings.extend(r.warnings);
    }
    Ok(report)
}

fn convert_package(conv: &Converter, pkg: &Arc<Package>) -> SectionReport {
    let mut infos = Vec::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (i, e) in pkg.exports.iter().enumerate() {
        let Some(kind) = texture_kind(pkg.class_name(e)) else { continue };
        if e.size == 0 {
            continue;
        }
        match convert_one(conv, pkg, i, kind, &mut warnings) {
            Ok(info) => infos.push(info),
            Err(err) => errors.push(format!("{}: {err}", pkg.ref_path(ObjRef::Export(i)))),
        }
    }
    let count = infos.len();
    if !infos.is_empty() {
        infos.sort_by(|a, b| a.path.cmp(&b.path));
        let list = conv.out.join("textures").join(format!("{}.json", pkg.name));
        if let Err(e) = write_json(&list, &infos) {
            errors.push(format!("{}: {e}", list.display()));
        }
    }
    SectionReport { items: count, errors, warnings }
}

fn convert_one(
    conv: &Converter,
    pkg: &Arc<Package>,
    index: usize,
    kind: TextureKind,
    warnings: &mut Vec<String>,
) -> Result<TextureInfo> {
    let tex = read_texture(pkg, index)?;
    let path = pkg.ref_path(ObjRef::Export(index));
    let masked = tex.props.bool("bMasked").unwrap_or(false);

    let palette = match conv.lib.resolve(pkg, tex.palette) {
        Ok(Some(h)) => Some((h.path(), read_palette(&h.package, h.export)?)),
        Ok(None) => None,
        Err(e) if conv.lib.is_missing_package(&e) => {
            // A few stock Unreal textures point at a palette package that isn't on the HP1 CD.
            // None of them are used by the game; keep them with a grey ramp so nothing breaks.
            warnings.push(format!("{path}: {e}, used a grey palette"));
            Some((format!("{path}.GreyPalette"), (0..=255u8).map(|v| [v, v, v, 255]).collect()))
        }
        Err(e) => return Err(e.into()),
    };
    let mut colors = palette.as_ref().map(|(_, c)| c.clone());
    if let Some(c) = colors.as_mut() {
        // Palette alpha bytes are not meaningful for ordinary textures.
        for (i, col) in c.iter_mut().enumerate() {
            col[3] = if masked && i == 0 { 0 } else { 255 };
        }
    }

    let dir = conv.out.join("textures").join(&pkg.name);
    let base = file_name(path_in_package(&path));
    let file = format!("textures/{}/{base}.png", pkg.name);

    let procedural = matches!(kind, TextureKind::Fire | TextureKind::Water | TextureKind::Other);
    let (width, height, rgba) = match tex.mips.first() {
        Some(mip) if mip.width > 0 && mip.height > 0 && !mip.data.is_empty() => {
            let rgba = mip_to_rgba(tex.format, mip, colors.as_deref())?;
            (mip.width, mip.height, rgba)
        }
        _ if procedural => {
            // Procedural textures can ship with no pixels; the game draws them every frame.
            let w = tex.props.int("UClamp").or(tex.props.byte("UBits").map(|b| 1 << b)).unwrap_or(64) as u32;
            let h = tex.props.int("VClamp").or(tex.props.byte("VBits").map(|b| 1 << b)).unwrap_or(64) as u32;
            (w, h, vec![0; (w * h * 4) as usize])
        }
        _ => return Err("texture has no image data".into()),
    };
    write_png(&dir.join(format!("{base}.png")), width, height, &rgba)?;

    let mut palette_file = None;
    if procedural
        && tex.format == TextureFormat::P8
        && let Some((pal_path, pal)) = &palette
    {
        let name = format!("{}.pal.png", file_name(pal_path));
        let flat: Vec<u8> = pal.iter().flatten().copied().collect();
        write_png(&dir.join(&name), pal.len() as u32, 1, &flat)?;
        palette_file = Some(format!("textures/{}/{name}", pkg.name));
    }

    Ok(TextureInfo {
        path,
        file,
        width,
        height,
        kind,
        masked,
        palette_file,
        sparks: tex
            .sparks
            .iter()
            .map(|s| Spark { kind: s.kind, heat: s.heat, x: s.x, y: s.y, params: s.bytes })
            .collect(),
        props: prop_map(pkg, &tex.props),
    })
}
