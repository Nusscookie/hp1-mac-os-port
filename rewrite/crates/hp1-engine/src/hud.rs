//! The HUD: cutscene borders and spoken lines, as screen tiles (see `specs/hud/Hud.md`). The
//! renderer draws the tiles; this module only decides what goes where.

use hp1_data::FontInfo;

/// How a tile is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Opaque.
    Normal,
    /// See-through where the texture is transparent.
    Masked,
}

/// A rectangle of a texture drawn onto the screen, in HUD units (window points).
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    /// Texture path, e.g. `HPBase.MedInkFont.Texture16`.
    pub texture: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Source rectangle in texture pixels: x, y, width, height.
    pub src: [f32; 4],
    /// Tint, 0..1.
    pub color: [f32; 4],
    pub style: Style,
}

/// A bitmap font from the converted data.
#[derive(Clone, Debug)]
pub struct Font {
    info: FontInfo,
}

/// A letter: page texture and rectangle.
struct Glyph<'a> {
    texture: &'a str,
    rect: [i32; 4],
}

impl Font {
    pub fn new(info: FontInfo) -> Self {
        Self { info }
    }

    fn find(&self, code: u32) -> Option<Glyph<'_>> {
        let mut index = code as usize;
        for page in &self.info.pages {
            if index < page.chars.len() {
                return Some(Glyph { texture: &page.texture, rect: page.chars[index] });
            }
            index -= page.chars.len();
        }
        None
    }

    /// The letter for `c`; one with no width uses its capital, then a space.
    fn glyph(&self, c: char) -> Option<Glyph<'_>> {
        let code = u32::from(c);
        let usable = |g: &Option<Glyph>| g.as_ref().is_some_and(|g| g.rect[2] != 0);
        let mut g = self.find(code);
        if !usable(&g) && c.is_ascii_lowercase() {
            g = self.find(u32::from(c.to_ascii_uppercase()));
        }
        if !usable(&g) {
            g = self.find(32);
        }
        g
    }

    /// Width and height of `text` on one line.
    pub fn text_size(&self, text: &str) -> (f32, f32) {
        text.chars()
            .filter_map(|c| self.glyph(c))
            .fold((0.0, 0.0), |(w, h), g| (w + g.rect[2] as f32, f32::max(h, g.rect[3] as f32)))
    }

    /// Tiles for `text` starting at (`x`, `y`), wrapped at `clip_w`; following lines start at 0.
    pub fn draw(&self, text: &str, x: f32, y: f32, clip_w: f32, color: [f32; 4], out: &mut Vec<Tile>) {
        let blocks = blocks(text);
        let mut lines: Vec<(Vec<&str>, f32)> = Vec::new();
        let (mut line, mut width, mut height) = (Vec::new(), 0.0, 0.0f32);
        for b in blocks {
            if b == "\n" {
                if !line.is_empty() {
                    lines.push((std::mem::take(&mut line), height));
                }
                (width, height) = (0.0, 0.0);
                continue;
            }
            let (bw, bh) = self.text_size(b);
            if width + bw > clip_w {
                lines.push((std::mem::take(&mut line), height));
                if b.starts_with(' ') {
                    (width, height) = (0.0, 0.0);
                } else {
                    line.push(b);
                    (width, height) = (bw, bh);
                }
            } else {
                line.push(b);
                width += bw;
                height = height.max(bh);
            }
        }
        if !line.is_empty() {
            lines.push((line, height));
        }
        let (mut cx, mut cy) = (x, y);
        for (words, h) in lines {
            for c in words.iter().flat_map(|w| w.chars()) {
                let Some(g) = self.glyph(c) else { continue };
                let [u, v, w, gh] = g.rect.map(|n| n as f32);
                out.push(Tile {
                    texture: g.texture.to_owned(),
                    x: cx,
                    y: cy,
                    w,
                    h: gh,
                    src: [u, v, w, gh],
                    color,
                    style: Style::Masked,
                });
                cx += w;
            }
            cx = 0.0;
            cy += h;
        }
    }
}

/// Words, runs of spaces and line breaks.
fn blocks(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let end = match c {
            '\n' => 1,
            ' ' => rest.find(|c| c != ' ').unwrap_or(rest.len()),
            _ => rest.find([' ', '\n']).unwrap_or(rest.len()),
        };
        out.push(&rest[..end]);
        rest = &rest[end..];
    }
    out
}

/// The two fonts spoken lines use.
pub struct HudFonts {
    pub message: Font,
    pub small: Font,
}

impl HudFonts {
    /// Font names for a language code (`eng`, `int`, `ger`, …), as the original console picks them.
    pub fn names(language: &str) -> (&'static str, &'static str) {
        match language.to_ascii_uppercase().as_str() {
            "ENG" | "INT" | "POL" => ("HPBase.MedInkFont", "HPBase.SmallInkFont"),
            _ => ("HPBase.SmallInkFont", "HPBase.TinyInkFont"),
        }
    }
}

struct Message {
    text: String,
    left: f32,
}

