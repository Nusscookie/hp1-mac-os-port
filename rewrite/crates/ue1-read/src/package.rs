//! Package header, name table, import table and export table.

use std::path::Path;

use crate::{Error, Reader, Result};

const SIGNATURE: u32 = 0x9E2A_83C1;

/// Object flags stored in the export table that matter when reading object data.
pub mod object_flags {
    /// The object's data starts with a saved script state frame.
    pub const HAS_STACK: u32 = 0x0200_0000;
    pub const PUBLIC: u32 = 0x0000_0004;
    pub const STANDALONE: u32 = 0x0008_0000;
}

/// A reference to an object: nothing, an entry in this package's import table, or one of its
/// exports. Stored on disk as a signed compact index (negative = import, positive = export).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ObjRef {
    #[default]
    None,
    Import(usize),
    Export(usize),
}

impl ObjRef {
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            0 => ObjRef::None,
            r if r < 0 => ObjRef::Import((-(r as i64) - 1) as usize),
            r => ObjRef::Export(r as usize - 1),
        }
    }

    pub fn is_none(self) -> bool {
        self == ObjRef::None
    }
}

#[derive(Clone, Debug)]
pub struct Import {
    pub class_package: usize,
    pub class_name: usize,
    pub outer: ObjRef,
    pub name: usize,
}

#[derive(Clone, Debug)]
pub struct Export {
    pub class: ObjRef,
    pub super_: ObjRef,
    pub outer: ObjRef,
    pub name: usize,
    pub flags: u32,
    pub size: usize,
    pub offset: usize,
}

/// A saved script state frame at the start of an object's data. The converter only needs to
/// step over it (and maybe know which state an actor starts in); the code position is ignored.
#[derive(Clone, Debug, Default)]
pub struct StateFrame {
    pub node: ObjRef,
    pub state: ObjRef,
    pub probe_mask: u64,
    pub latent_action: u32,
}

pub struct Package {
    /// File name without extension, e.g. `HPBase` or `Lev_Tut1`.
    pub name: String,
    pub data: Vec<u8>,
    pub version: u16,
    pub licensee: u16,
    pub flags: u32,
    pub guid: [u8; 16],
    pub names: Vec<String>,
    pub name_flags: Vec<u32>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

impl Package {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let data = std::fs::read(path)?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Self::parse(name, data).map_err(|e| Error::Format(format!("{}: {e}", path.display())))
    }

