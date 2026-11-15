//! Sounds and music: a format name plus the original file bytes (WAV, MP2, …).

use crate::{Error, Package, Props, Result, read_object};

#[derive(Clone, Debug)]
pub struct SoundData {
    pub props: Props,
    /// Lower-case format name as stored, e.g. `wav` or `mp2`.
    pub format: String,
    /// The complete original file (a RIFF WAV, an MPEG audio stream, …).
    pub data: Vec<u8>,
}

/// Reads a `Sound` or `Music` export. Both store the same layout.
pub fn read_sound(pkg: &Package, index: usize) -> Result<SoundData> {
    let (props, mut r) = read_object(pkg, index)?;
    let format = pkg.read_name(&mut r)?.to_ascii_lowercase();
    if pkg.version > 61 {
        r.u32()?; // offset of the data's end, lets the engine skip loading it
    }
    let len = r.index()?;
    let len = usize::try_from(len).map_err(|_| Error::Format(format!("negative sound size {len}")))?;
    let data = r.bytes(len)?.to_vec();
    Ok(SoundData { props, format, data })
}
