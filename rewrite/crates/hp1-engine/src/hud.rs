//! The HUD: cutscene borders, spoken lines, the bean counter and health, as screen tiles (see
//! `specs/hud/Hud.md` and `specs/props/Pickups.md`). The renderer draws the tiles; this module
//! only decides what goes where.

use hp1_data::{ClassTable, DataValue, FontInfo};

use crate::world::Stats;

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

/// The full bar, with the empty one over its top part as health goes down.
fn draw_health(stats: &Stats, size_of: &mut SizeOf, out: &mut Vec<Tile>) {
    icon(HEALTH_FULL, 0.0, 0.0, size_of, out);
    if let Some((w, h)) = size_of(HEALTH_EMPTY) {
        let shown = (h - 50.0) * (1.0 - stats.health_part()) + 25.0;
        out.push(Tile {
            texture: HEALTH_EMPTY.to_owned(),
            x: 0.0,
            y: 0.0,
            w,
            h: shown,
            src: [0.0, 0.0, w, shown],
            color: [1.0; 4],
            style: Style::Normal,
        });
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

/// The fonts the HUD uses: two for spoken lines, one for the counters.
pub struct HudFonts {
    pub message: Font,
    pub small: Font,
    /// The font set up for the whole HUD (`MedInkFont`), which the counters' numbers use.
    pub numbers: Font,
    /// The console's big font (lesson popups).
    pub big: Font,
}

impl HudFonts {
    /// Font names for a language code (`eng`, `int`, `ger`, …), as the original console picks them.
    pub fn names(language: &str) -> (&'static str, &'static str) {
        match language.to_ascii_uppercase().as_str() {
            "ENG" | "INT" | "POL" => ("HPBase.MedInkFont", "HPBase.SmallInkFont"),
            _ => ("HPBase.SmallInkFont", "HPBase.TinyInkFont"),
        }
    }

    /// The console's big font for a language code.
    pub fn big_name(language: &str) -> &'static str {
        match language.to_ascii_uppercase().as_str() {
            "ENG" | "INT" | "POL" => "HPBase.HugeInkFont",
            _ => "HPBase.BigInkFont",
        }
    }

    /// The HUD's own font (Asian and Thai text used other fonts; not done yet).
    pub const NUMBERS: &'static str = "HPBase.MedInkFont";
}

struct Message {
    text: String,
    left: f32,
}

/// Where a counter is while hidden: above the screen.
const HIDDEN: f32 = -128.0;

/// A counter that slides down from above the screen when shown (only beans so far).
struct Counter {
    /// Pixels in from the right edge.
    x_offset: f32,
    /// Top edge while shown.
    y_final: f32,
    image: String,
    /// Bean piles, one more per three beans above the first three.
    piles: Vec<String>,
    y: f32,
    shown: f32,
}

impl Counter {
    fn beans(classes: &ClassTable) -> Self {
        let d = classes.effective_defaults("HPMenu.BeansHudItem");
        let f = |k: &str, or: f32| d.get(k).and_then(DataValue::as_f32).unwrap_or(or);
        let t = |k: &str| d.get(k).and_then(DataValue::as_str).unwrap_or("").to_owned();
        let piles = (0..4)
            .map(|i| t(&if i == 0 { "BeanTexture".to_owned() } else { format!("BeanTexture[{i}]") }))
            .filter(|p| !p.is_empty())
            .collect();
        Self {
            x_offset: f("xOffset", 160.0),
            y_final: f("yFinalOffset", 4.0),
            image: t("Image"),
            piles,
            y: f("yOffset", HIDDEN),
            shown: 0.0,
        }
    }

    fn tick(&mut self, dt: f32) {
        if self.shown > 0.0 {
            self.shown -= dt;
            if self.y < self.y_final {
                self.y += (self.y_final - self.y) / 5.0;
            }
        } else if self.y > HIDDEN {
            self.y += (HIDDEN - self.y) / 5.0;
        }
    }
}

/// Texture sizes, for the HUD's icons (drawn at their own size).
pub type SizeOf<'a> = dyn FnMut(&str) -> Option<(f32, f32)> + 'a;

/// An icon at its own size.
fn icon(texture: &str, x: f32, y: f32, size_of: &mut SizeOf, out: &mut Vec<Tile>) -> Option<(f32, f32)> {
    let (w, h) = size_of(texture)?;
    out.push(Tile {
        texture: texture.to_owned(),
        x,
        y,
        w,
        h,
        src: [0.0, 0.0, w, h],
        color: [1.0; 4],
        style: Style::Normal,
    });
    Some((w, h))
}

/// The health bar's two pictures.
pub const HEALTH_FULL: &str = "HPBase.Icons.HarryBarFull";
pub const HEALTH_EMPTY: &str = "HPBase.Icons.HarryBarEmpty";

/// The HUD's state.
pub struct Hud {
    /// Height of each cutscene bar.
    border: f32,
    message: Option<Message>,
    /// `ToggleDialog`: whether the text of spoken lines is shown.
    pub show_dialog: bool,
    beans: Counter,
    /// The countdown bar (spell lessons).
    pub countdown: Option<Countdown>,
    /// A popup drawn over everything, even in cutscene mode.
    pub popup: Option<Popup>,
    /// Harry's lesson grade (`LessonScore`; None = not graded yet) and pass mark (`LessonPass`).
    pub lesson_score: Option<f32>,
    pub lesson_pass: f32,
    /// The grade counting up: time left and total.
    pub score_count: Option<(f32, f32)>,
}

pub const TIMER_FULL: &str = "HPBase.Icons.TimerBarFull";
pub const TIMER_EMPTY: &str = "HPBase.Icons.TimerBarEmpty";

/// The HUD countdown (`StartCountdown`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Countdown {
    pub left: f32,
    pub start: f32,
    /// Time left at the last tick sound.
    last_tick: f32,
}

