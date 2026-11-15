//! Reads the Unreal Engine 1 packages (`.u`, `.unr`, `.utx`, `.uax`, `.umx`) on the Harry Potter 1
//! CD (package version 76).
//!
//! Only the converter uses this crate. It reads content (names, the object tables, property
//! values, and later textures, meshes, sounds and levels). It never runs or interprets
//! UnrealScript: script bytecode and script text are skipped.

mod package;
mod props;
mod reader;

pub use package::{Export, Import, ObjRef, Package, StateFrame, object_flags};
pub use props::{PropType, Property, Value, read_properties};
pub use reader::{Reader, latin1};

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// Tried to read past the end of the data.
    Eof {
        pos: usize,
        len: usize,
    },
    /// The bytes don't match the expected layout.
    Format(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Eof { pos, len } => write!(f, "read past end of data (at {pos}, size {len})"),
            Error::Format(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
