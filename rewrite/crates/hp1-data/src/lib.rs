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
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bumped whenever a converted file changes shape; the game asks for a re-convert on mismatch.
pub const FORMAT_VERSION: u32 = 1;

/// A property value copied from the CD, in plain form. References to other objects become their
/// full dotted path (`Package.Group.Name`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
