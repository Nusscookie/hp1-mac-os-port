//! A tiny software renderer for checking converted meshes and animations without a GPU.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use glam::{Mat4, Vec2, Vec3};
use hp1_data::TextureInfo;
use hp1_data::mesh::{AnimFile, MeshBody, MeshFile, MeshIndexEntry, load_binary};
use hp1_engine::anim::{bone_links, bone_matrices, mesh_to_object, sample, skin};

struct Image {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

fn load_png(path: &Path) -> Result<Image> {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
    let mut reader = dec.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().context("png size")?];
    let info = reader.next_frame(&mut buf)?;
    if info.color_type != png::ColorType::Rgba {
        bail!("{} is not RGBA", path.display());
    }
    buf.truncate(info.buffer_size());
    Ok(Image { w: info.width, h: info.height, rgba: buf })
}

fn read_json_dir<T: for<'de> serde::Deserialize<'de>>(dir: &Path) -> Result<Vec<T>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "json") {
            let list: Vec<T> = serde_json::from_slice(&std::fs::read(&p)?)?;
            out.extend(list);
        }
    }
    Ok(out)
}

pub fn render_mesh(data: &Path, mesh_path: &str, out: &Path, seq: Option<&str>, time: f32) -> Result<()> {
    let meshes: Vec<MeshIndexEntry> = read_json_dir(&data.join("meshes"))?;
    let entry = meshes
        .iter()
        .find(|m| m.path.eq_ignore_ascii_case(mesh_path) || m.path.rsplit('.').next() == Some(mesh_path))
        .ok_or_else(|| anyhow!("mesh {mesh_path} not found"))?;
    let mesh: MeshFile = load_binary(&data.join(&entry.file))?;

    let points: Vec<Vec3> = match &mesh.body {
        MeshBody::Skeletal { bones, points, influences, .. } => {
            let anim = match &entry.animation_file {
                Some(f) => Some(load_binary::<AnimFile>(&data.join(f))?),
                None => None,
            };
            match (anim, seq) {
                (Some(anim), Some(name)) => {
                    let s = anim
                        .sequences
                        .iter()
                        .find(|s| s.info.name.eq_ignore_ascii_case(name))
                        .ok_or_else(|| {
                            let names: Vec<_> = anim.sequences.iter().map(|s| s.info.name.as_str()).collect();
                            anyhow!("sequence {name} not found; has {}", names.join(", "))
                        })?;
                    let pose = sample(bones, &bone_links(bones, &anim), s, time * s.duration);
                    skin(influences, &bone_matrices(bones, &pose))
                }
                _ => points.iter().map(|p| Vec3::from(*p)).collect(),
            }
        }
        MeshBody::Vertex { positions, frame_points, .. } => {
            positions.iter().take(*frame_points as usize).map(|p| Vec3::from(*p)).collect()
        }
    };
    let to_obj = mesh_to_object(&mesh.transform);
    let pts: Vec<Vec3> = points.iter().map(|p| to_obj.transform_point3(*p)).collect();

    // Textures by material.
    let textures: Vec<TextureInfo> = read_json_dir(&data.join("textures"))?;
    let tex_by_path: HashMap<String, &TextureInfo> =
        textures.iter().map(|t| (t.path.to_ascii_lowercase(), t)).collect();
    let mut images = Vec::new();
    for m in &mesh.materials {
        let img = m
            .texture
            .as_ref()
            .and_then(|p| tex_by_path.get(&p.to_ascii_lowercase()))
            .and_then(|t| load_png(&data.join(&t.file)).ok());
        images.push(img);
    }

    // Front view: the game's X is forward, Y right, Z up. Look from the front (+X) at the model.
    let view = Mat4::from_cols_array(&[
        0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let vp: Vec<Vec3> = pts.iter().map(|p| view.transform_point3(*p)).collect();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &vp {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let (w, h) = (512u32, 512u32);
    let size = (hi - lo).truncate().max_element().max(1.0);
    let scale = (w as f32 * 0.9) / size;
    let center = (lo + hi) * 0.5;
    let screen = |p: Vec3| -> Vec3 {
        Vec3::new((p.x - center.x) * scale + w as f32 / 2.0, (center.y - p.y) * scale + h as f32 / 2.0, p.z)
    };

    let mut color = vec![40u8; (w * h * 4) as usize];
    for px in color.chunks_mut(4) {
        px.copy_from_slice(&[48, 52, 60, 255]);
    }
    let mut depth = vec![f32::MIN; (w * h) as usize];
    let light = Vec3::new(0.4, 0.5, 0.75).normalize();
    for tri in &mesh.triangles {
        let vs = tri.vertices.map(|v| mesh.vertices[v as usize]);
        let p = vs.map(|v| vp[v.point as usize]);
        let s = p.map(screen);
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        let shade = 0.35 + 0.65 * n.dot(light).abs();
        let img = images.get(tri.material as usize).and_then(|i| i.as_ref());
        let (minx, maxx) =
            (s.iter().map(|v| v.x).fold(f32::MAX, f32::min), s.iter().map(|v| v.x).fold(f32::MIN, f32::max));
        let (miny, maxy) =
            (s.iter().map(|v| v.y).fold(f32::MAX, f32::min), s.iter().map(|v| v.y).fold(f32::MIN, f32::max));
        let area = (s[1].x - s[0].x) * (s[2].y - s[0].y) - (s[2].x - s[0].x) * (s[1].y - s[0].y);
        if area.abs() < 1e-6 {
            continue;
        }
        for y in (miny.max(0.0) as u32)..=(maxy.min(h as f32 - 1.0) as u32) {
            for x in (minx.max(0.0) as u32)..=(maxx.min(w as f32 - 1.0) as u32) {
                let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let e = |a: Vec3, b: Vec3| (b.x - a.x) * (q.y - a.y) - (q.x - a.x) * (b.y - a.y);
                let b0 = e(s[1], s[2]) / area;
                let b1 = e(s[2], s[0]) / area;
                let b2 = e(s[0], s[1]) / area;
                if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                    continue;
                }
                let z = b0 * s[0].z + b1 * s[1].z + b2 * s[2].z;
                let di = (y * w + x) as usize;
                if z <= depth[di] {
                    continue;
                }
                let base = match img {
                    Some(img) => {
                        let uv =
                            Vec2::from(vs[0].uv) * b0 + Vec2::from(vs[1].uv) * b1 + Vec2::from(vs[2].uv) * b2;
                        let tx = ((uv.x.rem_euclid(1.0)) * img.w as f32) as u32 % img.w;
                        let ty = ((uv.y.rem_euclid(1.0)) * img.h as f32) as u32 % img.h;
                        let i = ((ty * img.w + tx) * 4) as usize;
                        if img.rgba[i + 3] < 128 {
                            continue;
                        }
                        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
                    }
                    None => [200, 200, 200],
                };
                depth[di] = z;
                let c = &mut color[di * 4..di * 4 + 3];
                for k in 0..3 {
                    c[k] = (base[k] as f32 * shade).min(255.0) as u8;
                }
            }
        }
    }
    hp1_convert::write_png(out, w, h, &color).map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

/// Packs a level's light maps into one image (rows of maps, 2× scaled, ×2 brightness like the game).
pub fn light_maps(data: &Path, map: &str, out: &Path) -> Result<()> {
    let level: hp1_data::level::LevelFile = load_binary(&data.join("levels").join(format!("{map}.level")))?;
    let classes: hp1_data::ClassTable = serde_json::from_slice(&std::fs::read(data.join("classes.json"))?)?;
    let t = std::time::Instant::now();
    let maps = hp1_engine::lighting::build_level_light_maps(&level, &classes);
    let built = maps.iter().flatten().count();
    println!("{built} of {} light maps built in {:.2?}", maps.len(), t.elapsed());
    let width = 2048u32;
    let (mut x, mut y, mut row_h) = (0u32, 0u32, 0u32);
    let mut placed = Vec::new();
    for m in maps.iter().flatten() {
        let (w, h) = (m.width * 2 + 2, m.height * 2 + 2);
        if x + w > width {
            x = 0;
            y += row_h;
            row_h = 0;
        }
        placed.push((m, x, y));
        x += w;
        row_h = row_h.max(h);
    }
    let height = (y + row_h).max(1);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (m, ox, oy) in placed {
        for ty in 0..m.height * 2 {
            for tx in 0..m.width * 2 {
                let c = m.texels[((ty / 2) * m.width + tx / 2) as usize];
                let i = (((oy + ty) * width + ox + tx) * 4) as usize;
                for k in 0..3 {
                    rgba[i + k] = (c[k] * 2.0 * 255.0).min(255.0) as u8;
                }
                rgba[i + 3] = 255;
            }
        }
    }
    hp1_convert::write_png(out, width, height, &rgba).map_err(|e| anyhow!("{e}"))?;
    Ok(())
}
