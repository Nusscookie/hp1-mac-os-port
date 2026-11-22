//! Spell gestures (`Engine.Gesture` objects): the 2D patterns drawn with the wand.

use hp1_data::{Gesture, GestureTable, SectionReport};
use ue1_read::{ObjRef, Reader, Value, read_object};

use crate::{Converter, Result, write_json};

/// A dynamic array as saved in a property: a compact-index count, then the elements.
fn array<T>(
    raw: &[u8],
    mut item: impl FnMut(&mut Reader) -> ue1_read::Result<T>,
) -> ue1_read::Result<Vec<T>> {
    let mut r = Reader::new(raw);
    let n = r.index()?.max(0) as usize;
    let items = (0..n).map(|_| item(&mut r)).collect::<ue1_read::Result<Vec<T>>>()?;
    if r.remaining() != 0 {
        return Err(ue1_read::Error::Format(format!("{} bytes left after {n} array items", r.remaining())));
    }
    Ok(items)
}

pub fn convert_gestures(conv: &Converter) -> Result<SectionReport> {
    let mut table = GestureTable::default();
    let mut report = SectionReport::default();
    for name in conv.lib.package_names() {
        let Ok(pkg) = conv.lib.package(&name) else { continue };
        for (i, e) in pkg.exports.iter().enumerate() {
            if pkg.class_name(e) != "Gesture" || e.size == 0 {
                continue;
            }
            let path = pkg.ref_path(ObjRef::Export(i));
            let read = || -> ue1_read::Result<Gesture> {
                let (props, _) = read_object(&pkg, i)?;
                let raw = |n| match props.get(n) {
                    Some(Value::Raw(b)) => b.clone(),
                    _ => Vec::new(),
                };
                let points = array(&raw("Points"), |r| Ok([r.f32()?, r.f32()?, r.f32()?]))?;
                let segments = array(&raw("Segments"), |r| r.i32())?;
                Ok(Gesture { points, segments })
            };
            match read() {
                Ok(g) => {
                    table.gestures.insert(path, g);
                }
                Err(err) => report.errors.push(format!("{path}: {err}")),
            }
        }
    }
    report.items = table.gestures.len();
    write_json(&conv.out.join("gestures.json"), &table)?;
    Ok(report)
}
