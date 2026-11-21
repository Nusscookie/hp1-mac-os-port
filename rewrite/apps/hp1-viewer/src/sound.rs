//! Ambient sounds of placed objects and one music track.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use glam::Vec3;
use hp1_audio::{Clip, Mixer, Placement, VoiceId};
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

    pub fn clip(&mut self, path: &str, music: bool) -> Option<Clip> {
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

/// Plays what the world asked for since the last call.
pub fn play_outputs(
    lib: &mut Library,
    mixer: &Arc<Mutex<Mixer>>,
    outputs: &mut Vec<hp1_engine::world::Output>,
    ambient: &mut HashMap<usize, VoiceId>,
) {
    use hp1_engine::world::Output;
    if outputs.is_empty() {
        return;
    }
    let mut m = mixer.lock().unwrap_or_else(|e| e.into_inner());
    for o in outputs.drain(..) {
        match o {
            Output::Sound { path, at, radius, volume } => {
                if let Some(c) = lib.clip(&path, false) {
                    m.play(&c, Placement::At { position: at, radius }, volume, 1.0, false);
                }
            }
            Output::AmbientStart { actor, path, at, radius, volume } => {
                if let Some(c) = lib.clip(&path, false) {
                    let id = m.play(&c, Placement::At { position: at, radius }, volume, 1.0, true);
                    if let Some(old) = ambient.insert(actor, id) {
                        m.stop(old);
                    }
                }
            }
            Output::AmbientStop { actor } => {
                if let Some(id) = ambient.remove(&actor) {
                    m.stop(id);
                }
            }
            Output::Music { song } => match song.and_then(|s| lib.clip(&s, true)) {
                Some(c) => {
                    m.play_music(&c);
                }
                None => m.stop_music(),
            },
            // A new dialogue voice cuts off the one before.
            Output::Voice { path } => {
                if let Some(c) = lib.clip(&path, false) {
                    let id = m.play(&c, Placement::Flat, 1.0, 1.0, false);
                    if let Some(old) = ambient.insert(VOICE, id) {
                        m.stop(old);
                    }
                }
            }
            Output::Message { .. } | Output::ChangeLevel { .. } | Output::Note(_) => {}
        }
    }
}

/// Where the dialogue voice is kept among the looping sounds.
const VOICE: usize = usize::MAX;

/// Dialogue for the world: text in the game's language, voices from `AllDialog`.
pub struct DialogLines {
    text: HashMap<String, String>,
    sounds: Library,
}

impl DialogLines {
    pub fn open(data: &Path) -> Result<Self> {
        let text_dir = data.join("text");
        // The language the game is set to (`[Engine.Engine] Language`), else the default text.
        let lang = std::fs::read(text_dir.join("default.ini.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v["Engine.Engine"]["Language"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "int".into());
        let read = |ext: &str| -> Option<HashMap<String, String>> {
            let bytes = std::fs::read(text_dir.join(format!("hpdialog.{ext}.json"))).ok()?;
            let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            let all = v.get("all")?.as_object()?;
            Some(
                all.iter()
                    .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.as_str()?.to_owned())))
                    .collect(),
            )
        };
        let text = read(&lang).or_else(|| read("int")).unwrap_or_default();
        Ok(Self { text, sounds: Library::open(data)? })
    }
}

impl hp1_engine::dialog::Lines for DialogLines {
    fn text(&self, id: &str) -> String {
        self.text.get(&id.to_ascii_lowercase()).cloned().unwrap_or_default()
    }

    fn voice(&mut self, id: &str) -> Option<(String, f32)> {
        let path = format!("AllDialog.{id}");
        let clip = self.sounds.clip(&path, false)?;
        Some((path, clip.seconds()))
    }
}
