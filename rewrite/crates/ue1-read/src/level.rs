//! Levels: the actor list, the BSP model (geometry, zones, light maps) and brush polygons.

use crate::{Error, ObjRef, Package, Reader, Result, read_object};

#[derive(Clone, Debug, Default)]
pub struct ReachSpec {
    pub distance: i32,
    pub start: ObjRef,
    pub end: ObjRef,
    pub collision_radius: i32,
    pub collision_height: i32,
    pub flags: i32,
    pub pruned: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Level {
    pub actors: Vec<ObjRef>,
    pub url_map: String,
    pub model: ObjRef,
    pub reach_specs: Vec<ReachSpec>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BspNode {
    pub plane: [f32; 4],
    pub zone_mask: u64,
    pub flags: u8,
    pub vert_pool: i32,
    pub surf: i32,
    pub back: i32,
    pub front: i32,
    pub plane_next: i32,
    pub collision_bound: i32,
    pub render_bound: i32,
    pub zone: [i32; 2],
    pub num_vertices: u8,
    pub leaf: [i32; 2],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BspSurface {
    pub texture: ObjRef,
    pub poly_flags: u32,
    pub base: i32,
    pub normal: i32,
    pub texture_u: i32,
    pub texture_v: i32,
    pub light_map: i32,
    pub brush_poly: i32,
    pub pan_u: i16,
    pub pan_v: i16,
    pub brush_actor: ObjRef,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BspVert {
    pub vertex: i32,
    pub side: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Zone {
    pub actor: ObjRef,
    pub connectivity: u64,
    pub visibility: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LightMapIndex {
    pub data_offset: i32,
    pub pan: [f32; 3],
    pub u_clamp: i32,
    pub v_clamp: i32,
    pub u_scale: f32,
    pub v_scale: f32,
    /// Index into `Model::lights` of the first light (then consecutive until a null entry), or -1.
    pub light_actors: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Leaf {
    pub zone: i32,
    pub permeating: i32,
    pub volumetric: i32,
    pub visible_zones: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub vectors: Vec<[f32; 3]>,
    pub points: Vec<[f32; 3]>,
    pub nodes: Vec<BspNode>,
    pub surfaces: Vec<BspSurface>,
    pub verts: Vec<BspVert>,
    pub num_shared_sides: i32,
    pub zones: Vec<Zone>,
    pub polys: ObjRef,
    pub light_maps: Vec<LightMapIndex>,
    /// Shadow masks: for each light map and each of its lights, one bit per texel.
    pub light_bits: Vec<u8>,
    pub bounds: Vec<Bounds>,
    pub leaf_hulls: Vec<i32>,
    pub leaves: Vec<Leaf>,
    pub lights: Vec<ObjRef>,
    pub root_outside: bool,
    pub linked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Poly {
    pub base: [f32; 3],
    pub normal: [f32; 3],
    pub texture_u: [f32; 3],
    pub texture_v: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
    pub flags: u32,
    pub actor: ObjRef,
    pub texture: ObjRef,
    pub item_name: String,
    pub link: i32,
    pub brush_poly: i32,
    pub pan_u: i16,
    pub pan_v: i16,
}

fn vec3(r: &mut Reader) -> Result<[f32; 3]> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

fn count(r: &mut Reader, what: &str) -> Result<usize> {
    let n = r.index()?;
    usize::try_from(n)
        .ok()
        .filter(|&n| n <= r.remaining().max(1))
        .ok_or_else(|| Error::Format(format!("bad {what} count {n}")))
}

pub fn read_level(pkg: &Package, index: usize) -> Result<Level> {
    let (_, mut r) = read_object(pkg, index)?;
    let n = r.i32()?;
    r.i32()?; // capacity
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n <= r.remaining())
        .ok_or_else(|| Error::Format("bad actor count".into()))?;
    let mut level = Level::default();
    for _ in 0..n {
        level.actors.push(pkg.read_ref(&mut r)?);
    }
    r.string()?; // protocol
    r.string()?; // host
    level.url_map = r.string()?;
    r.string()?; // portal
    let opts = count(&mut r, "URL option")?;
    for _ in 0..opts {
        r.string()?;
    }
    r.i32()?; // port
    r.u32()?; // valid
    level.model = pkg.read_ref(&mut r)?;
    let n = count(&mut r, "reach spec")?;
    for _ in 0..n {
        level.reach_specs.push(ReachSpec {
            distance: r.i32()?,
            start: pkg.read_ref(&mut r)?,
            end: pkg.read_ref(&mut r)?,
            collision_radius: r.i32()?,
            collision_height: r.i32()?,
            flags: r.i32()?,
            pruned: r.u8()? != 0,
        });
    }
    Ok(level)
}

pub fn read_model(pkg: &Package, index: usize) -> Result<Model> {
    let (_, mut r) = read_object(pkg, index)?;
    if pkg.version <= 61 {
        return Err(Error::Format("old (version 61) models are not supported".into()));
    }
    // UPrimitive bounds
    r.skip(24 + 1 + 16)?;
    let mut m = Model::default();
    let n = count(&mut r, "vector")?;
    for _ in 0..n {
        m.vectors.push(vec3(&mut r)?);
    }
    let n = count(&mut r, "point")?;
    for _ in 0..n {
        m.points.push(vec3(&mut r)?);
    }
    let n = count(&mut r, "node")?;
    for _ in 0..n {
        m.nodes.push(BspNode {
            plane: [r.f32()?, r.f32()?, r.f32()?, r.f32()?],
            zone_mask: r.u64()?,
            flags: r.u8()?,
            vert_pool: r.index()?,
            surf: r.index()?,
            back: r.index()?,
            front: r.index()?,
            plane_next: r.index()?,
            collision_bound: r.index()?,
            render_bound: r.index()?,
            zone: [r.index()?, r.index()?],
            num_vertices: r.u8()?,
            leaf: [r.i32()?, r.i32()?],
        });
    }
    let n = count(&mut r, "surface")?;
    for _ in 0..n {
        m.surfaces.push(BspSurface {
            texture: pkg.read_ref(&mut r)?,
            poly_flags: r.u32()?,
            base: r.index()?,
            normal: r.index()?,
            texture_u: r.index()?,
            texture_v: r.index()?,
            light_map: r.index()?,
            brush_poly: r.index()?,
            pan_u: r.i16()?,
            pan_v: r.i16()?,
            brush_actor: pkg.read_ref(&mut r)?,
        });
    }
    let n = count(&mut r, "vertex")?;
    for _ in 0..n {
        m.verts.push(BspVert { vertex: r.index()?, side: r.index()? });
    }
    m.num_shared_sides = r.i32()?;
    let n = r.i32()?;
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n <= 64)
        .ok_or_else(|| Error::Format(format!("bad zone count {n}")))?;
    for _ in 0..n {
        m.zones.push(Zone { actor: pkg.read_ref(&mut r)?, connectivity: r.u64()?, visibility: r.u64()? });
    }
    m.polys = pkg.read_ref(&mut r)?;
    let n = count(&mut r, "light map")?;
    for _ in 0..n {
        m.light_maps.push(LightMapIndex {
            data_offset: r.i32()?,
            pan: vec3(&mut r)?,
            u_clamp: r.index()?,
            v_clamp: r.index()?,
            u_scale: r.f32()?,
            v_scale: r.f32()?,
            light_actors: r.i32()?,
        });
    }
    let n = count(&mut r, "light bit")?;
    m.light_bits = r.bytes(n)?.to_vec();
    let n = count(&mut r, "bounds")?;
    for _ in 0..n {
        let min = vec3(&mut r)?;
        let max = vec3(&mut r)?;
        r.u8()?;
        m.bounds.push(Bounds { min, max });
    }
    let n = count(&mut r, "leaf hull")?;
    for _ in 0..n {
        m.leaf_hulls.push(r.i32()?);
    }
    let n = count(&mut r, "leaf")?;
    for _ in 0..n {
        m.leaves.push(Leaf {
            zone: r.index()?,
            permeating: r.index()?,
            volumetric: r.index()?,
            visible_zones: r.u64()?,
        });
    }
    let n = count(&mut r, "light")?;
    for _ in 0..n {
        m.lights.push(pkg.read_ref(&mut r)?);
    }
    m.root_outside = r.i32()? != 0;
    m.linked = r.i32()? != 0;
    if r.remaining() != 0 {
        return Err(Error::Format(format!("model: {} bytes left over", r.remaining())));
    }
    Ok(m)
}

pub fn read_polys(pkg: &Package, index: usize) -> Result<Vec<Poly>> {
    let (_, mut r) = read_object(pkg, index)?;
    let n = r.i32()?;
    r.i32()?; // capacity
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n <= r.remaining())
        .ok_or_else(|| Error::Format("bad poly count".into()))?;
    let mut polys = Vec::with_capacity(n);
    for _ in 0..n {
        let nv = count(&mut r, "poly vertex")?;
        let mut p = Poly {
            base: vec3(&mut r)?,
            normal: vec3(&mut r)?,
            texture_u: vec3(&mut r)?,
            texture_v: vec3(&mut r)?,
            ..Default::default()
        };
        for _ in 0..nv {
            p.vertices.push(vec3(&mut r)?);
        }
        p.flags = r.u32()?;
        p.actor = pkg.read_ref(&mut r)?;
        p.texture = pkg.read_ref(&mut r)?;
        p.item_name = pkg.read_name(&mut r)?.to_owned();
        p.link = r.index()?;
        p.brush_poly = r.index()?;
        p.pan_u = r.i16()?;
        p.pan_v = r.i16()?;
        polys.push(p);
    }
    Ok(polys)
}