    pub fn parse(name: String, data: Vec<u8>) -> Result<Self> {
        let mut r = Reader::new(&data);
        if r.u32()? != SIGNATURE {
            return Err(Error::Format("not an Unreal package".into()));
        }
        let version = r.u16()?;
        let licensee = r.u16()?;
        if !(60..100).contains(&version) {
            return Err(Error::Format(format!("unsupported package version {version}")));
        }
        let flags = r.u32()?;
        let name_count = r.u32()? as usize;
        let name_offset = r.u32()? as usize;
        let export_count = r.u32()? as usize;
        let export_offset = r.u32()? as usize;
        let import_count = r.u32()? as usize;
        let import_offset = r.u32()? as usize;
        let mut guid = [0; 16];
        if version >= 68 {
            guid.copy_from_slice(r.bytes(16)?);
            let generations = r.u32()? as usize;
            r.skip(generations.checked_mul(8).ok_or_else(|| Error::Format("bad generation count".into()))?)?;
        } else {
            r.skip(8)?; // heritage count and offset
        }

        // Guard the counts so a damaged file can't make us allocate gigabytes.
        for (what, count) in [("name", name_count), ("export", export_count), ("import", import_count)] {
            if count > data.len() {
                return Err(Error::Format(format!("{what} count {count} larger than the file")));
            }
        }

        let mut names = Vec::with_capacity(name_count);
        let mut name_flags = Vec::with_capacity(name_count);
        r.seek(name_offset)?;
        for _ in 0..name_count {
            names.push(if version >= 64 { r.string()? } else { r.cstring()? });
            name_flags.push(r.u32()?);
        }

        let check_name = |i: i32| -> Result<usize> {
            usize::try_from(i)
                .ok()
                .filter(|&i| i < name_count)
                .ok_or_else(|| Error::Format(format!("name index {i} out of range")))
        };

        let mut imports = Vec::with_capacity(import_count);
        r.seek(import_offset)?;
        for _ in 0..import_count {
            imports.push(Import {
                class_package: check_name(r.index()?)?,
                class_name: check_name(r.index()?)?,
                outer: ObjRef::from_raw(r.i32()?),
                name: check_name(r.index()?)?,
            });
        }

        let mut exports = Vec::with_capacity(export_count);
        r.seek(export_offset)?;
        for _ in 0..export_count {
            let class = ObjRef::from_raw(r.index()?);
            let super_ = ObjRef::from_raw(r.index()?);
            let outer = ObjRef::from_raw(r.i32()?);
            let name = check_name(r.index()?)?;
            let flags = r.u32()?;
            let size = r.index()?;
            let offset = if size > 0 { r.index()? } else { 0 };
            let (size, offset) = (size.max(0) as usize, offset.max(0) as usize);
            if offset.checked_add(size).is_none_or(|end| end > data.len()) {
                return Err(Error::Format(format!("export {} lies outside the file", names[name])));
            }
            exports.push(Export { class, super_, outer, name, flags, size, offset });
        }

        let pkg = Package { name, data, version, licensee, flags, guid, names, name_flags, imports, exports };
        for (i, e) in pkg.exports.iter().enumerate() {
            for r in [e.class, e.super_, e.outer] {
                pkg.check_ref(r).map_err(|err| Error::Format(format!("export {i}: {err}")))?;
            }
        }
        for (i, imp) in pkg.imports.iter().enumerate() {
            pkg.check_ref(imp.outer).map_err(|err| Error::Format(format!("import {i}: {err}")))?;
        }
        Ok(pkg)
    }

    pub fn check_ref(&self, r: ObjRef) -> Result<()> {
        let ok = match r {
            ObjRef::None => true,
            ObjRef::Import(i) => i < self.imports.len(),
            ObjRef::Export(i) => i < self.exports.len(),
        };
        if ok { Ok(()) } else { Err(Error::Format(format!("object reference {r:?} out of range"))) }
    }

    /// Reads an object reference (compact index) and checks it against the tables.
    pub fn read_ref(&self, r: &mut Reader) -> Result<ObjRef> {
        let o = ObjRef::from_raw(r.index()?);
        self.check_ref(o)?;
        Ok(o)
    }

    /// Reads a name reference (compact index into the name table).
    pub fn read_name<'p>(&'p self, r: &mut Reader) -> Result<&'p str> {
        let i = r.index()?;
        usize::try_from(i)
            .ok()
            .and_then(|i| self.names.get(i))
            .map(String::as_str)
            .ok_or_else(|| Error::Format(format!("name index {i} out of range")))
    }

    pub fn name(&self, index: usize) -> &str {
        &self.names[index]
    }

    /// The object's own name, or `None` for a null reference.
    pub fn ref_name(&self, r: ObjRef) -> Option<&str> {
        match r {
            ObjRef::None => None,
            ObjRef::Import(i) => Some(self.name(self.imports[i].name)),
            ObjRef::Export(i) => Some(self.name(self.exports[i].name)),
        }
    }

    pub fn ref_outer(&self, r: ObjRef) -> ObjRef {
        match r {
            ObjRef::None => ObjRef::None,
            ObjRef::Import(i) => self.imports[i].outer,
            ObjRef::Export(i) => self.exports[i].outer,
        }
    }

    /// Dotted path including outers, e.g. `HPBase.CutScene.Cast` or `Engine.Texture`. Exports
    /// get this package's name in front, imports their outermost package.
    pub fn ref_path(&self, r: ObjRef) -> String {
        let mut parts = Vec::new();
        let mut cur = r;
        let mut guard = 0;
        while let Some(n) = self.ref_name(cur) {
            parts.push(n.to_owned());
            let outer = self.ref_outer(cur);
            if outer.is_none() {
                if matches!(cur, ObjRef::Export(_)) {
                    parts.push(self.name.clone());
                }
                break;
            }
            cur = outer;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        parts.reverse();
        parts.join(".")
    }

    /// The class name of an export. A null class means the export is itself a class.
    pub fn class_name(&self, e: &Export) -> &str {
        self.ref_name(e.class).unwrap_or("Class")
    }

    pub fn export_data(&self, e: &Export) -> &[u8] {
        &self.data[e.offset..e.offset + e.size]
    }

    pub fn find_export(&self, name: &str, class: Option<&str>) -> Option<usize> {
        self.exports.iter().position(|e| {
            self.name(e.name).eq_ignore_ascii_case(name)
                && class.is_none_or(|c| self.class_name(e).eq_ignore_ascii_case(c))
        })
    }

    /// Reads the state frame at the start of an object's data, if the export has one.
    pub fn read_state_frame(&self, e: &Export, r: &mut Reader) -> Result<Option<StateFrame>> {
        if e.flags & object_flags::HAS_STACK == 0 {
            return Ok(None);
        }
        let node = self.read_ref(r)?;
        let state = self.read_ref(r)?;
        let probe_mask = r.u64()?;
        let latent_action = r.u32()?;
        if !node.is_none() {
            r.index()?; // code offset inside the node: script position, not needed
        }
        Ok(Some(StateFrame { node, state, probe_mask, latent_action }))
    }

    /// True for exports whose data is a script/type definition rather than an ordinary object
    /// with tagged properties (classes, structs, functions, states, properties, enums, consts).
    pub fn is_field_export(&self, e: &Export) -> bool {
        is_field_class(self.class_name(e))
    }
}

