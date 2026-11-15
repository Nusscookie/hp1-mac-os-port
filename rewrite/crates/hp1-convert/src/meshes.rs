//! Skeletal meshes, vertex meshes and animations → our binary formats.

use std::sync::Arc;

use hp1_data::SectionReport;
use hp1_data::mesh::{
    AnimBone, AnimFile, AnimNotify, Bone, Influence, MeshBody, MeshFile, MeshIndexEntry, MeshMaterial,
    MeshTransform, MeshVertex, Sequence, SequenceInfo, Track, Triangle, save_binary,
};
use rayon::prelude::*;
use ue1_read::mesh::{AnimSeq, Face, LodMesh, Wedge, read_animation, read_lod_mesh, read_skeletal_mesh};
use ue1_read::{ObjRef, Package};

use crate::{Converter, Result, file_name, path_in_package, write_json};

/// Unreal's quaternions rotate the other way round; flip to the standard convention.
fn std_quat(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

pub fn anim_file_for(path: &str) -> String {
    let pkg = path.split('.').next().unwrap_or("");
    format!("anims/{pkg}/{}.anim", file_name(path_in_package(path)))
}

pub fn convert_meshes(conv: &Converter) -> Result<SectionReport> {
    let reports: Vec<SectionReport> = conv
        .lib
        .package_names()
        .par_iter()
        .map(|name| match conv.lib.package(name) {
            Ok(pkg) => convert_package(conv, &pkg),
            Err(e) => SectionReport { errors: vec![format!("{name}: {e}")], ..Default::default() },
        })
        .collect();
    let mut total = SectionReport::default();
    for r in reports {
        total.items += r.items;
        total.errors.extend(r.errors);
        total.warnings.extend(r.warnings);
    }
    Ok(total)
}

fn convert_package(conv: &Converter, pkg: &Arc<Package>) -> SectionReport {
    let mut report = SectionReport::default();
    let mut index = Vec::new();
    for (i, e) in pkg.exports.iter().enumerate() {
        if e.size == 0 {
            continue;
        }
        let path = pkg.ref_path(ObjRef::Export(i));
        let class = pkg.class_name(e);
        let result = match class {
            "SkeletalMesh" => convert_skeletal(conv, pkg, i, &path).map(Some),
            "LodMesh" | "Mesh" => convert_vertex(conv, pkg, i, &path).map(Some),
            "Animation" => convert_animation(conv, pkg, i, &path).map(|_| None),
            _ => continue,
        };
        match result {
            Ok(Some(entry)) => {
                report.items += 1;
                index.push(entry);
            }
            Ok(None) => report.items += 1,
            Err(err) => report.errors.push(format!("{path}: {err}")),
        }
    }
    if !index.is_empty() {
        index.sort_by(|a: &MeshIndexEntry, b| a.path.cmp(&b.path));
        let file = conv.out.join("meshes").join(format!("{}.json", pkg.name));
        if let Err(e) = write_json(&file, &index) {
            report.errors.push(format!("{}: {e}", file.display()));
        }
    }
    report
}

fn transform(m: &LodMesh) -> MeshTransform {
    MeshTransform { scale: m.scale, origin: m.origin, rot_origin: m.rot_origin }
}

fn materials(pkg: &Package, m: &LodMesh) -> Vec<MeshMaterial> {
    m.materials
        .iter()
        .map(|mat| {
            let texture = usize::try_from(mat.texture_index)
                .ok()
                .and_then(|t| m.textures.get(t))
                .filter(|r| !r.is_none())
                .map(|r| pkg.ref_path(*r));
            MeshMaterial { slot: mat.texture_index, texture, poly_flags: mat.poly_flags }
        })
        .collect()
}

fn vertices(wedges: &[Wedge]) -> Vec<MeshVertex> {
    wedges.iter().map(|w| MeshVertex { point: w.vertex as u32, uv: [w.u, w.v] }).collect()
}

fn triangles(faces: &[Face], vertex_count: usize) -> Result<Vec<Triangle>> {
    faces
        .iter()
        .map(|f| {
            let v = f.wedges.map(|w| w as u32);
            if v.iter().any(|&w| w as usize >= vertex_count) {
                return Err(format!("face uses wedge {v:?} of {vertex_count}").into());
            }
            Ok(Triangle { vertices: v, material: f.material })
        })
        .collect()
}

fn convert_skeletal(
    conv: &Converter,
    pkg: &Arc<Package>,
    index: usize,
    path: &str,
) -> Result<MeshIndexEntry> {
    let m = read_skeletal_mesh(pkg, index)?;
    let wedges = if m.ext_wedges.is_empty() { &m.lod.wedges } else { &m.ext_wedges };
    let verts = vertices(wedges);
    if let Some(bad) = verts.iter().find(|v| v.point as usize >= m.points.len()) {
        return Err(format!("wedge uses point {} of {}", bad.point, m.points.len()).into());
    }
    let tris = triangles(&m.lod.faces, verts.len())?;
    let special = triangles(&m.lod.special_faces, verts.len()).unwrap_or_default();

    let bones: Vec<Bone> = m
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| Bone {
            name: b.name.clone(),
            parent: if i == 0 || b.parent as usize >= i { -1 } else { b.parent as i32 },
            rotation: std_quat(b.orientation),
            position: b.position,
        })
        .collect();

    let mut influences: Vec<Vec<Influence>> = vec![Vec::new(); m.points.len()];
    for (bone, range) in m.weight_ranges.iter().enumerate().take(bones.len()) {
        let end =
            (range.first as usize + range.count as usize).min(m.weights.len()).min(m.local_points.len());
        for w in range.first as usize..end {
            let weight = m.weights[w];
            if let Some(list) = influences.get_mut(weight.point as usize) {
                list.push(Influence {
                    bone: bone as u16,
                    weight: weight.weight as f32 / 65535.0,
                    local: m.local_points[w],
                });
            }
        }
    }
    for list in &mut influences {
        list.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    }

    let default_animation = (!m.default_animation.is_none()).then(|| pkg.ref_path(m.default_animation));
    let file = format!("meshes/{}/{}.mesh", pkg.name, file_name(path_in_package(path)));
    let entry = MeshIndexEntry {
        path: path.to_owned(),
        file: file.clone(),
        skeletal: true,
        triangles: tris.len(),
        bones: bones.len(),
        animation_file: default_animation.as_deref().map(anim_file_for),
        default_animation: default_animation.clone(),
    };
    let mesh = MeshFile {
        path: path.to_owned(),
        transform: transform(&m.lod),
        bounds_min: m.lod.bounds_min,
        bounds_max: m.lod.bounds_max,
        materials: materials(pkg, &m.lod),
        vertices: verts,
        triangles: tris,
        special_triangles: special,
        body: MeshBody::Skeletal {
            bones,
            points: m.points,
            influences,
            default_animation,
            weapon_bone: m.weapon_bone,
            weapon_adjust: m.weapon_adjust,
        },
    };
    save_binary(&conv.out.join(&file), &mesh)?;
    Ok(entry)
}

