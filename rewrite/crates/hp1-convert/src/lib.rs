//! Converts an installed HP1 game folder (Unreal packages from the CD) into the rewrite's own
//! formats (see `hp1-data`). This is the only part of the rewrite that knows Unreal's formats.
//! Script code inside the packages is never read.

mod audio;
mod classes;
mod meshes;
mod text;
mod textures;

use std::path::{Path, PathBuf};

use hp1_data::{DataValue, FORMAT_VERSION, Manifest, PropMap};
use ue1_read::{Library, Package, Props, Value};

pub use audio::convert_audio;
pub use classes::convert_classes;
pub use meshes::convert_meshes;
pub use text::convert_text;
pub use textures::convert_textures;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct Converter {
    pub lib: Library,
    pub game: PathBuf,
    pub out: PathBuf,
}

impl Converter {
    pub fn new(game: &Path, out: &Path) -> Result<Self> {
        Ok(Self { lib: Library::scan(game)?, game: game.to_owned(), out: out.to_owned() })
    }

    /// Converts everything and writes `manifest.json`.
    pub fn convert_all(&self) -> Result<Manifest> {
        std::fs::create_dir_all(&self.out)?;
        let mut manifest = Manifest { format_version: FORMAT_VERSION, ..Default::default() };
        manifest.sections.insert("textures".into(), convert_textures(self)?);
        manifest.sections.insert("sounds".into(), convert_audio(self, "Sound", "sounds")?);
        manifest.sections.insert("music".into(), convert_audio(self, "Music", "music")?);
        manifest.sections.insert("meshes".into(), convert_meshes(self)?);
        manifest.sections.insert("classes".into(), convert_classes(self)?);
        manifest.sections.insert("text".into(), convert_text(self, &self.game)?);
        write_json(&self.out.join("manifest.json"), &manifest)?;
        Ok(manifest)
    }
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Turns a dotted object path into a safe file name (keeps letters, digits, `.`, `-`, `_`).
pub fn file_name(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect()
}

/// The object path without its leading package name: `HP_1st.Wall.Panel` → `Wall.Panel`.
pub fn path_in_package(path: &str) -> &str {
    path.split_once('.').map_or(path, |(_, rest)| rest)
}

pub fn data_value(pkg: &Package, v: &Value) -> DataValue {
    match v {
        Value::Byte(b) => DataValue::Int(*b as i64),
        Value::Int(i) => DataValue::Int(*i as i64),
        Value::Bool(b) => DataValue::Bool(*b),
        Value::Float(f) => DataValue::Float(*f as f64),
        Value::Object(r) if r.is_none() => DataValue::Null,
        Value::Object(r) => DataValue::Str(pkg.ref_path(*r)),
        Value::Name(s) | Value::Str(s) => DataValue::Str(s.clone()),
        Value::Vector([x, y, z]) => DataValue::Vector { x: *x, y: *y, z: *z },
        Value::Rotator([pitch, yaw, roll]) => DataValue::Rotator { pitch: *pitch, yaw: *yaw, roll: *roll },
        Value::Color([r, g, b, a]) => DataValue::Color { r: *r, g: *g, b: *b, a: *a },
        Value::Struct(name, raw) => DataValue::Struct { r#struct: name.clone(), hex: hex(raw) },
        Value::Raw(raw) => DataValue::Struct { r#struct: "raw".into(), hex: hex(raw) },
    }
}

pub fn prop_map(pkg: &Package, props: &Props) -> PropMap {
    props
        .iter()
        .map(|p| {
            let key =
                if p.array_index == 0 { p.name.clone() } else { format!("{}[{}]", p.name, p.array_index) };
            (key, data_value(pkg, &p.value))
        })
        .collect()
}

fn hex(raw: &[u8]) -> String {
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut w = enc.write_header()?;
    w.write_image_data(rgba)?;
    Ok(())
}
