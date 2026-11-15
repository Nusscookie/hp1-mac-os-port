//! The class table: every class's parent and its default values. No script code is read.

use hp1_data::{ClassEntry, ClassTable, SectionReport};
use ue1_read::ObjRef;
use ue1_read::class::read_class;

use crate::{Converter, Result, prop_map, write_json};

pub fn convert_classes(conv: &Converter) -> Result<SectionReport> {
    let mut table = ClassTable::default();
    let mut report = SectionReport::default();
    for name in conv.lib.package_names() {
        let pkg = match conv.lib.package(&name) {
            Ok(p) => p,
            Err(e) => {
                report.errors.push(format!("{name}: {e}"));
                continue;
            }
        };
        for (i, e) in pkg.exports.iter().enumerate() {
            if pkg.class_name(e) != "Class" || e.size == 0 {
                continue;
            }
            let path = pkg.ref_path(ObjRef::Export(i));
            match read_class(&pkg, i) {
                Ok(c) => {
                    if c.located_from_end {
                        report.warnings.push(format!("{path}: defaults located from the end of the class"));
                    }
                    let parent = (!c.parent.is_none()).then(|| pkg.ref_path(c.parent));
                    table.classes.insert(path, ClassEntry { parent, defaults: prop_map(&pkg, &c.defaults) });
                }
                Err(err) => report.errors.push(format!("{path}: {err}")),
            }
        }
    }
    report.items = table.classes.len();
    write_json(&conv.out.join("classes.json"), &table)?;
    Ok(report)
}
