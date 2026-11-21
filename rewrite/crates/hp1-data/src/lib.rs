//! The rewrite's own data formats. The converter writes them once from the CD; the game reads
//! only these. Everything is plain JSON plus standard media files (PNG, WAV, Ogg), so it is easy to
//! inspect and to load on any platform.
//!
//! Layout of a converted game folder:
//!
//! ```text
//! manifest.json                 FORMAT_VERSION, source language, what was converted
//! textures/<Package>.json       TextureInfo list for one package
//! textures/<Package>/<Group.Name>.png
//! sounds/<Package>.json         AudioInfo list; files sounds/<Package>/<Group.Name>.wav
//! music/<Package>.json          AudioInfo list; files music/<Package>/<Name>.mp2
//! meshes/<Package>.json         MeshIndexEntry list; files meshes/<Package>/<Name>.mesh (MeshFile)
//! anims/<Package>/<Name>.anim   AnimFile
//! levels/<Map>.level            LevelFile (binary); levels/<Map>.json is a readable summary
//! classes.json                  ClassTable: every class's parent and default values
//! text/<file>.json              TextFile: localized text and settings (e.g. text/hpdialog.int.json)
//! fonts/<Package>.json          FontInfo list; the pages are textures from textures/
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod level;
pub mod mesh;

/// Bumped whenever a converted file changes shape; the game asks for a re-convert on mismatch.
pub const FORMAT_VERSION: u32 = 4;

/// A property value copied from the CD, in plain form. References to other objects become their
/// full dotted path (`Package.Group.Name`).
///
/// In JSON the value is written bare (`64`, `"Text"`, `{"x":..}`); in binary files it carries a
/// variant tag, because binary formats cannot guess the variant back.
#[derive(Clone, Debug, PartialEq)]
pub enum DataValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Vector {
        x: f32,
        y: f32,
        z: f32,
    },
    Rotator {
        pitch: i32,
        yaw: i32,
        roll: i32,
    },
    Color {
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    },
    /// A struct kept as raw bytes (hex), with its type name.
    Struct {
        r#struct: String,
        hex: String,
    },
    Null,
}

macro_rules! data_value_mirror {
    ($name:ident, $($attr:meta)?) => {
        #[derive(Serialize, Deserialize)]
        $(#[$attr])?
        enum $name {
            Bool(bool),
            Int(i64),
            Float(f64),
            Str(String),
            Vector { x: f32, y: f32, z: f32 },
            Rotator { pitch: i32, yaw: i32, roll: i32 },
            Color { r: u8, g: u8, b: u8, a: u8 },
            Struct { r#struct: String, hex: String },
            Null,
        }
        impl From<DataValue> for $name {
            fn from(v: DataValue) -> Self {
                match v {
                    DataValue::Bool(b) => Self::Bool(b),
                    DataValue::Int(i) => Self::Int(i),
                    DataValue::Float(f) => Self::Float(f),
                    DataValue::Str(s) => Self::Str(s),
                    DataValue::Vector { x, y, z } => Self::Vector { x, y, z },
                    DataValue::Rotator { pitch, yaw, roll } => Self::Rotator { pitch, yaw, roll },
                    DataValue::Color { r, g, b, a } => Self::Color { r, g, b, a },
                    DataValue::Struct { r#struct, hex } => Self::Struct { r#struct, hex },
                    DataValue::Null => Self::Null,
                }
            }
        }
        impl From<$name> for DataValue {
            fn from(v: $name) -> Self {
                match v {
                    $name::Bool(b) => Self::Bool(b),
                    $name::Int(i) => Self::Int(i),
                    $name::Float(f) => Self::Float(f),
                    $name::Str(s) => Self::Str(s),
                    $name::Vector { x, y, z } => Self::Vector { x, y, z },
                    $name::Rotator { pitch, yaw, roll } => Self::Rotator { pitch, yaw, roll },
                    $name::Color { r, g, b, a } => Self::Color { r, g, b, a },
                    $name::Struct { r#struct, hex } => Self::Struct { r#struct, hex },
                    $name::Null => Self::Null,
                }
            }
        }
    };
}
data_value_mirror!(UntaggedValue, serde(untagged));
data_value_mirror!(TaggedValue,);

impl Serialize for DataValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            UntaggedValue::from(self.clone()).serialize(s)
        } else {
            TaggedValue::from(self.clone()).serialize(s)
        }
    }
}

impl<'de> Deserialize<'de> for DataValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            UntaggedValue::deserialize(d).map(Into::into)
        } else {
            TaggedValue::deserialize(d).map(Into::into)
        }
    }
}

