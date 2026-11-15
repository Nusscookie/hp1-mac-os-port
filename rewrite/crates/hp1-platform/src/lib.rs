//! Window, input and (later) audio output, on SDL3. Everything platform-specific lives here so the
//! game code stays the same on desktop and mobile.

pub use sdl3;

use sdl3::video::Window;

pub struct Platform {
    pub sdl: sdl3::Sdl,
    pub video: sdl3::VideoSubsystem,
    pub window: Window,
    pub events: sdl3::EventPump,
}

impl Platform {
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, String> {
        let sdl = sdl3::init().map_err(|e| e.to_string())?;
        let video = sdl.video().map_err(|e| e.to_string())?;
        let window = video
            .window(title, width, height)
            .resizable()
            .high_pixel_density()
            .build()
            .map_err(|e| e.to_string())?;
        let events = sdl.event_pump().map_err(|e| e.to_string())?;
        Ok(Self { sdl, video, window, events })
    }

    /// The drawable size in pixels (larger than the window size on high-density screens).
    pub fn pixel_size(&self) -> (u32, u32) {
        self.window.size_in_pixels()
    }

    pub fn set_mouse_captured(&self, on: bool) {
        self.sdl.mouse().set_relative_mouse_mode(&self.window, on);
    }
}
