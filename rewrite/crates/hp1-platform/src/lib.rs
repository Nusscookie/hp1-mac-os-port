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

    /// The window's size in points (pixels on ordinary screens, half the pixels on Retina).
    pub fn point_size(&self) -> (u32, u32) {
        self.window.size()
    }

    pub fn set_mouse_captured(&self, on: bool) {
        self.sdl.mouse().set_relative_mouse_mode(&self.window, on);
    }
}

/// Keeps the sound output running; dropping it stops the sound.
pub struct AudioOutput {
    _stream: sdl3::audio::AudioStreamWithCallback<Feeder>,
    pub rate: u32,
}

/// Produces interleaved stereo samples on the audio thread.
type FillFn = Box<dyn FnMut(&mut [f32]) + Send>;

struct Feeder {
    fill: FillFn,
    buf: Vec<f32>,
}

impl sdl3::audio::AudioCallback<f32> for Feeder {
    fn callback(&mut self, stream: &mut sdl3::audio::AudioStream, requested: i32) {
        // `requested` is in bytes; we send interleaved stereo f32.
        let n = (requested.max(0) as usize / 4) & !1;
        if n == 0 {
            return;
        }
        self.buf.resize(n, 0.0);
        (self.fill)(&mut self.buf);
        let _ = stream.put_data_f32(&self.buf);
    }
}

impl Platform {
    /// Starts stereo sound output at `rate`; `fill` is called from the audio thread to produce
    /// interleaved samples.
    pub fn open_audio(
        &self,
        rate: u32,
        fill: impl FnMut(&mut [f32]) + Send + 'static,
    ) -> Result<AudioOutput, String> {
        let audio = self.sdl.audio().map_err(|e| e.to_string())?;
        let spec = sdl3::audio::AudioSpec {
            freq: Some(rate as i32),
            channels: Some(2),
            format: Some(sdl3::audio::AudioFormat::f32_sys()),
        };
        let stream = audio
            .open_playback_stream(&spec, Feeder { fill: Box::new(fill), buf: Vec::new() })
            .map_err(|e| e.to_string())?;
        stream.resume().map_err(|e| e.to_string())?;
        Ok(AudioOutput { _stream: stream, rate })
    }
}
