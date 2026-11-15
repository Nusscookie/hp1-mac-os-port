//! Meshes: vertex-animated LOD meshes, skeletal meshes and HP1's compressed skeletal animations.

use crate::{Error, ObjRef, Package, Props, Reader, Result, read_object};

#[derive(Clone, Debug, Default)]
pub struct AnimNotify {
    pub time: f32,
    pub function: String,
}

#[derive(Clone, Debug, Default)]
pub struct AnimSeq {
    pub name: String,
    pub group: String,
    pub start_frame: u32,
    pub num_frames: u32,
    pub rate: f32,
    pub notifies: Vec<AnimNotify>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Wedge {
    pub vertex: u16,
    pub u: f32,
    pub v: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Face {
    pub wedges: [u16; 3],
    pub material: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Material {
    pub poly_flags: u32,
    pub texture_index: i32,
}

/// The parts shared by all meshes (UMesh + ULodMesh).
#[derive(Clone, Debug, Default)]
pub struct LodMesh {
    pub props: Props,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    /// Vertex positions for every animation frame, `frame_verts` per frame (vertex meshes only).
    pub verts: Vec<[f32; 3]>,
    pub anim_seqs: Vec<AnimSeq>,
    pub textures: Vec<ObjRef>,
    pub frame_verts: u32,
    pub anim_frames: u32,
    pub scale: [f32; 3],
    pub origin: [f32; 3],
    /// Pitch, yaw, roll.
    pub rot_origin: [i32; 3],
    pub faces: Vec<Face>,
    pub wedges: Vec<Wedge>,
    pub materials: Vec<Material>,
    /// Faces used only to attach things (weapon triangles); not drawn.
    pub special_faces: Vec<Face>,
    pub model_verts: u32,
    pub special_verts: u32,
    /// Maps the LOD vertex order back to the animation frames' order.
    pub remap_anim_verts: Vec<u16>,
}

#[derive(Clone, Debug, Default)]
pub struct RefBone {
    pub name: String,
    pub flags: u32,
    /// Unreal's convention: rotates the opposite way to a standard quaternion (x, y, z, w).
    pub orientation: [f32; 4],
    pub position: [f32; 3],
    pub length: f32,
    pub size: [f32; 3],
    pub num_children: u32,
    pub parent: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BoneWeightRange {
    pub first: u16,
    pub count: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BoneWeight {
    pub point: u16,
    /// 0..=65535 → 0.0..=1.0
    pub weight: u16,
}

#[derive(Clone, Debug, Default)]
pub struct SkeletalMesh {
    pub lod: LodMesh,
    /// Wedges with float texture coordinates (used instead of `lod.wedges` when present).
    pub ext_wedges: Vec<Wedge>,
    /// Bind-pose vertex positions.
    pub points: Vec<[f32; 3]>,
    pub bones: Vec<RefBone>,
    /// Per bone: which entries of `weights`/`local_points` belong to it.
    pub weight_ranges: Vec<BoneWeightRange>,
    pub weights: Vec<BoneWeight>,
    /// One per weight: the point's position in that bone's space.
    pub local_points: Vec<[f32; 3]>,
    pub default_animation: ObjRef,
    pub weapon_bone: u32,
    /// Origin, X, Y and Z axis of the weapon attachment frame.
    pub weapon_adjust: [[f32; 3]; 4],
}

#[derive(Clone, Debug, Default)]
pub struct AnimTrack {
    pub flags: u32,
    pub rotations: Vec<[f32; 4]>,
    pub positions: Vec<[f32; 3]>,
    pub times: Vec<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct AnimMove {
    pub root_speed: [f32; 3],
    pub track_time: f32,
    pub start_bone: u32,
    pub flags: u32,
    pub bone_indices: Vec<u32>,
    /// One per animation bone.
    pub tracks: Vec<AnimTrack>,
}

#[derive(Clone, Debug, Default)]
pub struct AnimBone {
    pub name: String,
    pub flags: u32,
    pub parent: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Animation {
    pub bones: Vec<AnimBone>,
    /// HP1 stores one move per sequence, in the same order as `sequences`.
    pub moves: Vec<AnimMove>,
    pub sequences: Vec<AnimSeq>,
}

fn vec3(r: &mut Reader) -> Result<[f32; 3]> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

fn count(r: &mut Reader, what: &str) -> Result<usize> {
    let n = r.index()?;
    usize::try_from(n)
        .ok()
        .filter(|&n| n <= r.remaining().max(1) * 8)
        .ok_or_else(|| Error::Format(format!("bad {what} count {n}")))
}

/// Reads a "lazy array" (versions after 61 store the file position of its end first) and checks
/// that the array really ends there. `file_offset` is where the object's data starts in the file.
fn lazy<T>(
    r: &mut Reader,
    version: u16,
    file_offset: usize,
    body: impl FnOnce(&mut Reader) -> Result<T>,
) -> Result<T> {
    let skip = if version > 61 { Some(r.u32()? as usize) } else { None };
    let out = body(r)?;
    if let Some(skip) = skip
        && skip != file_offset + r.pos()
    {
        return Err(Error::Format(format!("lazy array ends at {}, expected {skip}", file_offset + r.pos())));
    }
    Ok(out)
}

fn read_seq(pkg: &Package, r: &mut Reader) -> Result<AnimSeq> {
    let name = pkg.read_name(r)?.to_owned();
    let group = pkg.read_name(r)?.to_owned();
    let start_frame = r.u32()?;
    let num_frames = r.u32()?;
    let n = count(r, "notify")?;
    let mut notifies = Vec::with_capacity(n);
    for _ in 0..n {
        let time = r.f32()?;
        let function = pkg.read_name(r)?.to_owned();
        notifies.push(AnimNotify { time, function });
    }
    notifies.sort_by(|a, b| a.time.total_cmp(&b.time));
    let rate = r.f32()?;
    Ok(AnimSeq { name, group, start_frame, num_frames, rate, notifies })
}

fn read_face(r: &mut Reader) -> Result<Face> {
    Ok(Face { wedges: [r.u16()?, r.u16()?, r.u16()?], material: r.u16()? })
}

/// Reads the UMesh and ULodMesh parts and leaves `r` at the subclass data.
fn read_lod_part<'a>(pkg: &'a Package, index: usize) -> Result<(LodMesh, Reader<'a>)> {
    let (props, mut r) = read_object(pkg, index)?;
    let v = pkg.version;
    let off = pkg.exports[index].offset;
    let mut m = LodMesh { props, ..Default::default() };
    // UPrimitive
    m.bounds_min = vec3(&mut r)?;
    m.bounds_max = vec3(&mut r)?;
    r.u8()?; // valid
    r.skip(if v > 61 { 16 } else { 12 })?; // bounding sphere
    // UMesh
    m.verts = lazy(&mut r, v, off, |r| {
        let n = count(r, "vertex")?;
        let mut verts = Vec::with_capacity(n);
        for _ in 0..n {
            let p = r.i32()?;
            // 11 bits x, 11 bits y, 10 bits z, signed.
            verts.push([((p << 21) >> 21) as f32, ((p << 10) >> 21) as f32, (p >> 22) as f32]);
        }
        Ok(verts)
    })?;
    lazy(&mut r, v, off, |r| {
        let n = count(r, "triangle")?;
        r.skip(n * (6 + 6 + 4 + 4)) // old triangle list: unused for LOD meshes
    })?;
    let n = count(&mut r, "sequence")?;
    for _ in 0..n {
        m.anim_seqs.push(read_seq(pkg, &mut r)?);
    }
    lazy(&mut r, v, off, |r| {
        let n = count(r, "connect")?;
        r.skip(n * 8)
    })?;
    r.skip(24 + 1 + if v > 61 { 16 } else { 12 })?; // mesh bounding box and sphere
    lazy(&mut r, v, off, |r| {
        let n = count(r, "vertex link")?;
        r.skip(n * 4)
    })?;
    let n = count(&mut r, "texture")?;
    for _ in 0..n {
        m.textures.push(pkg.read_ref(&mut r)?);
    }
    let n = count(&mut r, "bounding box")?;
    r.skip(n * 25)?;
    let n = count(&mut r, "bounding sphere")?;
    r.skip(n * if v > 61 { 16 } else { 12 })?;
    m.frame_verts = r.u32()?;
    m.anim_frames = r.u32()?;
    r.u32()?; // and-flags
    r.u32()?; // or-flags
    m.scale = vec3(&mut r)?;
    m.origin = vec3(&mut r)?;
    m.rot_origin = [r.i32()?, r.i32()?, r.i32()?];
    r.u32()?; // current poly
    r.u32()?; // current vertex
    if v == 65 {
        r.f32()?;
    } else if v >= 66 {
        let n = count(&mut r, "texture LOD")?;
        r.skip(n * 4)?;
    }
    // ULodMesh
    let n = count(&mut r, "collapse point")?;
    r.skip(n * 2)?;
    let n = count(&mut r, "face level")?;
    r.skip(n * 2)?;
    let n = count(&mut r, "face")?;
    for _ in 0..n {
        m.faces.push(read_face(&mut r)?);
    }
    let n = count(&mut r, "collapse wedge")?;
    r.skip(n * 2)?;
    let n = count(&mut r, "wedge")?;
    for _ in 0..n {
        let vertex = r.u16()?;
        let u = r.u8()? as f32 / 256.0;
        let vv = r.u8()? as f32 / 256.0;
        m.wedges.push(Wedge { vertex, u, v: vv });
    }
    let n = count(&mut r, "material")?;
    for _ in 0..n {
        m.materials.push(Material { poly_flags: r.u32()?, texture_index: r.i32()? });
    }
    let n = count(&mut r, "special face")?;
    for _ in 0..n {
        m.special_faces.push(read_face(&mut r)?);
    }
    m.model_verts = r.u32()?;
    m.special_verts = r.u32()?;
    r.skip(4 * 6)?; // LOD tuning: scale max, hysteresis, strength, min verts, morph, z displace
    let n = count(&mut r, "remap")?;
    for _ in 0..n {
        m.remap_anim_verts.push(r.u16()?);
    }
    r.u32()?; // old frame verts
    Ok((m, r))
}

pub fn read_lod_mesh(pkg: &Package, index: usize) -> Result<LodMesh> {
    read_lod_part(pkg, index).map(|(m, _)| m)
}

pub fn read_skeletal_mesh(pkg: &Package, index: usize) -> Result<SkeletalMesh> {
    let (lod, mut r) = read_lod_part(pkg, index)?;
    let mut m = SkeletalMesh { lod, ..Default::default() };
    let n = count(&mut r, "ext wedge")?;
    for _ in 0..n {
        let vertex = r.u16()?;
        r.u16()?; // flags
        let u = r.f32()?;
        let v = r.f32()?;
        m.ext_wedges.push(Wedge { vertex, u, v });
    }
    let n = count(&mut r, "point")?;
    for _ in 0..n {
        m.points.push(vec3(&mut r)?);
    }
    let n = count(&mut r, "bone")?;
    for _ in 0..n {
        let name = pkg.read_name(&mut r)?.to_owned();
        let flags = r.u32()?;
        let orientation = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
        let position = vec3(&mut r)?;
        let length = r.f32()?;
        let size = vec3(&mut r)?;
        let num_children = r.u32()?;
        let parent = r.u32()?;
        m.bones.push(RefBone { name, flags, orientation, position, length, size, num_children, parent });
    }
    let n = count(&mut r, "bone weight index")?;
    for _ in 0..n {
        let first = r.u16()?;
        let cnt = r.u16()?;
        r.u16()?; // detail A
        r.u16()?; // detail B
        m.weight_ranges.push(BoneWeightRange { first, count: cnt });
    }
    let n = count(&mut r, "bone weight")?;
    for _ in 0..n {
        m.weights.push(BoneWeight { point: r.u16()?, weight: r.u16()? });
    }
    let n = count(&mut r, "local point")?;
    for _ in 0..n {
        m.local_points.push(vec3(&mut r)?);
    }
    r.u32()?; // skeletal depth
    m.default_animation = pkg.read_ref(&mut r)?;
    m.weapon_bone = r.u32()?;
    m.weapon_adjust = [vec3(&mut r)?, vec3(&mut r)?, vec3(&mut r)?, vec3(&mut r)?];
    if r.remaining() != 0 {
        return Err(Error::Format(format!("skeletal mesh: {} bytes left over", r.remaining())));
    }
    Ok(m)
}

/// HP1 (package version 76) animation: track headers per move, the sequence list, then pooled
/// compressed keys (rotations as 3 × i16 stereographic projection, positions as 3 × i16 fractions of
/// a per-track scale, times as u8 deltas in per-track units).
pub fn read_animation(pkg: &Package, index: usize) -> Result<Animation> {
    struct Keys {
        quats: usize,
        positions: usize,
        times: usize,
        pos_scale: f32,
        time_scale: f32,
    }
    let (_, mut r) = read_object(pkg, index)?;
    let mut anim = Animation::default();
    let n = count(&mut r, "animation bone")?;
    for _ in 0..n {
        let name = pkg.read_name(&mut r)?.to_owned();
        anim.bones.push(AnimBone { name, flags: r.u32()?, parent: r.u32()? });
    }
    let mut keys = Vec::new();
    let n = count(&mut r, "move")?;
    for _ in 0..n {
        let mut mv = AnimMove {
            root_speed: vec3(&mut r)?,
            track_time: r.f32()?,
            start_bone: r.u32()?,
            flags: r.u32()?,
            ..Default::default()
        };
        let nb = count(&mut r, "bone index")?;
        for _ in 0..nb {
            mv.bone_indices.push(r.u32()?);
        }
        let nt = count(&mut r, "track")?;
        for _ in 0..nt {
            let flags = r.u32()?;
            keys.push(Keys {
                quats: count(&mut r, "rotation key")?,
                positions: count(&mut r, "position key")?,
                times: count(&mut r, "time key")?,
                pos_scale: r.f32()?,
                time_scale: r.f32()?,
            });
            mv.tracks.push(AnimTrack { flags, ..Default::default() });
        }
        anim.moves.push(mv);
    }
    let n = count(&mut r, "sequence")?;
    for _ in 0..n {
        anim.sequences.push(read_seq(pkg, &mut r)?);
    }
    let frac = |r: &mut Reader| -> Result<f32> { Ok(r.i16()? as f32 / 32767.0) };
    let n = count(&mut r, "rotation pool")?;
    let mut quats = Vec::with_capacity(n);
    for _ in 0..n {
        let (x, y, z) = (frac(&mut r)?, frac(&mut r)?, frac(&mut r)?);
        let d = x * x + y * y + z * z;
        let s = 2.0 / (1.0 + d);
        quats.push([x * s, y * s, z * s, (1.0 - d) / (1.0 + d)]);
    }
    let n = count(&mut r, "position pool")?;
    let mut positions = Vec::with_capacity(n);
    for _ in 0..n {
        positions.push([frac(&mut r)?, frac(&mut r)?, frac(&mut r)?]);
    }
    let n = count(&mut r, "time pool")?;
    let times = r.bytes(n)?.to_vec();
    if r.remaining() != 0 {
        return Err(Error::Format(format!("animation: {} bytes left over", r.remaining())));
    }

    let (mut ki, mut qi, mut pi, mut ti) = (0usize, 0usize, 0usize, 0usize);
    for mv in &mut anim.moves {
        for track in &mut mv.tracks {
            let k = &keys[ki];
            ki += 1;
            if qi + k.quats > quats.len() || pi + k.positions > positions.len() || ti + k.times > times.len()
            {
                return Err(Error::Format("animation key pool is smaller than its tracks".into()));
            }
            track.rotations.extend_from_slice(&quats[qi..qi + k.quats]);
            qi += k.quats;
            track
                .positions
                .extend(positions[pi..pi + k.positions].iter().map(|p| p.map(|c| c * k.pos_scale)));
            pi += k.positions;
            let mut t = 0.0;
            for &dt in &times[ti..ti + k.times] {
                t += dt as f32 * k.time_scale;
                track.times.push(t);
            }
            ti += k.times;
        }
    }
    Ok(anim)
}
