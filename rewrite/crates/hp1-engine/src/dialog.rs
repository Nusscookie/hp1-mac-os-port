//! Dialogue lines: their text, their voice, and how long they last
//! (`specs/cutscenes/CutScene.md`, Dialogue).

use crate::world::{Output, World};

/// Where dialogue comes from. The program provides it (text files and voice sounds).
pub trait Lines {
    /// The line's text in the chosen language; empty if it has none.
    fn text(&self, id: &str) -> String;
    /// The voice sound (path for `Output::Voice`) and its length in seconds, if there is one.
    fn voice(&mut self, id: &str) -> Option<(String, f32)>;
}

/// No text and no voices (tests, or data without dialogue).
pub struct NoLines;

impl Lines for NoLines {
    fn text(&self, _id: &str) -> String {
        String::new()
    }
    fn voice(&mut self, _id: &str) -> Option<(String, f32)> {
        None
    }
}

/// Plays dialogue line `id`: its voice, and its text as a message. Returns how long it lasts.
pub fn deliver(w: &mut World, id: &str) -> f32 {
    let mut text = w.lines.text(id);
    let seconds = match w.lines.voice(id) {
        Some((path, length)) => {
            w.outputs.push(Output::Voice { path });
            // A little room between lines.
            length + 0.5
        }
        None => {
            text = format!("*{text}");
            3.0
        }
    };
    // Emote sounds ("…EMOTIV…") have no subtitle.
    if !id.to_ascii_uppercase().contains("EMOTIV") {
        w.hud.show_line(&text, seconds);
        w.outputs.push(Output::Message { text, seconds });
    }
    seconds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behaviours::tests::world_with;

    struct OneVoice;
    impl Lines for OneVoice {
        fn text(&self, id: &str) -> String {
            format!("text of {id}")
        }
        fn voice(&mut self, id: &str) -> Option<(String, f32)> {
            id.eq_ignore_ascii_case("hello").then(|| ("AllDialog.hello".to_owned(), 2.0))
        }
    }

    #[test]
    fn lines_last_as_long_as_their_voice_or_three_seconds() {
        let mut w = world_with(vec![]);
        w.lines = Box::new(OneVoice);
        assert_eq!(deliver(&mut w, "HELLO"), 2.5);
        assert_eq!(deliver(&mut w, "silent"), 3.0);
        assert_eq!(deliver(&mut w, "x_EMOTIVE_1"), 3.0);
        assert_eq!(
            w.outputs,
            vec![
                Output::Voice { path: "AllDialog.hello".into() },
                Output::Message { text: "text of HELLO".into(), seconds: 2.5 },
                Output::Message { text: "*text of silent".into(), seconds: 3.0 },
            ]
        );
    }
}
