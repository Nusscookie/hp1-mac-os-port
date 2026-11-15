//! Class exports: the parent class and the default property values.
//!
//! A class export also holds script bytecode (code that would run inside an Unreal VM). This
//! reader never runs or decodes it. Class-level bytecode is almost always empty, because functions
//! and states are separate exports; when it isn't, the default values are found by locating the
//! property list that ends the export (see `defaults_from_end`).

use crate::{Error, ObjRef, Package, Props, Reader, Result, read_properties};

#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub name: String,
    pub parent: ObjRef,
    pub flags: u32,
    pub config_name: String,
    pub defaults: Props,
    /// True when the defaults had to be located from the end because the class has class-level
    /// bytecode in front of them.
    pub located_from_end: bool,
}

pub fn read_class(pkg: &Package, index: usize) -> Result<ClassInfo> {
    let e = &pkg.exports[index];
    if !pkg.class_name(e).eq_ignore_ascii_case("Class") {
        return Err(Error::Format(format!("{} is not a class", pkg.name(e.name))));
    }
    let data = pkg.export_data(e);
    let mut r = Reader::new(data);
    pkg.read_state_frame(e, &mut r)?;
    // UField
    let parent = pkg.read_ref(&mut r)?;
    pkg.read_ref(&mut r)?; // next field
    // UStruct
    pkg.read_ref(&mut r)?; // script text
    pkg.read_ref(&mut r)?; // first child field
    pkg.read_name(&mut r)?; // friendly name
    r.u32()?; // line
    r.u32()?; // text position
    let script_size = r.u32()?;
    let name = pkg.name(e.name).to_owned();
    if script_size != 0 {
        let defaults = defaults_from_end(pkg, data)?;
        return Ok(ClassInfo {
            name,
            parent,
            flags: 0,
            config_name: String::new(),
            defaults,
            located_from_end: true,
        });
    }
    // UState
    r.u64()?; // probe mask
    r.u64()?; // ignore mask
    r.u16()?; // label table offset
    r.u32()?; // state flags
    // UClass
    if pkg.version <= 61 {
        r.u32()?;
    }
    let flags = r.u32()?;
    r.skip(16)?; // GUID
    let deps = r.index()?;
    for _ in 0..deps.max(0) {
        pkg.read_ref(&mut r)?;
        r.u32()?;
        r.u32()?;
    }
    let imports = r.index()?;
    for _ in 0..imports.max(0) {
        r.index()?;
    }
    let mut config_name = String::new();
    if pkg.version >= 62 {
        r.index()?; // class within
        config_name = pkg.read_name(&mut r)?.to_owned();
    }
    let defaults = Props(read_properties(pkg, &mut r)?);
    if r.remaining() != 0 {
        return Err(Error::Format(format!("class {name}: {} bytes left after defaults", r.remaining())));
    }
    Ok(ClassInfo { name, parent, flags, config_name, defaults, located_from_end: false })
}

/// The defaults are the last thing in a class export and end with the `None` name. Every tag
/// boundary inside the list also parses to the end, so a candidate start must also be preceded by
/// the class's config name (one of the names classes use: `System`, `User`, `None`, …). The
/// earliest such start wins.
fn defaults_from_end(pkg: &Package, data: &[u8]) -> Result<Props> {
    let config_names: Vec<usize> = ["None", "System", "User", "Engine", "Server"]
        .iter()
        .filter_map(|n| pkg.names.iter().position(|m| m.eq_ignore_ascii_case(n)))
        .collect();
    let lower = data.len().saturating_sub(64 * 1024).max(2);
    for start in lower..data.len() {
        let preceded_by_config = [1usize, 2].iter().any(|&k| {
            start >= k
                && Reader::at(data, start - k)
                    .and_then(|mut r| r.index().map(|v| (v, r.pos())))
                    .is_ok_and(|(v, end)| end == start && config_names.contains(&(v as usize)))
        });
        if !preceded_by_config {
            continue;
        }
        let mut r = Reader::at(data, start)?;
        if let Ok(props) = read_properties(pkg, &mut r)
            && r.remaining() == 0
        {
            return Ok(Props(props));
        }
    }
    Err(Error::Format("class defaults not found".into()))
}
