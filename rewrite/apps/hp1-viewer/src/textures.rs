//! Loads converted textures on demand and hands out their index in the renderer's list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use hp1_data::{DataValue, TextureInfo, TextureKind};
use hp1_render::Image;

pub struct Textures {
    data: PathBuf,
    info: HashMap<String, TextureInfo>,
    index: HashMap<String, u32>,
    /// Index 0 is a grey stand-in for missing textures.
    pub images: Vec<Image>,
    /// The texture path of each image.
    paths: Vec<String>,
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
            paths: vec![String::new()],
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
        // Water-type textures are not simulated yet; they show their source picture.
        let file = self.info.get(&key).map(|t| {
            let source = (t.kind == TextureKind::Water)
                .then(|| t.props.get("SourceTexture").and_then(DataValue::as_str))
                .flatten()
                .and_then(|s| self.info.get(&s.to_ascii_lowercase()));
            source.unwrap_or(t).file.clone()
        });
        let i = match file.map(|f| load_png(&self.data.join(f))) {
            Some(Ok(img)) => {
                self.images.push(img);
                self.paths.push(key.clone());
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

/// A texture that cycles through a chain of frames.
pub struct Animation {
    pub base: u32,
    pub frames: Vec<u32>,
    pub fps: f32,
}

impl Textures {
    /// Loads the frames of every animated texture loaded so far.
    pub fn animations(&mut self) -> Vec<Animation> {
        let mut out = Vec::new();
        // Only textures in use count as chain starts, not the frames loaded on the way.
        let in_use = self.paths.len();
        for i in 1..in_use {
            let base = self.paths[i].clone();
            let fps = self
                .info(&base)
                .and_then(|t| t.props.get("MaxFrameRate"))
                .and_then(DataValue::as_f32)
                .filter(|&f| f > 0.0)
                .unwrap_or(25.0);
            let mut frames = vec![i as u32];
            let mut cur = base.clone();
            while let Some(next) = self
                .info(&cur)
                .and_then(|t| t.props.get("AnimNext"))
                .and_then(DataValue::as_str)
                .map(str::to_owned)
            {
                if next.eq_ignore_ascii_case(&base) || frames.len() > 256 {
                    break;
                }
                frames.push(self.get(&next));
                cur = next;
            }
            if frames.len() > 1 {
                out.push(Animation { base: i as u32, frames, fps });
            }
        }
        out
    }
}

/// A fire texture being simulated.
pub struct FireTexture {
    pub index: u32,
    pub path: String,
    sim: hp1_engine::procedural::Fire,
    palette: Vec<[u8; 4]>,
    masked: bool,
    fps: f32,
    steps: u64,
}

impl FireTexture {
    /// Advances to `time` seconds and returns the new image if it changed.
    pub fn update(&mut self, time: f32) -> Option<Image> {
        let due = (time * self.fps) as u64;
        if due <= self.steps {
            return None;
        }
        for _ in 0..(due - self.steps).min(10) {
            self.sim.step();
        }
        self.steps = due;
        Some(self.image())
    }

    pub fn image(&self) -> Image {
        Image {
            width: self.sim.width as u32,
            height: self.sim.height as u32,
            rgba: self.sim.rgba(&self.palette, self.masked),
        }
    }
}

impl Textures {
    /// Sets up the simulation of every loaded fire texture.
    pub fn fires(&self) -> Vec<FireTexture> {
        let mut out = Vec::new();
        for (i, path) in self.paths.iter().enumerate().skip(1) {
            let Some(t) = self.info(path) else { continue };
            if t.kind != TextureKind::Fire {
                continue;
            }
            let palette: Vec<[u8; 4]> = t
                .palette_file
                .as_ref()
                .and_then(|f| load_png(&self.data.join(f)).ok())
                .map(|p| p.rgba.as_chunks::<4>().0.to_vec())
                .unwrap_or_default();
            let prop = |n: &str| t.props.get(n);
            let mut sim = hp1_engine::procedural::Fire::new(
                t.width,
                t.height,
                &t.sparks,
                prop("RenderHeat").and_then(DataValue::as_i64).unwrap_or(255).clamp(0, 255) as u8,
                prop("bRising").and_then(DataValue::as_bool).unwrap_or(false),
                prop("SparksLimit").and_then(DataValue::as_i64).unwrap_or(1024),
                0x9E37_79B9 ^ (i as u32).wrapping_mul(2_654_435_761),
            );
            // Fires run for a while before they are first shown.
            for _ in 0..prop("PrimeCount").and_then(DataValue::as_i64).unwrap_or(0).clamp(0, 1000) {
                sim.step();
            }
            let fps = prop("MaxFrameRate").and_then(DataValue::as_f32).filter(|&f| f > 0.0).unwrap_or(25.0);
            out.push(FireTexture {
                index: i as u32,
                path: path.clone(),
                sim,
                palette,
                masked: t.masked,
                fps,
                steps: 0,
            });
        }
        out
    }
}
