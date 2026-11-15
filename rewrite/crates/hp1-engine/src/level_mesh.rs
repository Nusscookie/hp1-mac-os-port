//! Turns the level's BSP geometry into triangles ready for drawing: positions, texture
//! coordinates and light-map coordinates, grouped by texture and blend style. Light maps are
//! packed into a few large atlas pages.

use std::collections::HashMap;

use glam::Vec3;
use hp1_data::level::LevelFile;

use crate::lighting::LightMapImage;

/// Surface flags stored in the level (the bits this module looks at).
pub mod poly_flags {
    pub const INVISIBLE: u32 = 0x0000_0001;
    pub const MASKED: u32 = 0x0000_0002;
    pub const TRANSLUCENT: u32 = 0x0000_0004;
    pub const MODULATED: u32 = 0x0000_0040;
    pub const FAKE_BACKDROP: u32 = 0x0000_0080;
    pub const TWO_SIDED: u32 = 0x0000_0100;
    pub const UNLIT: u32 = 0x0040_0000;
    pub const PORTAL: u32 = 0x0400_0000;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Style {
    Opaque,
    Masked,
    /// Adds to what is behind.
    Translucent,
    /// Multiplies what is behind.
    Modulated,
    /// Where the sky shows through.
    Sky,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// Light-map coordinates in the batch's atlas page; negative = full bright.
    pub lm_uv: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct Batch {
    pub texture: Option<String>,
    pub style: Style,
    pub lightmap_page: u32,
    pub indices: Vec<u32>,
}

/// An RGBA8 light-map atlas page; colours are stored as `min(light, 1)`.
#[derive(Clone, Debug)]
pub struct LightPage {
    pub size: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct LevelMesh {
    pub vertices: Vec<WorldVertex>,
    pub batches: Vec<Batch>,
    pub light_pages: Vec<LightPage>,
}

/// What the mesh builder needs to know about a texture.
#[derive(Clone, Copy, Debug)]
pub struct TextureSize {
    pub width: u32,
    pub height: u32,
    pub draw_scale: f32,
    pub masked: bool,
}

const PAGE_SIZE: u32 = 2048;

struct Packer {
    pages: Vec<LightPage>,
    x: u32,
    y: u32,
    row: u32,
}

impl Packer {
    /// Places a map with a one-texel border (copies of its edge) and returns (page, x, y) of the
    /// map's first real texel.
    fn place(&mut self, m: &LightMapImage) -> Option<(u32, u32, u32)> {
        let (w, h) = (m.width + 2, m.height + 2);
        if w > PAGE_SIZE || h > PAGE_SIZE {
            return None;
        }
        if self.pages.is_empty() || self.x + w > PAGE_SIZE {
            self.x = 0;
            self.y += self.row;
            self.row = 0;
        }
        if self.pages.is_empty() || self.y + h > PAGE_SIZE {
            self.pages
                .push(LightPage { size: PAGE_SIZE, rgba: vec![255; (PAGE_SIZE * PAGE_SIZE * 4) as usize] });
            self.x = 0;
            self.y = 0;
            self.row = 0;
        }
        let page = self.pages.last_mut()?;
        for ty in 0..h {
            for tx in 0..w {
                let sx = tx.saturating_sub(1).min(m.width - 1);
                let sy = ty.saturating_sub(1).min(m.height - 1);
                let c = m.texels[(sy * m.width + sx) as usize];
                let i = (((self.y + ty) * PAGE_SIZE + self.x + tx) * 4) as usize;
                for k in 0..3 {
                    page.rgba[i + k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
                page.rgba[i + 3] = 255;
            }
        }
        let at = (self.pages.len() as u32 - 1, self.x + 1, self.y + 1);
        self.x += w;
        self.row = self.row.max(h);
        Some(at)
    }
}

pub fn build_level_mesh(
    level: &LevelFile,
    light_maps: &[Option<LightMapImage>],
    texture_size: impl Fn(&str) -> Option<TextureSize>,
) -> LevelMesh {
    use poly_flags::*;
    let model = &level.model;
    let point = |i: i32| usize::try_from(i).ok().and_then(|i| model.points.get(i)).map(|p| Vec3::from(*p));
    let vector = |i: i32| usize::try_from(i).ok().and_then(|i| model.vectors.get(i)).map(|p| Vec3::from(*p));

    let mut packer = Packer { pages: Vec::new(), x: 0, y: 0, row: 0 };
    let placed: Vec<Option<(u32, u32, u32)>> =
        light_maps.iter().map(|m| m.as_ref().and_then(|m| packer.place(m))).collect();

    let mut mesh = LevelMesh::default();
    let mut batch_of: HashMap<(Option<String>, Style, u32), usize> = HashMap::new();
    let mut size_cache: HashMap<String, Option<TextureSize>> = HashMap::new();

    for node in &model.nodes {
        if node.num_vertices < 3 {
            continue;
        }
        let Some(surf) = usize::try_from(node.surf).ok().and_then(|s| model.surfaces.get(s)) else {
            continue;
        };
        let flags = surf.poly_flags;
        if flags & (INVISIBLE | PORTAL) != 0 {
            continue;
        }
        let (Some(base), Some(u_vec), Some(v_vec)) =
            (point(surf.base), vector(surf.texture_u), vector(surf.texture_v))
        else {
            continue;
        };
        let size = surf
            .texture
            .as_ref()
            .and_then(|t| *size_cache.entry(t.clone()).or_insert_with(|| texture_size(t)));
        let tex = size.unwrap_or(TextureSize { width: 64, height: 64, draw_scale: 1.0, masked: false });
        let style = if flags & FAKE_BACKDROP != 0 {
            Style::Sky
        } else if flags & TRANSLUCENT != 0 {
            Style::Translucent
        } else if flags & MODULATED != 0 {
            Style::Modulated
        } else if flags & MASKED != 0 || tex.masked {
            Style::Masked
        } else {
            Style::Opaque
        };

        // Light map placement, unless the surface is drawn full bright.
        let lit = flags & UNLIT == 0 && style != Style::Sky && style != Style::Modulated;
        let lm = usize::try_from(surf.light_map)
            .ok()
            .filter(|_| lit)
            .and_then(|i| Some((model.light_maps.get(i)?, placed.get(i).copied()??)));
        let page = lm.map_or(0, |(_, (p, _, _))| p);

        let key = (surf.texture.clone(), style, page);
        let bi = *batch_of.entry(key).or_insert_with(|| {
            mesh.batches.push(Batch {
                texture: surf.texture.clone(),
                style,
                lightmap_page: page,
                indices: Vec::new(),
            });
            mesh.batches.len() - 1
        });

        let u_dot = u_vec.dot(base);
        let v_dot = v_vec.dot(base);
        let u_pan = u_dot - surf.pan_u as f32;
        let v_pan = v_dot - surf.pan_v as f32;
        let (u_mult, v_mult) = (
            1.0 / (tex.draw_scale * tex.width.max(1) as f32),
            1.0 / (tex.draw_scale * tex.height.max(1) as f32),
        );
        let first = mesh.vertices.len() as u32;
        let mut count = 0u32;
        for k in 0..node.num_vertices as i32 {
            let Some(vert) = usize::try_from(node.vert_pool + k).ok().and_then(|i| model.verts.get(i)) else {
                break;
            };
            let Some(p) = point(vert.point) else { break };
            let (u, v) = (u_vec.dot(p), v_vec.dot(p));
            let lm_uv = match lm {
                Some((l, (_, px, py))) => {
                    let s = (u - (u_dot + l.pan[0] - 0.5 * l.u_scale)) / l.u_scale;
                    let t = (v - (v_dot + l.pan[1] - 0.5 * l.v_scale)) / l.v_scale;
                    [(px as f32 + s) / PAGE_SIZE as f32, (py as f32 + t) / PAGE_SIZE as f32]
                }
                None => [-1.0, -1.0],
            };
            mesh.vertices.push(WorldVertex {
                pos: p.into(),
                uv: [(u - u_pan) * u_mult, (v - v_pan) * v_mult],
                lm_uv,
            });
            count += 1;
        }
        let indices = &mut mesh.batches[bi].indices;
        for i in 2..count {
            indices.extend_from_slice(&[first, first + i - 1, first + i]);
        }
    }
    mesh.batches.retain(|b| !b.indices.is_empty());
    mesh.batches.sort_by_key(|b| b.style);
    mesh.light_pages = packer.pages;
    mesh
}
