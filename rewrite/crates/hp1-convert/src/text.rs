//! Localized text and settings files (`.int`, `.eng`, `.ger`, `.fre`, `.ini`, …) → JSON.

use std::collections::BTreeMap;
use std::path::Path;

use hp1_data::{SectionReport, TextFile, TextValue};

use crate::{Converter, Result, write_json};

const TEXT_EXTENSIONS: &[&str] = &["int", "eng", "ger", "fre", "hun", "ita", "spa", "ini"];

pub fn convert_text(conv: &Converter, game: &Path) -> Result<SectionReport> {
    let mut report = SectionReport::default();
    let dir = game.join("System");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if !path.is_file() || !TEXT_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        // Our own settings files from the Mac port are not game data.
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
        if file.to_ascii_lowercase().starts_with("se-") {
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                let parsed = parse_ini(&decode(&bytes));
                write_json(
                    &conv.out.join("text").join(format!("{}.json", file.to_ascii_lowercase())),
                    &parsed,
                )?;
                report.items += 1;
            }
            Err(e) => report.errors.push(format!("{file}: {e}")),
        }
    }
    Ok(report)
}

/// UTF-16 with a byte-order mark, otherwise Latin-1.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        ue1_read::latin1(bytes)
    }
}

pub fn parse_ini(text: &str) -> TextFile {
    let mut out: TextFile = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim_start();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_owned();
            out.entry(section.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim().to_owned();
        let value = value.to_owned();
        let sec = out.entry(section.clone()).or_default();
        match sec.get_mut(&key) {
            None => {
                sec.insert(key, TextValue::One(value));
            }
            Some(TextValue::One(first)) => {
                let first = std::mem::take(first);
                sec.insert(key, TextValue::Many(vec![first, value]));
            }
            Some(TextValue::Many(list)) => list.push(value),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_sections_and_repeats() {
        let f = parse_ini("[a]\r\nx=1\r\n; note\r\ny=hello=world\r\nx=2\r\n[b]\r\nz=\r\n");
        assert_eq!(f["a"]["x"], TextValue::Many(vec!["1".into(), "2".into()]));
        assert_eq!(f["a"]["y"], TextValue::One("hello=world".into()));
        assert_eq!(f["b"]["z"], TextValue::One(String::new()));
        assert_eq!(hp1_data::text_lookup(&f, "A", "Y"), Some("hello=world"));
    }
}
