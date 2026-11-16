//! Ambient sounds of placed objects and one music track.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use glam::Vec3;
use hp1_audio::{Clip, Mixer, Placement};
use hp1_data::level::LevelFile;
use hp1_data::{AudioInfo, ClassTable, DataValue};
use hp1_engine::props::ActorProps;

pub struct Library {
    data: PathBuf,
    sounds: HashMap<String, AudioInfo>,
    music: HashMap<String, AudioInfo>,
    clips: HashMap<String, Option<Clip>>,
}

fn read_dir(dir: &Path) -> Result<HashMap<String, AudioInfo>> {
    let mut out = HashMap::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "json") {
            let list: Vec<AudioInfo> = serde_json::from_slice(&std::fs::read(&p)?)?;
            out.extend(list.into_iter().map(|a| (a.path.to_ascii_lowercase(), a)));
        }
    }
    Ok(out)
}

impl Library {
    pub fn open(data: &Path) -> Result<Self> {
        Ok(Self {
            data: data.to_owned(),
            sounds: read_dir(&data.join("sounds"))?,
            music: read_dir(&data.join("music"))?,
            clips: HashMap::new(),
        })
    }

    fn clip(&mut self, path: &str, music: bool) -> Option<Clip> {
        let key = path.to_ascii_lowercase();
        if let Some(c) = self.clips.get(&key) {
            return c.clone();
        }
        let table = if music { &self.music } else { &self.sounds };
        // Music can be named by package alone (`Name` for `Name.Name`).
        let info = table.get(&key).or_else(|| table.get(&format!("{key}.{key}")));
        let clip = info.and_then(|a| {
            let bytes = std::fs::read(self.data.join(&a.file)).ok()?;
            hp1_audio::decode(bytes, &a.format).ok()
        });
        self.clips.insert(key, clip.clone());
        clip
    }
}

/// Starts every ambient sound of the level and the chosen music. Returns how many ambient sounds
/// were started and the music's name.
pub fn start(
    lib: &mut Library,
    mixer: &Arc<Mutex<Mixer>>,
    level: &LevelFile,
    classes: &ClassTable,
    music: Option<&str>,
    start: Vec3,
) -> (usize, Option<String>) {
    let mut ambient = Vec::new();
    let mut nearest_song: Option<(f32, String)> = None;
    for a in level.actors.iter().flatten() {
        let p = ActorProps::new(a, classes);
        let loc = Vec3::from(p.vec3("Location"));
        if let Some(path) = p.get("AmbientSound").and_then(DataValue::as_str) {
            let radius = (p.byte("SoundRadius", 32) as f32 + 1.0) * 25.0;
            let volume = p.byte("SoundVolume", 128) as f32 / 255.0;
            let pitch = p.byte("SoundPitch", 64) as f32 / 64.0;
            ambient.push((path.to_owned(), loc, radius, volume, pitch));
        }
        if a.class.ends_with(".MusicEvent")
            && let Some(song) = p.get("Song").and_then(DataValue::as_str)
        {
            let d = loc.distance(start);
            if nearest_song.as_ref().is_none_or(|(best, _)| d < *best) {
                nearest_song = Some((d, song.to_owned()));
            }
        }
    }
    let song = music.map(str::to_owned).or(nearest_song.map(|(_, s)| s));
    let clips: Vec<_> = ambient.iter().map(|(path, ..)| lib.clip(path, false)).collect();
    let music_clip = song.as_deref().and_then(|s| lib.clip(s, true));

    let mut m = mixer.lock().unwrap_or_else(|e| e.into_inner());
    let mut started = 0;
    for ((_, loc, radius, volume, pitch), clip) in ambient.iter().zip(&clips) {
        if let Some(clip) = clip {
            m.play(clip, Placement::At { position: *loc, radius: *radius }, *volume, *pitch, true);
            started += 1;
        }
    }
    if let Some(clip) = &music_clip {
        m.play_music(clip);
    }
    (started, song.filter(|_| music_clip.is_some()))
}
