//! Tagged property lists: the "name = value" pairs every ordinary object starts with.
//!
//! Each tag says its name, type and byte size, so a list can be read without knowing the class.
//! Struct values are stored as plain member-by-member bytes; the common math structs are
//! decoded here, others are kept raw until the converter knows their layout.

use crate::{Error, ObjRef, Package, Reader, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropType {
    Byte,
    Int,
    Bool,
    Float,
    Object,
    Name,
    /// Old fixed-size string.
    String,
    Class,
    Array,
    Struct,
    Vector,
    Rotator,
    Str,
    Map,
    FixedArray,
}

impl PropType {
    fn from_bits(bits: u8) -> Result<Self> {
        Ok(match bits {
            1 => PropType::Byte,
            2 => PropType::Int,
            3 => PropType::Bool,
            4 => PropType::Float,
            5 => PropType::Object,
            6 => PropType::Name,
            7 => PropType::String,
            8 => PropType::Class,
            9 => PropType::Array,
            10 => PropType::Struct,
            11 => PropType::Vector,
            12 => PropType::Rotator,
            13 => PropType::Str,
            14 => PropType::Map,
            15 => PropType::FixedArray,
            b => return Err(Error::Format(format!("unknown property type {b}"))),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(u8),
    Int(i32),
    Bool(bool),
    Float(f32),
    Object(ObjRef),
    Name(String),
    Str(String),
    Vector([f32; 3]),
    /// Pitch, yaw, roll in Unreal units (65536 = full turn).
    Rotator([i32; 3]),
    /// R, G, B, A.
    Color([u8; 4]),
    /// A struct this reader doesn't decode yet, with its name.
    Struct(String, Vec<u8>),
    /// Dynamic arrays, maps and anything else kept as raw bytes.
    Raw(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub name: String,
    pub ty: PropType,
    pub array_index: u32,
    pub value: Value,
}

/// Reads tags until the terminating `None` name. `r` must be positioned at the first tag (after
/// any state frame).
pub fn read_properties(pkg: &Package, r: &mut Reader) -> Result<Vec<Property>> {
    let mut props = Vec::new();
    loop {
        let name = pkg.read_name(r)?;
        if name.eq_ignore_ascii_case("None") {
            return Ok(props);
        }
        let name = name.to_owned();
        let info = r.u8()?;
        let ty = PropType::from_bits(info & 0x0f).map_err(|e| Error::Format(format!("{name}: {e}")))?;
        let flag = info & 0x80 != 0;
        let struct_name = if ty == PropType::Struct { Some(pkg.read_name(r)?.to_owned()) } else { None };
        let size = match (info >> 4) & 7 {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 12,
            4 => 16,
            5 => r.u8()? as usize,
            6 => r.u16()? as usize,
            _ => r.u32()? as usize,
        };
        let mut array_index = 0;
        if flag && ty != PropType::Bool {
            let b = r.u8()? as u32;
            array_index = if b & 0xc0 == 0xc0 {
                let rest = r.bytes(3)?;
                ((b & 0x3f) << 24) | (rest[0] as u32) << 16 | (rest[1] as u32) << 8 | rest[2] as u32
            } else if b & 0x80 != 0 {
                ((b & 0x7f) << 8) | r.u8()? as u32
            } else {
                b
            };
        }

        // A bool's value lives in the info byte; it has no data bytes.
        let value = if ty == PropType::Bool {
            Value::Bool(flag)
        } else {
            let raw = r.bytes(size)?;
            decode_value(pkg, ty, struct_name.as_deref(), raw)
                .map_err(|e| Error::Format(format!("property {name}: {e}")))?
        };
        props.push(Property { name, ty, array_index, value });
    }
}

fn decode_value(pkg: &Package, ty: PropType, struct_name: Option<&str>, raw: &[u8]) -> Result<Value> {
    let mut r = Reader::new(raw);
    let value = match ty {
        PropType::Byte if raw.len() == 1 => Value::Byte(r.u8()?),
        PropType::Int if raw.len() == 4 => Value::Int(r.i32()?),
        PropType::Float if raw.len() == 4 => Value::Float(r.f32()?),
        PropType::Object | PropType::Class => Value::Object(pkg.read_ref(&mut r)?),
        PropType::Name => Value::Name(pkg.read_name(&mut r)?.to_owned()),
        PropType::Str => Value::Str(r.string()?),
        PropType::String => {
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            r.skip(raw.len())?;
            Value::Str(crate::latin1(&raw[..end]))
        }
        PropType::Struct => {
            let s = struct_name.unwrap_or_default();
            match (s, raw.len()) {
                ("Vector", 12) => Value::Vector([r.f32()?, r.f32()?, r.f32()?]),
                ("Rotator", 12) => Value::Rotator([r.i32()?, r.i32()?, r.i32()?]),
                ("Color", 4) => Value::Color([r.u8()?, r.u8()?, r.u8()?, r.u8()?]),
                _ => {
                    r.skip(raw.len())?;
                    Value::Struct(s.to_owned(), raw.to_vec())
                }
            }
        }
        PropType::Vector if raw.len() == 12 => Value::Vector([r.f32()?, r.f32()?, r.f32()?]),
        PropType::Rotator if raw.len() == 12 => Value::Rotator([r.i32()?, r.i32()?, r.i32()?]),
        PropType::Byte | PropType::Int | PropType::Float | PropType::Vector | PropType::Rotator => {
            return Err(Error::Format(format!("{ty:?} with unexpected size {}", raw.len())));
        }
        PropType::Bool => unreachable!("bools have no data"),
        PropType::Array | PropType::Map | PropType::FixedArray => {
            r.skip(raw.len())?;
            Value::Raw(raw.to_vec())
        }
    };
    if r.remaining() != 0 {
        return Err(Error::Format(format!(
            "{ty:?} value left {} of {} bytes unread",
            r.remaining(),
            raw.len()
        )));
    }
    Ok(value)
}
