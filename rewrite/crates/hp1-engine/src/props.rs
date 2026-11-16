//! Reading a placed object's settings: its own values first, then its class's defaults.

use hp1_data::level::Actor;
use hp1_data::{ClassTable, DataValue, PropMap};

/// One object's settings with the class defaults behind them.
pub struct ActorProps<'a> {
    pub actor: &'a Actor,
    pub defaults: PropMap,
}

impl<'a> ActorProps<'a> {
    pub fn new(actor: &'a Actor, classes: &ClassTable) -> Self {
        Self { actor, defaults: classes.effective_defaults(&actor.class) }
    }

    pub fn get(&self, name: &str) -> Option<&DataValue> {
        self.actor.props.get(name).or_else(|| self.defaults.get(name))
    }
    pub fn f32(&self, name: &str, fallback: f32) -> f32 {
        self.get(name).and_then(DataValue::as_f32).unwrap_or(fallback)
    }
    pub fn int(&self, name: &str, fallback: i64) -> i64 {
        self.get(name).and_then(DataValue::as_i64).unwrap_or(fallback)
    }
    pub fn byte(&self, name: &str, fallback: u8) -> u8 {
        self.int(name, fallback as i64).clamp(0, 255) as u8
    }
    pub fn bool(&self, name: &str) -> bool {
        self.get(name).and_then(DataValue::as_bool).unwrap_or(false)
    }
    pub fn bool_or(&self, name: &str, fallback: bool) -> bool {
        self.get(name).and_then(DataValue::as_bool).unwrap_or(fallback)
    }
    /// A name, text or object setting; empty when unset or `None`.
    pub fn text(&self, name: &str) -> String {
        self.get(name)
            .and_then(DataValue::as_str)
            .filter(|s| !s.eq_ignore_ascii_case("None"))
            .unwrap_or("")
            .to_owned()
    }
    pub fn vec3(&self, name: &str) -> [f32; 3] {
        self.get(name).and_then(DataValue::as_vec3).unwrap_or([0.0; 3])
    }
    pub fn rotator(&self, name: &str) -> [i32; 3] {
        self.get(name).and_then(DataValue::as_rotator).unwrap_or([0; 3])
    }
}
