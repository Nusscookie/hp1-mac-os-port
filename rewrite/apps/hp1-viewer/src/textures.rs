//! Loads converted textures on demand and hands out their index in the renderer's list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use hp1_data::{DataValue, TextureInfo};
use hp1_render::Image;

pub struct Textures {
    data: PathBuf,
    info: HashMap<String, TextureInfo>,
    index: HashMap<String, u32>,
    /// Index 0 is a grey stand-in for missing textures.
    pub images: Vec<Image>,
    pub missing: usize,
}

pub fn load_png(path: &Path) -> Result<Image> {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
    let mut reader = dec.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().context("png size")?];
    let info = reader.next_frame(&mut buf)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        bail!("{} is not 8-bit RGBA", path.display());
    }
    buf.truncate(info.buffer_size());
    Ok(Image { width: info.width, height: info.height, rgba: buf })
}

impl Textures {
    pub fn open(data: &Path) -> Result<Self> {
        let mut info = HashMap::new();
        for e in std::fs::read_dir(data.join("textures"))? {
            let p = e?.path();
            if p.extension().is_some_and(|x| x == "json") {
                let list: Vec<TextureInfo> = serde_json::from_slice(&std::fs::read(&p)?)?;
                info.extend(list.into_iter().map(|t| (t.path.to_ascii_lowercase(), t)));
            }
        }
        Ok(Self {
            data: data.to_owned(),
            info,
            index: HashMap::new(),
            images: vec![Image { width: 1, height: 1, rgba: vec![128, 128, 128, 255] }],
            missing: 0,
        })
    }

    pub fn info(&self, path: &str) -> Option<&TextureInfo> {
        self.info.get(&path.to_ascii_lowercase())
    }

    pub fn draw_scale(&self, path: &str) -> f32 {
        self.info(path).and_then(|t| t.props.get("DrawScale")).and_then(DataValue::as_f32).unwrap_or(1.0)
    }

    /// The renderer index of a texture, loading it the first time (0 if it can't be found).
    pub fn get(&mut self, path: &str) -> u32 {
        let key = path.to_ascii_lowercase();
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        let i = match self.info.get(&key).map(|t| load_png(&self.data.join(&t.file))) {
            Some(Ok(img)) => {
                self.images.push(img);
                self.images.len() as u32 - 1
            }
            _ => {
                self.missing += 1;
                0
            }
        };
        self.index.insert(key, i);
        i
    }
}
