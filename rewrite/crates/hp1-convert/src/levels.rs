//! Maps → our level format.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use hp1_data::level::{
    Actor, Bound, Brush, LeafEntry, LevelFile, LightMap, Model, Node, Poly, ReachSpec, Surface, Vert,
    ZoneEntry,
};
use hp1_data::mesh::save_binary;
use hp1_data::{DataValue, SectionReport};
use rayon::prelude::*;
use ue1_read::level::{read_level, read_model, read_polys};
use ue1_read::{ObjRef, Package, read_object};

use crate::{Converter, Result, prop_map, write_json};

pub fn convert_levels(conv: &Converter) -> Result<SectionReport> {
    let maps: Vec<String> = conv
        .lib
        .package_names()
        .into_iter()
        .filter(|n| {
            conv.lib.path_of(n).is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("unr")))
        })
        .collect();
    let results: Vec<std::result::Result<(), String>> = maps
        .par_iter()
        .map(|name| {
            let pkg = conv.lib.package(name).map_err(|e| format!("{name}: {e}"))?;
            convert_map(conv, &pkg).map_err(|e| format!("{name}: {e}"))
        })
        .collect();
    let mut report = SectionReport::default();
    for r in results {
        match r {
            Ok(()) => report.items += 1,
            Err(e) => report.errors.push(e),
        }
    }
    Ok(report)
}

struct Ctx<'a> {
    pkg: &'a Package,
    /// Export index → actor index.
    actor_of: HashMap<usize, i32>,
}

impl Ctx<'_> {
    fn actor(&self, r: ObjRef) -> i32 {
        match r {
            ObjRef::Export(i) => self.actor_of.get(&i).copied().unwrap_or(-1),
            _ => -1,
        }
    }
    fn texture(&self, r: ObjRef) -> Option<String> {
        (!r.is_none()).then(|| self.pkg.ref_path(r))
    }

    fn model(&self, index: usize) -> Result<Model> {
        let m = read_model(self.pkg, index)?;
        Ok(Model {
            vectors: m.vectors,
            points: m.points,
            nodes: m
                .nodes
                .iter()
                .map(|n| Node {
                    plane: n.plane,
                    zone_mask: n.zone_mask,
                    flags: n.flags,
                    vert_pool: n.vert_pool,
                    surf: n.surf,
                    back: n.back,
                    front: n.front,
                    plane_next: n.plane_next,
                    collision_bound: n.collision_bound,
                    render_bound: n.render_bound,
                    zone: n.zone,
                    num_vertices: n.num_vertices,
                    leaf: n.leaf,
                })
                .collect(),
            surfaces: m
                .surfaces
                .iter()
                .map(|s| Surface {
                    texture: self.texture(s.texture),
                    poly_flags: s.poly_flags,
                    base: s.base,
                    normal: s.normal,
                    texture_u: s.texture_u,
                    texture_v: s.texture_v,
                    light_map: s.light_map,
                    brush_poly: s.brush_poly,
                    pan_u: s.pan_u,
                    pan_v: s.pan_v,
                    brush_actor: self.actor(s.brush_actor),
                })
                .collect(),
            verts: m.verts.iter().map(|v| Vert { point: v.vertex, side: v.side }).collect(),
            zones: m
                .zones
                .iter()
                .map(|z| ZoneEntry {
                    actor: self.actor(z.actor),
                    connectivity: z.connectivity,
                    visibility: z.visibility,
                })
                .collect(),
            light_maps: m
                .light_maps
                .iter()
                .map(|l| LightMap {
                    data_offset: l.data_offset,
                    pan: l.pan,
                    u_clamp: l.u_clamp,
                    v_clamp: l.v_clamp,
                    u_scale: l.u_scale,
                    v_scale: l.v_scale,
                    first_light: l.light_actors,
                })
                .collect(),
            light_bits: m.light_bits,
            bounds: m.bounds.iter().map(|b| Bound { min: b.min, max: b.max }).collect(),
            leaf_hulls: m.leaf_hulls,
            leaves: m
                .leaves
                .iter()
                .map(|l| LeafEntry {
                    zone: l.zone,
                    permeating: l.permeating,
                    volumetric: l.volumetric,
                    visible_zones: l.visible_zones,
                })
                .collect(),
            lights: m.lights.iter().map(|&l| self.actor(l)).collect(),
            root_outside: m.root_outside,
            linked: m.linked,
        })
    }

    fn polys(&self, r: ObjRef) -> Result<Vec<Poly>> {
        let ObjRef::Export(i) = r else { return Ok(Vec::new()) };
        Ok(read_polys(self.pkg, i)?
            .into_iter()
            .map(|p| Poly {
                base: p.base,
                normal: p.normal,
                texture_u: p.texture_u,
                texture_v: p.texture_v,
                vertices: p.vertices,
                flags: p.flags,
                texture: self.texture(p.texture),
                item_name: p.item_name,
                link: p.link,
                pan_u: p.pan_u,
                pan_v: p.pan_v,
            })
            .collect())
    }
}