impl Countdown {
    pub fn new(seconds: f32) -> Self {
        Self { left: seconds, start: seconds, last_tick: 0.0 }
    }
}

/// The lesson popups, with their texts (from `Pickup`).
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    /// "Grade: N%" and "Pass Mark: M%".
    Grade { grade: String, pass: String },
    /// "Lesson Score R/4" and the house points won.
    Final { title: String, rounds: usize, points: String },
}

impl Default for Hud {
    fn default() -> Self {
        Self::new(&ClassTable::default())
    }
}

/// The texture whose first texel fills the cutscene bars.
pub const BORDER_TEXTURE: &str = "HPBase.Icons.leftPanel";

impl Hud {
    /// The HUD with its counters' settings from the converted classes.
    pub fn new(classes: &ClassTable) -> Self {
        Self {
            border: 0.0,
            message: None,
            show_dialog: true,
            beans: Counter::beans(classes),
            countdown: None,
            popup: None,
            lesson_score: None,
            lesson_pass: 0.0,
            score_count: None,
        }
    }

    /// Shows the bean counter for 5 s.
    pub fn show_beans(&mut self) {
        self.beans.shown = 5.0;
    }

    /// A spoken line to show for `seconds`; it replaces the one showing.
    pub fn show_line(&mut self, text: &str, seconds: f32) {
        self.message = Some(Message { text: text.to_owned(), left: seconds });
    }

    /// The line showing, if any.
    pub fn line(&self) -> Option<&str> {
        self.message.as_ref().map(|m| m.text.as_str())
    }