impl DataValue {
    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            DataValue::Int(i) => Some(i as f32),
            DataValue::Float(f) => Some(f as f32),
            DataValue::Bool(b) => Some(b as u8 as f32),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            DataValue::Int(i) => Some(i),
            DataValue::Float(f) => Some(f as i64),
            DataValue::Bool(b) => Some(b as i64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            DataValue::Bool(b) => Some(b),
            DataValue::Int(i) => Some(i != 0),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            DataValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_vec3(&self) -> Option<[f32; 3]> {
        match *self {
            DataValue::Vector { x, y, z } => Some([x, y, z]),
            _ => None,
        }
    }
    pub fn as_rotator(&self) -> Option<[i32; 3]> {
        match *self {
            DataValue::Rotator { pitch, yaw, roll } => Some([pitch, yaw, roll]),
            _ => None,
        }
    }
}

/// Properties by name. Array elements other than the first are stored as `Name[i]`.
pub type PropMap = BTreeMap<String, DataValue>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureKind {
    Plain,
    /// Procedural flames; the game simulates them from `sparks` and the properties.
    Fire,
    /// Procedural water/ripple textures (Wet, Water, Wave, Ice); simulated by the game.
    Water,
    /// Anything else that the converter only copied the base image of.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spark {
    pub kind: u8,
    pub heat: u8,
    pub x: u8,
    pub y: u8,
    pub params: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureInfo {
    /// Full path, e.g. `HP_1st.Wall.WoodBigPanel_1`.
    pub path: String,
    /// Image file relative to the converted folder.
    pub file: String,
    pub width: u32,
    pub height: u32,
    pub kind: TextureKind,
    /// Palette index 0 is see-through (the image already has alpha 0 there).
    pub masked: bool,
    /// The source palette, for procedural textures that index it at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette_file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sparks: Vec<Spark>,
    /// Every property the CD stores for this texture (flags, animation chain, detail texture, …).
    pub props: PropMap,
}

/// A bitmap font: letters cut out of one or more page textures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontInfo {
    /// Full path, e.g. `HPBase.MedInkFont`.
    pub path: String,
    pub pages: Vec<FontPage>,
    /// How many characters each page holds; character `c` is on page `c / chars_per_page`.
    pub chars_per_page: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontPage {
    /// Texture path (see `TextureInfo::path`).
    pub texture: String,
    /// Per character on this page: x, y, width, height in the texture's pixels.
    pub chars: Vec<[i32; 4]>,
}

/// A sound effect, line of dialogue or piece of music. The file is the original audio stream from
/// the CD, unchanged (WAV for sounds, MPEG Layer II for music), so loop points and quality are kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioInfo {
    pub path: String,
    pub file: String,
    /// `wav`, `mp2`, …
    pub format: String,
    pub bytes: usize,
    pub props: PropMap,
}

/// A class from the CD: where it sits in the hierarchy and the starting values it gives its
/// objects. Only data is kept here; behaviour is written in Rust.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClassEntry {
    /// Full path of the parent class, e.g. `Engine.Pawn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Default values set by this class (not including inherited ones).
    pub defaults: PropMap,
}

/// All classes by full path (`HPBase.harry`, `Engine.Actor`, …).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClassTable {
    pub classes: BTreeMap<String, ClassEntry>,
}

impl ClassTable {
    /// Finds a class by its full path or by its bare name, ignoring case.
    pub fn find(&self, name: &str) -> Option<(&str, &ClassEntry)> {
        if let Some((k, v)) = self.classes.get_key_value(name) {
            return Some((k.as_str(), v));
        }
        let want = name.to_ascii_lowercase();
        self.classes.iter().find_map(|(k, v)| {
            let lk = k.to_ascii_lowercase();
            let short = lk.rsplit('.').next().unwrap_or(&lk);
            (lk == want || short == want).then_some((k.as_str(), v))
        })
    }

    /// The class and its ancestors, nearest first.
    pub fn chain(&self, name: &str) -> Vec<(&str, &ClassEntry)> {
        let mut out = Vec::new();
        let mut cur = self.find(name);
        while let Some((path, entry)) = cur {
            if out.len() > 64 || out.iter().any(|(p, _)| *p == path) {
                break;
            }
            out.push((path, entry));
            cur = entry.parent.as_deref().and_then(|p| self.find(p));
        }
        out
    }

    /// Default values with inheritance applied: a class's own values override its parents'.
    pub fn effective_defaults(&self, name: &str) -> PropMap {
        let mut merged = PropMap::new();
        for (_, entry) in self.chain(name).into_iter().rev() {
            for (k, v) in &entry.defaults {
                merged.insert(k.clone(), v.clone());
            }
        }
        merged
    }

    /// True if `name` is `ancestor` or derives from it.
    pub fn is_a(&self, name: &str, ancestor: &str) -> bool {
        self.chain(name).iter().any(|(p, _)| {
            p.eq_ignore_ascii_case(ancestor)
                || p.rsplit('.').next().is_some_and(|s| s.eq_ignore_ascii_case(ancestor))
        })
    }
}

/// One value or, for keys repeated in a section, all of them in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextValue {
    One(String),
    Many(Vec<String>),
}

/// An INI-style text file (localization or settings): section → key → value. Keys keep their
/// original spelling; look them up ignoring case.
pub type TextFile = BTreeMap<String, BTreeMap<String, TextValue>>;

/// Looks up `section`/`key` ignoring case and returns the first value.
pub fn text_lookup<'a>(file: &'a TextFile, section: &str, key: &str) -> Option<&'a str> {
    let (_, sec) = file.iter().find(|(k, _)| k.eq_ignore_ascii_case(section))?;
    let (_, v) = sec.iter().find(|(k, _)| k.eq_ignore_ascii_case(key))?;
    match v {
        TextValue::One(s) => Some(s),
        TextValue::Many(v) => v.first().map(String::as_str),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    /// Notes per converted area, e.g. counts and problems.
    pub sections: BTreeMap<String, SectionReport>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SectionReport {
    pub items: usize,
    pub errors: Vec<String>,
    /// Converted, but with something missing (e.g. a reference to a package not on the CD).
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_in_binary() {
        let mut m = PropMap::new();
        m.insert("A".into(), DataValue::Int(3));
        m.insert("B".into(), DataValue::Float(0.5));
        m.insert("C".into(), DataValue::Vector { x: 1.0, y: 2.0, z: 3.0 });
        m.insert("D".into(), DataValue::Str("x".into()));
        m.insert("E".into(), DataValue::Null);
        let bytes = postcard::to_stdvec(&m).unwrap();
        let back: PropMap = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(m, back);
    }
}