pub fn is_field_class(class: &str) -> bool {
    matches!(class, "Class" | "Struct" | "State" | "Function" | "Enum" | "Const")
        || (class.ends_with("Property") && !class.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objref_from_raw() {
        assert_eq!(ObjRef::from_raw(0), ObjRef::None);
        assert_eq!(ObjRef::from_raw(1), ObjRef::Export(0));
        assert_eq!(ObjRef::from_raw(-1), ObjRef::Import(0));
        assert_eq!(ObjRef::from_raw(-5), ObjRef::Import(4));
    }

    /// A tiny hand-built version 69 package: names, one import, one export.
    #[test]
    fn parse_minimal_package() {
        let mut d = Vec::new();
        let header_len = 4 + 2 + 2 + 4 + 6 * 4 + 16 + 4;
        d.extend_from_slice(&SIGNATURE.to_le_bytes());
        d.extend_from_slice(&69u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        let names: [&[u8]; 4] = [b"None", b"Core", b"Class", b"Thing"];
        let names_len: usize = names.iter().map(|n| 1 + n.len() + 1 + 4).sum();
        let name_off = header_len;
        let import_off = name_off + names_len;
        let import_len = 1 + 1 + 4 + 1;
        let export_off = import_off + import_len;
        for v in [4, name_off, 1, export_off, 1, import_off] {
            d.extend_from_slice(&(v as u32).to_le_bytes());
        }
        d.extend_from_slice(&[0; 16]);
        d.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(d.len(), header_len);
        for n in names {
            d.push(n.len() as u8 + 1);
            d.extend_from_slice(n);
            d.push(0);
            d.extend_from_slice(&0u32.to_le_bytes());
        }
        // import: Core.Class "Class" (outer none)
        d.extend_from_slice(&[1, 2]);
        d.extend_from_slice(&0i32.to_le_bytes());
        d.push(2);
        // export: class = import 0 (raw -1 = 0x81), super none, outer none, name "Thing", no data
        d.extend_from_slice(&[0x81, 0]);
        d.extend_from_slice(&0i32.to_le_bytes());
        d.push(3);
        d.extend_from_slice(&0u32.to_le_bytes());
        d.push(0);

        let p = Package::parse("Test".into(), d).unwrap();
        assert_eq!(p.names, ["None", "Core", "Class", "Thing"]);
        assert_eq!(p.exports.len(), 1);
        assert_eq!(p.class_name(&p.exports[0]), "Class");
        assert_eq!(p.ref_path(ObjRef::Export(0)), "Test.Thing");
        assert_eq!(p.find_export("thing", None), Some(0));
    }
}