/// The HUD's state.
pub struct Hud {
    /// Height of each cutscene bar.
    border: f32,
    message: Option<Message>,
    /// `ToggleDialog`: whether the text of spoken lines is shown.
    pub show_dialog: bool,
}

impl Default for Hud {
    fn default() -> Self {
        Self { border: 0.0, message: None, show_dialog: true }
    }
}

/// The texture whose first texel fills the cutscene bars.
pub const BORDER_TEXTURE: &str = "HPBase.Icons.leftPanel";

impl Hud {
    /// A spoken line to show for `seconds`; it replaces the one showing.
    pub fn show_line(&mut self, text: &str, seconds: f32) {
        self.message = Some(Message { text: text.to_owned(), left: seconds });
    }

    /// The line showing, if any.
    pub fn line(&self) -> Option<&str> {
        self.message.as_ref().map(|m| m.text.as_str())
    }

    /// One game tick.
    pub fn tick(&mut self, dt: f32) {
        if let Some(m) = &mut self.message {
            m.left -= dt;
            if m.left < 0.0 {
                self.message = None;
            }
        }
    }

    /// One drawn frame on a `w` × `h` screen: moves the bars and returns what to draw.
    pub fn draw(&mut self, w: f32, h: f32, cut_mode: bool, fonts: Option<&HudFonts>) -> Vec<Tile> {
        let full = h / 8.0;
        if cut_mode || self.message.is_some() {
            if self.border < full {
                self.border += (full - self.border) / 5.0;
            }
        } else if self.border > 0.0 {
            self.border -= self.border / 5.0;
        }
        let mut out = Vec::new();
        for y in [0.0, h - self.border] {
            out.push(Tile {
                texture: BORDER_TEXTURE.to_owned(),
                x: 0.0,
                y,
                w,
                h: self.border,
                src: [0.0, 0.0, 1.0, 1.0],
                color: [1.0; 4],
                style: Style::Normal,
            });
        }
        if let (Some(m), Some(fonts), true) = (&self.message, fonts, self.show_dialog)
            && !m.text.is_empty()
        {
            let (tw, th) = fonts.message.text_size(&m.text);
            let x = if tw < w - 16.0 { ((w - tw) / 2.0).trunc() } else { 0.0 };
            let lines = ((tw + 90.0) / w).trunc() + 1.0;
            let fit = if th > 0.0 { (full / th).trunc() } else { 0.0 };
            let font = if lines > fit { &fonts.small } else { &fonts.message };
            let blue = [127.0 / 255.0, 127.0 / 255.0, 1.0, 1.0];
            font.draw(&m.text, x, h - self.border + 1.0, w, blue, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hp1_data::FontPage;

    /// Letters 8 wide and 10 high (lower case missing), 32 per page.
    fn font() -> Font {
        let page = |first: u32| FontPage {
            texture: format!("Test.Page{first}"),
            chars: (first..first + 32)
                .map(|c| if (97..=122).contains(&c) { [0, 0, 0, 0] } else { [c as i32, 0, 8, 10] })
                .collect(),
        };
        Font::new(FontInfo {
            path: "Test.Font".into(),
            pages: (0..4).map(|p| page(p * 32)).collect(),
            chars_per_page: 32,
        })
    }

    #[test]
    fn letters_fall_back_to_capitals_and_wrap_at_the_screen_edge() {
        let f = font();
        assert_eq!(f.text_size("ab"), (16.0, 10.0));
        let mut out = Vec::new();
        // 80 wide: "AAAA BBBB " is 80, "CCCC" would make 112.
        f.draw("aaaa bbbb cccc", 4.0, 0.0, 80.0, [1.0; 4], &mut out);
        let at: Vec<(f32, f32)> = out.iter().map(|t| (t.x, t.y)).collect();
        assert_eq!(at[0], (4.0, 0.0));
        // The wrapped line starts at the left edge.
        assert_eq!(at[10], (0.0, 10.0));
        assert_eq!(out.len(), 14);
        assert_eq!(out[10].src[0], 'C' as i32 as f32);
        // A run of spaces at the start of a wrapped line is dropped.
        out.clear();
        f.draw("aaaa bbbbb cc", 0.0, 0.0, 80.0, [1.0; 4], &mut out);
        assert_eq!((out[10].x, out[10].y), (0.0, 10.0));
        assert_eq!(out.len(), 12);
    }

    #[test]
    fn bars_slide_in_for_a_line_and_out_after_it() {
        let fonts = HudFonts { message: font(), small: font() };
        let mut hud = Hud::default();
        hud.show_line("HI", 0.5);
        let tiles = hud.draw(640.0, 480.0, false, Some(&fonts));
        // A fifth of the way to 60 (one eighth of 480).
        assert_eq!(tiles[0].h, 12.0);
        // Centred: (640 - 16) / 2.
        assert_eq!((tiles[2].x, tiles[2].y), (312.0, 480.0 - 12.0 + 1.0));
        hud.tick(0.6);
        assert!(hud.line().is_none());
        let tiles = hud.draw(640.0, 480.0, false, Some(&fonts));
        assert!((tiles[0].h - 9.6).abs() < 1e-4);
        assert_eq!(tiles.len(), 2);
    }
}
