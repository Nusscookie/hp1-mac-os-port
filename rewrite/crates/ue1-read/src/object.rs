//! Reading an ordinary object: optional state frame, tagged properties, then class-specific data.

use crate::{ObjRef, Package, Property, Reader, Result, Value, read_properties};

/// An object's property list with convenience lookups (case-insensitive, array index 0 unless
/// asked). A property that isn't stored has its class default, which callers supply.
#[derive(Clone, Debug, Default)]
pub struct Props(pub Vec<Property>);

impl Props {
    pub fn get_at(&self, name: &str, index: u32) -> Option<&Value> {
        self.0.iter().find(|p| p.array_index == index && p.name.eq_ignore_ascii_case(name)).map(|p| &p.value)
    }
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.get_at(name, 0)
    }
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn byte(&self, name: &str) -> Option<u8> {
        match self.get(name)? {
            Value::Byte(b) => Some(*b),
            _ => None,
        }
    }
    pub fn int(&self, name: &str) -> Option<i32> {
        match self.get(name)? {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn float(&self, name: &str) -> Option<f32> {
        match self.get(name)? {
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn object(&self, name: &str) -> Option<ObjRef> {
        match self.get(name)? {
            Value::Object(r) => Some(*r),
            _ => None,
        }
    }
    pub fn name(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Name(n) | Value::Str(n) => Some(n),
            _ => None,
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &Property> {
        self.0.iter()
    }
}

/// Reads the state frame (if any) and the property list of export `index`, and returns a reader
/// positioned at the class-specific data that follows.
pub fn read_object(pkg: &Package, index: usize) -> Result<(Props, Reader<'_>)> {
    let e = &pkg.exports[index];
    let mut r = Reader::new(pkg.export_data(e));
    pkg.read_state_frame(e, &mut r)?;
    let props = read_properties(pkg, &mut r)?;
    Ok((Props(props), r))
}