    /// One game tick; gives a sound to play from Harry (at half volume), if any.
    pub fn tick(&mut self, dt: f32, cut_mode: bool) -> Option<&'static str> {
        self.beans.tick(dt);
        if let Some((left, _)) = &mut self.score_count {
            *left -= dt;
            if *left <= 0.0 {
                self.score_count = None;
            }
        }
        let mut sound = None;
        if let Some(c) = &mut self.countdown {
            c.left = (c.left - dt).max(0.0);
            // The original plays the ticks while drawing the bar, which it does outside cutscene
            // mode only.
            if !cut_mode && (c.last_tick - c.left).abs() > 1.0 {
                c.last_tick = c.left;
                sound = Some(match c.left {
                    t if t > 30.0 => "HPSounds.menu_sfx.timer_1",
                    t if t > 20.0 => "HPSounds.menu_sfx.timer_2",
                    t if t > 15.0 => "HPSounds.menu_sfx.timer_3",
                    t if t > 10.0 => "HPSounds.menu_sfx.timer_4",
                    t if t > 5.0 => "HPSounds.menu_sfx.timer_5",
                    _ => "HPSounds.menu_sfx.timer_6",
                });
            }
        }
        if let Some(m) = &mut self.message {
            m.left -= dt;
            if m.left < 0.0 {
                self.message = None;
            }
        }
        sound
    }

    /// One drawn frame on a `w` × `h` screen: moves the bars and returns what to draw.
    /// `size_of` gives the size of a texture, for the icons.
    pub fn draw(
        &mut self,
        [w, h]: [f32; 2],
        cut_mode: bool,
        stats: &Stats,
        fonts: Option<&HudFonts>,
        size_of: &mut SizeOf,
    ) -> Vec<Tile> {
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
        self.draw_beans(w, stats, fonts, size_of, &mut out);
        if !cut_mode {
            draw_health(stats, size_of, &mut out);
            self.draw_countdown([w, h], size_of, &mut out);
        }
        if let Some(fonts) = fonts {
            self.draw_popup([w, h], &fonts.big, &mut out);
        }
        out
    }

    fn draw_countdown(&self, [w, h]: [f32; 2], size_of: &mut SizeOf, out: &mut Vec<Tile>) {
        let Some(c) = &self.countdown else { return };
        let Some((fw, fh)) = size_of(TIMER_FULL) else { return };
        let (x, y) = (w - fw - 8.0, h - 160.0);
        icon(TIMER_EMPTY, x, y, size_of, out);
        let part = if c.start > 0.0 { c.left / c.start } else { 0.0 };
        let shown = (fw - 97.0) * part + 97.0;
        out.push(Tile {
            texture: TIMER_FULL.to_owned(),
            x,
            y,
            w: shown,
            h: fh,
            src: [0.0, 0.0, shown, fh],
            color: [1.0; 4],
            style: Style::Normal,
        });
    }

    /// The grade as the popup shows it: whole percent, counting up while the replay runs.
    pub fn shown_grade(&self) -> Option<i32> {
        let score = self.lesson_score?;
        let mut n = (score * 100.0) as i32;
        if let Some((left, max)) = self.score_count
            && max > 0.0
        {
            n = (n as f32 * (max - left) / max) as i32;
        }
        Some(n)
    }

    fn draw_popup(&self, [w, h]: [f32; 2], font: &Font, out: &mut Vec<Tile>) {
        let yellow = [1.0, 1.0, 0.0, 1.0];
        // Centred on the label alone; the number is added after it, as in the original.
        let centred = |label: &str| ((w / 2.0) - font.text_size(label).0 / 2.0).trunc();
        match &self.popup {
            None => {}
            Some(Popup::Grade { grade, pass }) => {
                let pass_text = format!("{pass} {}%", (self.lesson_pass * 100.0) as i32);
                match self.shown_grade().filter(|_| self.lesson_score.is_some_and(|s| s >= 0.0)) {
                    Some(n) => {
                        font.draw(&format!("{grade} {n}%"), centred(grade), h - 80.0, w, yellow, out);
                        font.draw(&pass_text, centred(pass), h - 48.0, w, yellow, out);
                    }
                    None => font.draw(&pass_text, 32.0, h - 48.0, w, yellow, out),
                }
            }
            Some(Popup::Final { title, rounds, points }) => {
                font.draw(&format!("{title} {rounds}/4"), centred(title), (h / 2.0) - 40.0, w, yellow, out);
                font.draw(points, centred(points), h / 2.0, w, yellow, out);
            }
        }
    }

    fn draw_beans(
        &self,
        w: f32,
        stats: &Stats,
        fonts: Option<&HudFonts>,
        size_of: &mut SizeOf,
        out: &mut Vec<Tile>,
    ) {
        let c = &self.beans;
        if c.y <= HIDDEN {
            return;
        }
        let x = w - c.x_offset;
        if let Some((iw, ih)) = icon(&c.image, x, c.y, size_of, out)
            && let Some(fonts) = fonts
        {
            let left = if stats.beans < 10 { 5.0 } else { 10.0 };
            let text = stats.beans.to_string();
            fonts.numbers.draw(
                &text,
                x + iw / 2.0 - left,
                c.y + ih / 2.0 + 5.0,
                w,
                [0.0, 0.0, 0.0, 1.0],
                out,
            );
        }
        let mut n = stats.beans.min(15) - 3;
        for pile in &c.piles {
            if n <= 0 {
                break;
            }
            icon(pile, x, c.y, size_of, out);
            n -= 3;
        }
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
        let fonts = HudFonts { message: font(), small: font(), numbers: font(), big: font() };
        let stats = Stats { beans: 0, health: 50.0, max_health: 50.0, house_points: 0 };
        let mut hud = Hud::default();
        hud.show_line("HI", 0.5);
        let tiles = hud.draw([640.0, 480.0], false, &stats, Some(&fonts), &mut |_| None);
        // A fifth of the way to 60 (one eighth of 480).
        assert_eq!(tiles[0].h, 12.0);
        // Centred: (640 - 16) / 2.
        assert_eq!((tiles[2].x, tiles[2].y), (312.0, 480.0 - 12.0 + 1.0));
        hud.tick(0.6, false);
        assert!(hud.line().is_none());
        let tiles = hud.draw([640.0, 480.0], false, &stats, Some(&fonts), &mut |_| None);
        assert!((tiles[0].h - 9.6).abs() < 1e-4);
        assert_eq!(tiles.len(), 2);
    }

    #[test]
    fn the_bean_counter_slides_in_and_health_shows_outside_cutscenes() {
        let fonts = HudFonts { message: font(), small: font(), numbers: font(), big: font() };
        let mut stats = Stats { beans: 10, health: 25.0, max_health: 50.0, house_points: 0 };
        let mut hud = Hud::default();
        hud.beans.image = "Test.Counter".into();
        hud.beans.piles = (1..=4).map(|i| format!("Test.Beans{i}")).collect();
        let mut size = |t: &str| Some(if t.starts_with("Test.") { (64.0, 32.0) } else { (128.0, 150.0) });
        // Hidden until shown.
        let tiles = hud.draw([640.0, 480.0], false, &stats, Some(&fonts), &mut size);
        assert!(tiles.iter().all(|t| !t.texture.starts_with("Test.")));
        hud.show_beans();
        hud.tick(0.1, false);
        // A fifth of the way from -128 to 4.
        assert!((hud.beans.y - (-128.0 + 132.0 / 5.0)).abs() < 1e-4);
        let tiles = hud.draw([640.0, 480.0], false, &stats, Some(&fonts), &mut size);
        let counter = tiles.iter().find(|t| t.texture == "Test.Counter").unwrap();
        assert_eq!(counter.x, 640.0 - 160.0);
        // "10": two digits, 10 left of the centre; then 10 - 3 beans make 3 piles.
        let digits: Vec<_> = tiles.iter().filter(|t| t.color == [0.0, 0.0, 0.0, 1.0]).collect();
        assert_eq!(digits.len(), 2);
        assert_eq!(digits[0].x, 640.0 - 160.0 + 32.0 - 10.0);
        assert_eq!(tiles.iter().filter(|t| t.texture.starts_with("Test.Beans")).count(), 3);
        // Half health: (150 - 50) * 0.5 + 25 rows of the empty bar.
        let empty = tiles.iter().find(|t| t.texture == HEALTH_EMPTY).unwrap();
        assert_eq!(empty.h, 75.0);
        // No health bar in cutscene mode; the counter goes back up after 5 s.
        stats.beans = 1;
        let tiles = hud.draw([640.0, 480.0], true, &stats, Some(&fonts), &mut size);
        assert!(tiles.iter().all(|t| t.texture != HEALTH_FULL));
        for _ in 0..200 {
            hud.tick(0.05, false);
        }
        assert!(hud.beans.y < -127.0);
    }
}
