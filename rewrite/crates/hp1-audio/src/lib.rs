//! Sound for the rewrite: decoding the converted sound files and mixing everything that plays
//! into one stereo stream. Platform code pulls the mixed samples (see `Mixer::mix`).

use std::sync::Arc;

use glam::Vec3;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// A decoded sound: interleaved samples.
#[derive(Clone, Debug)]
pub struct Clip {
    pub rate: u32,
    pub channels: u16,
    pub samples: Arc<Vec<f32>>,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.rate.max(1) as f32
    }
}

/// Decodes a whole file. `extension` (`"mp2"`, `"wav"`, …) helps pick the format.
pub fn decode(bytes: Vec<u8>, extension: &str) -> Result<Clip, String> {
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(extension);
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("unknown sound format: {e}"))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("no audio track")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("no decoder: {e}"))?;
    let (mut rate, mut channels) = (track.codec_params.sample_rate.unwrap_or(22050), 1u16);
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(_)) | Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                rate = spec.rate;
                channels = spec.channels.count() as u16;
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                samples.extend_from_slice(buf.samples());
            }
            // A damaged frame: skip it, as players do.
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(Clip { rate, channels, samples: Arc::new(samples) })
}

/// Where a sound comes from.
#[derive(Clone, Copy, Debug)]
pub enum Placement {
    /// Plays at full volume in both ears (music, menus).
    Flat,
    /// Plays from a point; fades out with distance and reaches zero at `radius`.
    At { position: Vec3, radius: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceId(u64);

struct Voice {
    id: VoiceId,
    clip: Clip,
    /// Position in source frames.
    pos: f64,
    pitch: f32,
    volume: f32,
    looping: bool,
    placement: Placement,
    music: bool,
}

/// Everything that plays, mixed down to interleaved stereo at `rate`.
pub struct Mixer {
    pub rate: u32,
    voices: Vec<Voice>,
    next_id: u64,
    listener: Vec3,
    /// The listener's right-hand direction (for left/right panning).
    right: Vec3,
    pub sound_volume: f32,
    pub music_volume: f32,
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            voices: Vec::new(),
            next_id: 1,
            listener: Vec3::ZERO,
            right: Vec3::Y,
            sound_volume: 1.0,
            music_volume: 0.7,
        }
    }

    pub fn play(
        &mut self,
        clip: &Clip,
        placement: Placement,
        volume: f32,
        pitch: f32,
        looping: bool,
    ) -> VoiceId {
        let id = VoiceId(self.next_id);
        self.next_id += 1;
        self.voices.push(Voice {
            id,
            clip: clip.clone(),
            pos: 0.0,
            pitch: pitch.max(0.01),
            volume,
            looping,
            placement,
            music: false,
        });
        id
    }

    /// Replaces the current music (looping).
    pub fn play_music(&mut self, clip: &Clip) -> VoiceId {
        self.voices.retain(|v| !v.music);
        let id = self.play(clip, Placement::Flat, 1.0, 1.0, true);
        if let Some(v) = self.voices.last_mut() {
            v.music = true;
        }
        id
    }

    pub fn stop(&mut self, id: VoiceId) {
        self.voices.retain(|v| v.id != id);
    }

    pub fn set_listener(&mut self, position: Vec3, right: Vec3) {
        self.listener = position;
        self.right = right.normalize_or(Vec3::Y);
    }

    pub fn playing(&self) -> usize {
        self.voices.len()
    }

    /// Left and right gain of a voice right now.
    fn gains(&self, v: &Voice) -> (f32, f32) {
        let base = v.volume * if v.music { self.music_volume } else { self.sound_volume };
        match v.placement {
            Placement::Flat => (base, base),
            Placement::At { position, radius } => {
                let to = position - self.listener;
                let d = to.length();
                if d >= radius {
                    return (0.0, 0.0);
                }
                let fade = 1.0 - d / radius;
                let pan = if d > 1.0 { to.dot(self.right) / d } else { 0.0 };
                // Equal-power panning, never fully silent on the far side.
                let angle = (pan * 0.8 + 1.0) * std::f32::consts::FRAC_PI_4;
                (
                    base * fade * angle.cos() * std::f32::consts::SQRT_2,
                    base * fade * angle.sin() * std::f32::consts::SQRT_2,
                )
            }
        }
    }

    /// Mixes the next `out.len() / 2` stereo frames into `out` (overwriting it).
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let rate = self.rate as f64;
        let gains: Vec<(f32, f32)> = self.voices.iter().map(|v| self.gains(v)).collect();
        for (v, (gl, gr)) in self.voices.iter_mut().zip(gains) {
            let frames = v.clip.frames();
            if frames == 0 {
                v.looping = false;
                v.pos = 0.0;
                continue;
            }
            let ch = v.clip.channels.max(1) as usize;
            let step = v.clip.rate as f64 / rate * v.pitch as f64;
            let s = &v.clip.samples;
            for frame in out.as_chunks_mut::<2>().0 {
                let mut i = v.pos as usize;
                if i >= frames {
                    if !v.looping {
                        break;
                    }
                    v.pos -= frames as f64;
                    i = v.pos as usize % frames;
                }
                if gl != 0.0 || gr != 0.0 {
                    // Linear interpolation between neighbouring frames.
                    let j = if i + 1 < frames {
                        i + 1
                    } else if v.looping {
                        0
                    } else {
                        i
                    };
                    let t = (v.pos - i as f64) as f32;
                    let get = |f: usize, c: usize| s[f * ch + c.min(ch - 1)];
                    let l = get(i, 0) + (get(j, 0) - get(i, 0)) * t;
                    let r = get(i, 1) + (get(j, 1) - get(i, 1)) * t;
                    frame[0] += l * gl;
                    frame[1] += r * gr;
                }
                v.pos += step;
            }
        }
        self.voices.retain(|v| v.looping || (v.pos as usize) < v.clip.frames());
        for x in out.iter_mut() {
            // Soft limit so many sounds at once don't crackle.
            *x = x.tanh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize) -> Clip {
        Clip { rate: 100, channels: 1, samples: Arc::new(vec![0.5; frames]) }
    }

    #[test]
    fn one_shot_sound_ends() {
        let mut m = Mixer::new(100);
        m.play(&tone(10), Placement::Flat, 1.0, 1.0, false);
        let mut out = vec![0.0; 40];
        m.mix(&mut out);
        assert!(out[0] > 0.4 && out[0] < 0.5);
        assert_eq!(out[30], 0.0);
        assert_eq!(m.playing(), 0);
    }

    #[test]
    fn far_sounds_are_silent_and_sides_pan() {
        let mut m = Mixer::new(100);
        m.set_listener(Vec3::ZERO, Vec3::Y);
        m.play(
            &tone(100),
            Placement::At { position: Vec3::new(0.0, 50.0, 0.0), radius: 100.0 },
            1.0,
            1.0,
            true,
        );
        let mut out = vec![0.0; 8];
        m.mix(&mut out);
        assert!(out[1] > out[0], "a sound on the right is louder in the right ear");
        m.set_listener(Vec3::new(0.0, 500.0, 0.0), Vec3::Y);
        m.mix(&mut out);
        assert!(out.iter().all(|&x| x == 0.0));
    }
}
