//! Fonts → one JSON list per package. The page images are ordinary textures and are written by
//! the texture conversion.

use hp1_data::{FontInfo, FontPage, SectionReport};
use ue1_read::{ObjRef, Package, read_object};

use crate::{Converter, Result, write_json};

pub fn convert_fonts(conv: &Converter) -> Result<SectionReport> {
    let mut report = SectionReport::default();
    for name in conv.lib.package_names() {
        let pkg = match conv.lib.package(&name) {
            Ok(p) => p,
            Err(e) => {
                report.errors.push(format!("{name}: {e}"));
                continue;
            }
        };
        let mut fonts = Vec::new();
        for (i, e) in pkg.exports.iter().enumerate() {
            if !pkg.class_name(e).eq_ignore_ascii_case("Font") || e.size == 0 {
                continue;
            }
            match read_font(&pkg, i) {
                Ok(f) => fonts.push(f),
                Err(err) => report.errors.push(format!("{}: {err}", pkg.ref_path(ObjRef::Export(i)))),
            }
        }
        if fonts.is_empty() {
            continue;
        }
        report.items += fonts.len();
        let list = conv.out.join("fonts").join(format!("{}.json", pkg.name));
        if let Err(e) = write_json(&list, &fonts) {
            report.errors.push(format!("{}: {e}", list.display()));
        }
    }
    Ok(report)
}

/// The font layout of package version 64 and later (HP1's): a list of pages, each a texture
/// reference and its character rectangles, then the characters per page.
fn read_font(pkg: &Package, index: usize) -> Result<FontInfo> {
    if pkg.version <= 63 {
        return Err("fonts of package version 63 and older are not supported".into());
    }
    let (_, mut r) = read_object(pkg, index)?;
    let count = |n: i32| usize::try_from(n).ok().filter(|&n| n <= 65536).ok_or("bad count");
    let mut pages = Vec::new();
    for _ in 0..count(r.index()?)? {
        let texture = pkg.ref_path(pkg.read_ref(&mut r)?);
        let mut chars = Vec::new();
        for _ in 0..count(r.index()?)? {
            chars.push([r.i32()?, r.i32()?, r.i32()?, r.i32()?]);
        }
        pages.push(FontPage { texture, chars });
    }
    let chars_per_page = r.u32()?;
    Ok(FontInfo { path: pkg.ref_path(ObjRef::Export(index)), pages, chars_per_page })
}