fn convert_map(conv: &Converter, pkg: &Arc<Package>) -> Result<()> {
    let level_index =
        pkg.exports.iter().position(|e| pkg.class_name(e) == "Level").ok_or("no Level object in the map")?;
    let level = read_level(pkg, level_index)?;
    let mut ctx = Ctx { pkg, actor_of: HashMap::new() };
    for (i, a) in level.actors.iter().enumerate() {
        if let ObjRef::Export(e) = a {
            ctx.actor_of.insert(*e, i as i32);
        }
    }

    let mut brushes = BTreeMap::new();
    let mut actors = Vec::with_capacity(level.actors.len());
    for a in &level.actors {
        let ObjRef::Export(e) = *a else {
            actors.push(None);
            continue;
        };
        let export = &pkg.exports[e];
        let (props, _) = read_object(pkg, e)?;
        let mut brush = None;
        // Plain `Brush` actors are the editor's building blocks, already baked into the level's
        // BSP. Only brushes that exist at runtime (movers and other brush subclasses) keep theirs.
        let is_editor_brush = pkg.class_name(export).eq_ignore_ascii_case("Brush");
        if let Some(ObjRef::Export(b)) = props.object("Brush")
            && !is_editor_brush
            && pkg.class_name(&pkg.exports[b]) == "Model"
        {
            let key = pkg.ref_path(ObjRef::Export(b));
            if !brushes.contains_key(&key) {
                let model = ctx.model(b)?;
                let polys = ctx.polys(read_model(pkg, b)?.polys)?;
                brushes.insert(key.clone(), Brush { model, polys });
            }
            brush = Some(key);
        }
        actors.push(Some(Actor {
            name: pkg.name(export.name).to_owned(),
            class: pkg.ref_path(export.class),
            props: prop_map(pkg, &props),
            brush,
        }));
    }

    let ObjRef::Export(model_index) = level.model else { return Err("level has no model".into()) };
    let file = LevelFile {
        name: pkg.name.clone(),
        model: ctx.model(model_index)?,
        reach_specs: level
            .reach_specs
            .iter()
            .map(|r| ReachSpec {
                distance: r.distance,
                start: ctx.actor(r.start),
                end: ctx.actor(r.end),
                collision_radius: r.collision_radius,
                collision_height: r.collision_height,
                flags: r.flags,
                pruned: r.pruned,
            })
            .collect(),
        actors,
        brushes,
    };
    save_binary(&conv.out.join("levels").join(format!("{}.level", pkg.name)), &file)?;

    // A readable summary for people: counts per class and the level's title.
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    for a in file.actors.iter().flatten() {
        *classes.entry(a.class.clone()).or_default() += 1;
    }
    let title = file
        .actors
        .iter()
        .flatten()
        .find(|a| a.class.ends_with(".LevelInfo"))
        .and_then(|a| a.props.get("Title").cloned())
        .unwrap_or(DataValue::Null);
    let summary = serde_json::json!({
        "name": file.name,
        "title": title,
        "actors": file.actors.iter().flatten().count(),
        "bsp_nodes": file.model.nodes.len(),
        "surfaces": file.model.surfaces.len(),
        "light_maps": file.model.light_maps.len(),
        "brushes": file.brushes.len(),
        "classes": classes,
    });
    write_json(&conv.out.join("levels").join(format!("{}.json", pkg.name)), &summary)?;
    Ok(())
}
