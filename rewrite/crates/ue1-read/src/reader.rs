//! A bounds-checked little-endian cursor over a byte slice.

use crate::{Error, Result};

#[derive(Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn at(data: &'a [u8], pos: usize) -> Result<Self> {
        if pos > data.len() {
            return Err(Error::Eof { pos, len: data.len() });
        }
        Ok(Self { data, pos })
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn seek(&mut self, pos: usize) -> Result<()> {
        if pos > self.data.len() {
            return Err(Error::Eof { pos, len: self.data.len() });
        }
        self.pos = pos;
        Ok(())
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        let Some(end) = end else {
            return Err(Error::Eof { pos: self.pos + n, len: self.data.len() });
        };
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.bytes(N)?.try_into().expect("length checked"))
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    /// Unreal's variable-length signed integer ("compact index"): the first byte holds the sign
    /// (0x80), a continue bit (0x40) and 6 value bits; each following byte holds a continue bit
    /// (0x80) and 7 value bits. At most 5 bytes.
    pub fn index(&mut self) -> Result<i32> {
        let start = self.pos;
        let b = self.u8()?;
        let negative = b & 0x80 != 0;
        let mut value = (b & 0x3f) as u32;
        if b & 0x40 != 0 {
            let mut shift = 6;
            loop {
                let b = self.u8()?;
                value |= ((b & 0x7f) as u32) << shift;
                if b & 0x80 == 0 {
                    break;
                }
                shift += 7;
                if shift > 27 {
                    return Err(Error::Format(format!("compact index too long at {start}")));
                }
            }
        }
        let value = value as i32;
        Ok(if negative { value.wrapping_neg() } else { value })
    }

    /// A length-prefixed string (package version 64 and later): compact-index length that
    /// counts the terminating zero, then Latin-1 bytes. A negative length means UTF-16.
    pub fn string(&mut self) -> Result<String> {
        let len = self.index()?;
        if len == 0 {
            return Ok(String::new());
        }
        if len > 0 {
            let raw = self.bytes(len as usize)?;
            Ok(latin1(strip_nul(raw)))
        } else {
            let n = len.unsigned_abs() as usize;
            let raw = self.bytes(n * 2)?;
            let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
            let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
            Ok(String::from_utf16_lossy(&units[..end]))
        }
    }

    /// A zero-terminated Latin-1 string (package versions before 64).
    pub fn cstring(&mut self) -> Result<String> {
        let rest = &self.data[self.pos..];
        let Some(end) = rest.iter().position(|&b| b == 0) else {
            return Err(Error::Eof { pos: self.data.len(), len: self.data.len() });
        };
        let s = latin1(&rest[..end]);
        self.pos += end + 1;
        Ok(s)
    }
}

fn strip_nul(raw: &[u8]) -> &[u8] {
    match raw.iter().position(|&b| b == 0) {
        Some(end) => &raw[..end],
        None => raw,
    }
}

/// Latin-1 maps byte values straight to the first 256 Unicode code points.
pub fn latin1(raw: &[u8]) -> String {
    raw.iter().map(|&b| b as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_index_round_values() {
        let cases: &[(&[u8], i32)] = &[
            (&[0x00], 0),
            (&[0x01], 1),
            (&[0x3f], 63),
            (&[0x81], -1),
            (&[0x40, 0x01], 64),
            (&[0x7f, 0x7f], 63 | (127 << 6)),
            (&[0x40, 0x80, 0x01], 1 << 13),
            (&[0xc0, 0x01], -64),
        ];
        for &(bytes, want) in cases {
            assert_eq!(Reader::new(bytes).index().unwrap(), want, "{bytes:02x?}");
        }
    }

    #[test]
    fn strings() {
        let mut r = Reader::new(&[4, b'a', b'b', b'c', 0, b'x', b'y', 0]);
        assert_eq!(r.string().unwrap(), "abc");
        assert_eq!(r.cstring().unwrap(), "xy");
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn eof_is_an_error() {
        assert!(Reader::new(&[1, 2]).u32().is_err());
        assert!(Reader::new(&[0x40]).index().is_err());
    }
}