fn seq_info(s: &AnimSeq) -> SequenceInfo {
    SequenceInfo {
        name: s.name.clone(),
        group: s.group.clone(),
        rate: s.rate,
        frames: s.num_frames,
        start_frame: s.start_frame,
        notifies: s.notifies.iter().map(|n| AnimNotify { time: n.time, event: n.function.clone() }).collect(),
    }
}

fn convert_vertex(conv: &Converter, pkg: &Arc<Package>, index: usize, path: &str) -> Result<MeshIndexEntry> {
    let m = read_lod_mesh(pkg, index)?;
    let verts = vertices(&m.wedges);
    let tris = triangles(&m.faces, verts.len())?;
    let file = format!("meshes/{}/{}.mesh", pkg.name, file_name(path_in_package(path)));
    let mesh = MeshFile {
        path: path.to_owned(),
        transform: transform(&m),
        bounds_min: m.bounds_min,
        bounds_max: m.bounds_max,
        materials: materials(pkg, &m),
        vertices: verts,
        special_triangles: triangles(&m.special_faces, m.wedges.len()).unwrap_or_default(),
        body: MeshBody::Vertex {
            frame_points: m.frame_verts,
            frames: m.anim_frames,
            positions: m.verts.clone(),
            sequences: m.anim_seqs.iter().map(seq_info).collect(),
        },
        triangles: tris,
    };
    let entry = MeshIndexEntry {
        path: path.to_owned(),
        file: file.clone(),
        skeletal: false,
        triangles: mesh.triangles.len(),
        bones: 0,
        default_animation: None,
        animation_file: None,
    };
    save_binary(&conv.out.join(&file), &mesh)?;
    Ok(entry)
}

fn convert_animation(conv: &Converter, pkg: &Arc<Package>, index: usize, path: &str) -> Result<()> {
    let a = read_animation(pkg, index)?;
    if a.moves.len() != a.sequences.len() {
        return Err(format!("{} moves but {} sequences", a.moves.len(), a.sequences.len()).into());
    }
    let bones = a
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| AnimBone {
            name: b.name.clone(),
            parent: if i == 0 || b.parent as usize >= i { -1 } else { b.parent as i32 },
        })
        .collect();
    let sequences = a
        .moves
        .iter()
        .zip(&a.sequences)
        .map(|(mv, seq)| Sequence {
            info: seq_info(seq),
            duration: if seq.rate > 0.0 { seq.num_frames as f32 / seq.rate } else { mv.track_time },
            root_speed: mv.root_speed,
            tracks: mv
                .tracks
                .iter()
                .map(|t| Track {
                    rotations: t.rotations.iter().map(|q| std_quat(*q)).collect(),
                    positions: t.positions.clone(),
                    times: t.times.clone(),
                })
                .collect(),
        })
        .collect();
    let anim = AnimFile { path: path.to_owned(), bones, sequences };
    save_binary(&conv.out.join(anim_file_for(path)), &anim)?;
    Ok(())
}
