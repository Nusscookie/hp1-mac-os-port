//! Textures and palettes.

use crate::{Error, ObjRef, Package, Props, Reader, Result, read_object};

/// Pixel formats used by UE1 textures (the `Format` property).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFormat {
    /// 8-bit palette indices.
    P8,
    /// 32-bit BGRA (used for light maps in later engines).
    Rgba7,
    R5G6B5,
    /// DXT1 block compression.
    Dxt1,
    Rgb8,
    Bgra8,
    Other(u8),
}

impl TextureFormat {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::P8,
            1 => Self::Rgba7,
            2 => Self::R5G6B5,
            3 => Self::Dxt1,
            4 => Self::Rgb8,
            5 => Self::Bgra8,
            o => Self::Other(o),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Mip {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// One spark of a fire texture: where it sits and how it behaves. The fire itself is simulated
/// by the game, not here.
#[derive(Clone, Copy, Debug)]
pub struct Spark {
    pub kind: u8,
    pub heat: u8,
    pub x: u8,
    pub y: u8,
    pub bytes: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub props: Props,
    pub format: TextureFormat,
    pub mips: Vec<Mip>,
    /// Block-compressed copies, if the texture has them (`bHasComp`).
    pub compressed: Vec<Mip>,
    pub compressed_format: Option<TextureFormat>,
    pub palette: ObjRef,
    /// Only for fire textures.
    pub sparks: Vec<Spark>,
}

fn read_mips(r: &mut Reader, version: u16) -> Result<Vec<Mip>> {
    let count = r.u8()? as usize;
    let mut mips = Vec::with_capacity(count);
    for _ in 0..count {
        if version >= 63 {
            r.u32()?; // offset of the data's end: lets the engine skip mips, not needed here
        }
        let len = r.index()?;
        let len = usize::try_from(len).map_err(|_| Error::Format(format!("negative mip size {len}")))?;
        let data = r.bytes(len)?.to_vec();
        let width = r.u32()?;
        let height = r.u32()?;
        r.u8()?; // UBits
        r.u8()?; // VBits
        mips.push(Mip { width, height, data });
    }
    Ok(mips)
}

pub fn read_texture(pkg: &Package, index: usize) -> Result<Texture> {
    let class = pkg.class_name(&pkg.exports[index]).to_owned();
    let (props, mut r) = read_object(pkg, index)?;
    let mips = read_mips(&mut r, pkg.version)?;
    let (compressed, compressed_format) = if props.bool("bHasComp").unwrap_or(false) {
        (
            read_mips(&mut r, pkg.version)?,
            Some(TextureFormat::from_byte(props.byte("CompFormat").unwrap_or(0))),
        )
    } else {
        (Vec::new(), None)
    };
    let mut sparks = Vec::new();
    if class.eq_ignore_ascii_case("FireTexture") {
        let n = r.index()?.max(0) as usize;
        for _ in 0..n {
            let b = r.bytes(8)?;
            sparks.push(Spark { kind: b[0], heat: b[1], x: b[2], y: b[3], bytes: [b[4], b[5], b[6], b[7]] });
        }
    }
    let format = TextureFormat::from_byte(props.byte("Format").unwrap_or(0));
    let palette = props.object("Palette").unwrap_or_default();
    Ok(Texture { props, format, mips, compressed, compressed_format, palette, sparks })
}

/// 256 colours as RGBA. UE1 palettes store an alpha byte that isn't meaningful for most
/// textures; callers decide transparency (masked textures use index 0 as see-through).
pub fn read_palette(pkg: &Package, index: usize) -> Result<Vec<[u8; 4]>> {
    let (_, mut r) = read_object(pkg, index)?;
    let n = r.index()?;
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n <= 65536)
        .ok_or_else(|| Error::Format("bad palette size".into()))?;
    let mut colors = Vec::with_capacity(n);
    for _ in 0..n {
        let b = r.bytes(4)?;
        colors.push([b[0], b[1], b[2], if pkg.version < 66 { 255 } else { b[3] }]);
    }
    Ok(colors)
}

/// Decodes a mip to RGBA8. `palette` is needed for P8.
pub fn mip_to_rgba(format: TextureFormat, mip: &Mip, palette: Option<&[[u8; 4]]>) -> Result<Vec<u8>> {
    let (w, h) = (mip.width as usize, mip.height as usize);
    let pixels = w * h;
    let need = |n: usize| {
        if mip.data.len() < n {
            Err(Error::Format(format!("mip has {} bytes, {n} needed for {w}x{h} {format:?}", mip.data.len())))
        } else {
            Ok(())
        }
    };
    let mut out = Vec::with_capacity(pixels * 4);
    match format {
        TextureFormat::P8 => {
            need(pixels)?;
            let pal = palette.ok_or_else(|| Error::Format("P8 texture without a palette".into()))?;
            for &i in &mip.data[..pixels] {
                let c = pal.get(i as usize).copied().unwrap_or([0, 0, 0, 255]);
                out.extend_from_slice(&c);
            }
        }
        TextureFormat::Bgra8 | TextureFormat::Rgba7 => {
            need(pixels * 4)?;
            for c in mip.data[..pixels * 4].as_chunks::<4>().0 {
                out.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
            }
        }
        TextureFormat::Rgb8 => {
            need(pixels * 3)?;
            for c in mip.data[..pixels * 3].as_chunks::<3>().0 {
                out.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        TextureFormat::R5G6B5 => {
            need(pixels * 2)?;
            for c in mip.data[..pixels * 2].as_chunks::<2>().0 {
                let [r, g, b] = rgb565(u16::from_le_bytes([c[0], c[1]]));
                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
        TextureFormat::Dxt1 => {
            let bw = w.div_ceil(4);
            let bh = h.div_ceil(4);
            need(bw * bh * 8)?;
            out.resize(pixels * 4, 0);
            for by in 0..bh {
                for bx in 0..bw {
                    let block = &mip.data[(by * bw + bx) * 8..][..8];
                    let texels = decode_dxt1_block(block);
                    for (k, t) in texels.iter().enumerate() {
                        let (x, y) = (bx * 4 + k % 4, by * 4 + k / 4);
                        if x < w && y < h {
                            out[(y * w + x) * 4..][..4].copy_from_slice(t);
                        }
                    }
                }
            }
        }
        TextureFormat::Other(f) => return Err(Error::Format(format!("texture format {f} not supported"))),
    }
    Ok(out)
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

fn decode_dxt1_block(b: &[u8]) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let p0 = rgb565(c0);
    let p1 = rgb565(c1);
    let mix = |a: u8, b: u8, wa: u32, wb: u32| ((a as u32 * wa + b as u32 * wb) / (wa + wb)) as u8;
    let mut colors = [[0u8; 4]; 4];
    colors[0] = [p0[0], p0[1], p0[2], 255];
    colors[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 {
        colors[2] = [mix(p0[0], p1[0], 2, 1), mix(p0[1], p1[1], 2, 1), mix(p0[2], p1[2], 2, 1), 255];
        colors[3] = [mix(p0[0], p1[0], 1, 2), mix(p0[1], p1[1], 1, 2), mix(p0[2], p1[2], 1, 2), 255];
    } else {
        colors[2] = [mix(p0[0], p1[0], 1, 1), mix(p0[1], p1[1], 1, 1), mix(p0[2], p1[2], 1, 1), 255];
        colors[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| colors[((bits >> (i * 2)) & 3) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p8_uses_palette() {
        let mip = Mip { width: 2, height: 1, data: vec![1, 0] };
        let pal = [[1, 2, 3, 4], [10, 20, 30, 40]];
        assert_eq!(mip_to_rgba(TextureFormat::P8, &mip, Some(&pal)).unwrap(), [10, 20, 30, 40, 1, 2, 3, 4]);
    }

    #[test]
    fn dxt1_solid_block() {
        // c0 = pure red, c1 = black, all texels index 0.
        let block = [0x00, 0xf8, 0x00, 0x00, 0, 0, 0, 0];
        let mip = Mip { width: 4, height: 4, data: block.to_vec() };
        let rgba = mip_to_rgba(TextureFormat::Dxt1, &mip, None).unwrap();
        assert!(rgba.chunks(4).all(|p| p == [255, 0, 0, 255]));
    }
}
