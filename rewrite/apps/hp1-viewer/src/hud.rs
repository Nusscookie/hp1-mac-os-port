//! Draws the engine's HUD tiles with the converted fonts and textures.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use hp1_data::{FontInfo, TextureInfo};
use hp1_engine::hud::{Font, HudFonts, Style};
use hp1_engine::world::World;
use hp1_render::{Gpu, Overlay, Quad, wgpu};

use crate::textures::load_png;

#[derive(Clone, Copy)]
struct Image {
    /// Overlay number.
    id: u32,
    w: f32,
    h: f32,
    masked: bool,
}

pub struct HudDraw {
    pub overlay: Overlay,
    data: PathBuf,
    fonts: Option<HudFonts>,
    /// Texture lists per package, loaded when first needed.
    packages: HashMap<String, HashMap<String, TextureInfo>>,
    /// Uploaded images by lower-case texture path (None = missing).
    images: HashMap<String, Option<Image>>,
}

/// The language the game is set to (`[Engine.Engine] Language`), else `int`.
pub fn language(data: &Path) -> String {
    std::fs::read(data.join("text").join("default.ini.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["Engine.Engine"]["Language"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "int".into())
}

fn load_fonts(data: &Path) -> Option<HudFonts> {
    let list: Vec<FontInfo> =
        serde_json::from_slice(&std::fs::read(data.join("fonts").join("HPBase.json")).ok()?).ok()?;
    let find = |path: &str| list.iter().find(|f| f.path.eq_ignore_ascii_case(path)).cloned().map(Font::new);
    let (message, small) = HudFonts::names(&language(data));
    Some(HudFonts { message: find(message)?, small: find(small)?, numbers: find(HudFonts::NUMBERS)? })
}

impl HudDraw {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, data: &Path) -> Self {
        let fonts = load_fonts(data);
        if fonts.is_none() {
            eprintln!("HUD: no fonts in the converted data (convert again for subtitles)");
        }
        Self {
            overlay: Overlay::new(gpu, format),
            data: data.to_owned(),
            fonts,
            packages: HashMap::new(),
            images: HashMap::new(),
        }
    }

    fn image(&mut self, gpu: &Gpu, path: &str) -> Option<Image> {
        let key = path.to_ascii_lowercase();
        if let Some(found) = self.images.get(&key) {
            return *found;
        }
        let package = path.split('.').next().unwrap_or_default().to_owned();
        let list = self.packages.entry(package.clone()).or_insert_with(|| {
            std::fs::read(self.data.join("textures").join(format!("{package}.json")))
                .ok()
                .and_then(|b| serde_json::from_slice::<Vec<TextureInfo>>(&b).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|t| (t.path.to_ascii_lowercase(), t))
                .collect()
        });
        let found = list.get(&key).and_then(|t| {
            let img = load_png(&self.data.join(&t.file)).ok()?;
            let (w, h) = (img.width as f32, img.height as f32);
            Some(Image { id: self.overlay.add_image(gpu, &img), w, h, masked: t.masked })
        });
        self.images.insert(key, found);
        found
    }

    /// The HUD for one drawn frame on a screen `size` units big.
    pub fn quads(&mut self, gpu: &Gpu, world: &mut World, size: [f32; 2]) -> Vec<Quad> {
        let fonts = self.fonts.take();
        let tiles = world.hud.draw(size, world.cut_mode, &world.stats, fonts.as_ref(), &mut |path| {
            self.image(gpu, path).map(|i| (i.w, i.h))
        });
        self.fonts = fonts;
        tiles
            .into_iter()
            .filter_map(|t| {
                let i = self.image(gpu, &t.texture)?;
                let [sx, sy, sw, sh] = t.src;
                Some(Quad {
                    image: i.id,
                    dst: [t.x, t.y, t.w, t.h],
                    uv: [sx / i.w, sy / i.h, (sx + sw) / i.w, (sy + sh) / i.h],
                    color: t.color,
                    masked: t.style == Style::Masked || i.masked,
                })
            })
            .collect()
    }
}
