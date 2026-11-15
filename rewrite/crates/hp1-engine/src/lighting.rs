//! Static lighting of the level geometry.
//!
//! Every lit surface has a small light map: a grid of texels laid over the surface. A texel's
//! colour is the zone's ambient colour plus, for each light listed for that surface, the light's
//! colour times how strongly it reaches the texel. "How strongly" combines three things: the
//! shadow mask stored in the level (one bit per texel, softened with a 3×3 blur), the distance to
//! the light (a smooth falloff that reaches zero at the light's radius) and the angle between the
//! surface and the direction to the light. Some lights shape their light differently (spot,
//! cylinder, shell, …), see `effect_value`.
//!
//! The formulas match the SurrealEngine build on `main`, so screenshots of both can be compared.

use glam::Vec3;
use hp1_data::ClassTable;
use hp1_data::level::{LevelFile, LightMap, Model};

use crate::props::ActorProps;
use crate::units::rotator_matrix;

/// One light map's texels as linear RGB, row by row.
#[derive(Clone, Debug)]
pub struct LightMapImage {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<[f32; 3]>,
}

/// The parts of a light that shape static lighting.
#[derive(Clone, Copy, Debug)]
pub struct LightSource {
    pub location: Vec3,
    /// In world units.
    pub radius: f32,
    pub color: Vec3,
    pub effect: u8,
    pub cone: u8,
    /// The direction the light points (spot lights).
    pub direction: Vec3,
}

/// The game's colour model for lights: hue, saturation (255 = white) and brightness, each 0–255.
pub fn hsb_to_rgb(hue: u8, saturation: u8, brightness: u8) -> Vec3 {
    if brightness == 0 {
        return Vec3::ZERO;
    }
    let v = 6.512735 * (brightness as f32).sqrt();
    if saturation >= 250 {
        return Vec3::splat(v / 255.0);
    }
    let mut s = saturation as f32 / 2.5;
    if s > 32.0 {
        s += 2.0;
    }
    let pos = hue as f32 / 85.0;
    let frac = pos - pos.floor();
    let p = s * v / 104.0;
    let q = (1.0 - frac) * v + p * frac;
    let t = frac * v + p * (1.0 - frac);
    let c = if hue < 85 {
        Vec3::new(q, t, p)
    } else if hue < 170 {
        Vec3::new(p, q, t)
    } else {
        Vec3::new(t, p, q)
    };
    c / 255.0
}

/// Light types that give no light.
const LIGHT_TYPE_NONE: u8 = 0;

impl LightSource {
    pub fn from_actor(a: &ActorProps) -> Option<Self> {
        let brightness = a.byte("LightBrightness", 64);
        if a.byte("LightType", 0) == LIGHT_TYPE_NONE || brightness == 0 {
            return None;
        }
        let [p, y, r] = a.rotator("Rotation");
        Some(Self {
            location: Vec3::from(a.vec3("Location")),
            radius: (a.byte("LightRadius", 64) as f32 + 1.0) * 25.0,
            color: hsb_to_rgb(a.byte("LightHue", 0), a.byte("LightSaturation", 255), brightness),
            effect: a.byte("LightEffect", 0),
            cone: a.byte("LightCone", 128),
            direction: rotator_matrix(p, y, r).transform_vector3(Vec3::X),
        })
    }
}

/// Distance falloff: 1 near the light, a smooth curve down to 0 at the radius. `d2` is the
/// squared distance divided by the squared radius.
fn falloff(d2: f32) -> f32 {
    let v = (d2 + 0.0001).sqrt();
    ((1.0 + 2.0 * v * v * v - 3.0 * v * v) / v).min(1.0)
}

/// How strongly `light` reaches a texel at `p` with surface normal `n`, before shadowing.
fn effect_value(light: &LightSource, p: Vec3, n: Vec3) -> f32 {
    let l = light.location - p;
    let len2 = l.length_squared();
    let inv_r2 = 1.0 / (light.radius * light.radius);
    let d2 = len2 * inv_r2;
    match light.effect {
        // Non-incidence: distance only, linear.
        13 => (1.0 - len2.sqrt() / light.radius).max(0.0),
        // Shell: a thin bright band near the radius.
        14 => {
            let d = len2.sqrt() / light.radius;
            if d > 0.8 && d < 1.0 { 1.0 - 10.0 * (d - 0.9).abs() } else { 0.0 }
        }
        // Omni bump map: gives no light.
        15 => 0.0,
        // Cylinder: horizontal distance only.
        17 => (1.0 - (l.x * l.x + l.y * l.y) * inv_r2).max(0.0),
        _ if d2 >= 1.0 => 0.0,
        // Static spot and spotlight: a cone around the light's direction.
        8 | 12 => {
            let cos_outer = 1.0 - light.cone as f32 / 255.0;
            if cos_outer >= 1.0 {
                return 0.0;
            }
            let ln = l / len2.sqrt();
            let spot = 1.0 - ((1.0 - ln.dot(-light.direction)) / (1.0 - cos_outer)).min(1.0);
            falloff(d2) * ln.dot(n).abs() * spot * spot
        }
        _ => {
            let ln = l / len2.sqrt();
            falloff(d2) * ln.dot(n).abs()
        }
    }
}

/// The texel grid of one light map in world space.
pub struct LightMapFrame {
    pub origin: Vec3,
    pub u: Vec3,
    pub v: Vec3,
    pub normal: Vec3,
}

impl LightMapFrame {
    /// World positions of all texel centres, row by row.
    pub fn texel_positions(&self, lm: &LightMap) -> Vec<Vec3> {
        let (w, h) = (lm.u_clamp.max(0) as usize, lm.v_clamp.max(0) as usize);
        let n = self.normal.normalize_or_zero();
        // Steps in world space that move one unit along the surface's u and v coordinates.
        let det = self.u.dot(self.v.cross(n));
        let (us, vs) = if det.abs() > f32::EPSILON {
            (self.v.cross(n) / det, n.cross(self.u) / det)
        } else {
            (self.u, self.v)
        };
        let u_pan = self.u.dot(self.origin) + lm.pan[0] - 0.5 * lm.u_scale;
        let v_pan = self.v.dot(self.origin) + lm.pan[1] - 0.5 * lm.v_scale;
        // Texel (s, t) maps back to world: origin + us·(s·UScale + UPan − u(origin)) + vs·(…).
        let u0 = self.u.dot(self.origin);
        let v0 = self.v.dot(self.origin);
        let mut out = Vec::with_capacity(w * h);
        for y in 0..h {
            let tv = (y as f32 + 0.5) * lm.v_scale + v_pan - v0;
            for x in 0..w {
                let tu = (x as f32 + 0.5) * lm.u_scale + u_pan - u0;
                out.push(self.origin + us * tu + vs * tv);
            }
        }
        out
    }
}

/// The light map's shadow mask for its `index`-th light, blurred, as 0..2 per texel.
fn shadow_mask(model: &Model, lm: &LightMap, index: usize) -> Option<Vec<f32>> {
    let (w, h) = (lm.u_clamp.max(0) as usize, lm.v_clamp.max(0) as usize);
    let pitch = w.div_ceil(8);
    let start = usize::try_from(lm.data_offset).ok()? + index * pitch * h;
    let bits = model.light_bits.get(start..start + pitch * h)?;
    let raw: Vec<f32> =
        (0..w * h).map(|i| ((bits[(i / w) * pitch + (i % w) / 8] >> (i % w % 8)) & 1) as f32).collect();
    const WEIGHTS: [f32; 3] = [0.125, 0.25, 0.125];
    const MIDDLE: [f32; 3] = [0.25, 0.5, 0.25];
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut v = 0.0;
            for dy in 0..3 {
                let yy = (y + dy).saturating_sub(1).min(h - 1);
                let row = if dy == 1 { &MIDDLE } else { &WEIGHTS };
                for (dx, weight) in row.iter().enumerate() {
                    let xx = (x + dx).saturating_sub(1).min(w - 1);
                    v += raw[yy * w + xx] * weight;
                }
            }
            out[y * w + x] = v;
        }
    }
    Some(out)
}

/// The ambient colour of zone number `zone` (from `bsp::zone_at`).
pub fn zone_ambient(level: &LevelFile, classes: &ClassTable, zone: usize) -> Vec3 {
    let actor = level.model.zones.get(zone).map_or(-1, |z| z.actor);
    ambient(level, classes, actor)
}

/// All lights of the level that give light.
pub fn level_lights(level: &LevelFile, classes: &ClassTable) -> Vec<LightSource> {
    level
        .actors
        .iter()
        .flatten()
        .filter_map(|a| LightSource::from_actor(&ActorProps::new(a, classes)))
        .collect()
}

/// Light at a point of a moving or placed object (no shadows): ambient plus each light's colour
/// times distance falloff times how much the surface faces the light.
pub fn vertex_light(lights: &[LightSource], ambient: Vec3, p: Vec3, n: Vec3) -> Vec3 {
    let mut c = ambient;
    for light in lights {
        let l = light.location - p;
        let d2 = l.length_squared() / (light.radius * light.radius);
        if d2 >= 1.0 {
            continue;
        }
        let facing = if n == Vec3::ZERO { 1.0 } else { (l.normalize_or_zero().dot(n)).max(0.0) };
        c += (light.color * falloff(d2) * facing).min(Vec3::ONE);
    }
    c.min(Vec3::ONE)
}

/// The zone ambient colour for a zone actor index (`-1` = the level's own settings).
fn ambient(level: &LevelFile, classes: &ClassTable, zone_actor: i32) -> Vec3 {
    let actor = usize::try_from(zone_actor)
        .ok()
        .and_then(|i| level.actors.get(i)?.as_ref())
        .or_else(|| level.actors.iter().flatten().find(|a| a.class.ends_with(".LevelInfo")));
    let Some(actor) = actor else { return Vec3::ZERO };
    let p = ActorProps::new(actor, classes);
    hsb_to_rgb(p.byte("AmbientHue", 0), p.byte("AmbientSaturation", 255), p.byte("AmbientBrightness", 0))
}

/// Builds every light map of the level's geometry. The result is indexed like
/// `level.model.light_maps`; maps no surface uses stay `None`.
pub fn build_level_light_maps(level: &LevelFile, classes: &ClassTable) -> Vec<Option<LightMapImage>> {
    let model = &level.model;
    let lights: Vec<Option<LightSource>> = level
        .actors
        .iter()
        .map(|a| a.as_ref().and_then(|a| LightSource::from_actor(&ActorProps::new(a, classes))))
        .collect();

    // Which surface (and which zone) each light map belongs to.
    let mut owner: Vec<Option<(usize, i32)>> = vec![None; model.light_maps.len()];
    for node in &model.nodes {
        let Some(surf) = usize::try_from(node.surf).ok().and_then(|s| model.surfaces.get(s).map(|x| (s, x)))
        else {
            continue;
        };
        if let Ok(lm) = usize::try_from(surf.1.light_map)
            && lm < owner.len()
            && owner[lm].is_none()
        {
            let zone = if node.zone[1] != 0 { node.zone[1] } else { node.zone[0] };
            let zone_actor =
                usize::try_from(zone).ok().and_then(|z| model.zones.get(z)).map_or(-1, |z| z.actor);
            owner[lm] = Some((surf.0, zone_actor));
        }
    }

    let point = |i: i32| {
        usize::try_from(i).ok().and_then(|i| model.points.get(i)).map_or(Vec3::ZERO, |p| Vec3::from(*p))
    };
    let vector = |i: i32| {
        usize::try_from(i).ok().and_then(|i| model.vectors.get(i)).map_or(Vec3::ZERO, |p| Vec3::from(*p))
    };

    model
        .light_maps
        .iter()
        .zip(&owner)
        .map(|(lm, owner)| {
            let (surf, zone_actor) = (*owner)?;
            let s = &model.surfaces[surf];
            let (w, h) = (u32::try_from(lm.u_clamp).ok()?, u32::try_from(lm.v_clamp).ok()?);
            if w == 0 || h == 0 || w > 4096 || h > 4096 {
                return None;
            }
            let frame = LightMapFrame {
                origin: point(s.base),
                u: vector(s.texture_u),
                v: vector(s.texture_v),
                normal: vector(s.normal),
            };
            let n = frame.normal.normalize_or_zero();
            let positions = frame.texel_positions(lm);
            let base = ambient(level, classes, zone_actor);
            let mut texels = vec![base; positions.len()];
            if let Ok(first) = usize::try_from(lm.first_light) {
                for (index, &actor) in model.lights.iter().skip(first).take_while(|&&a| a >= 0).enumerate() {
                    let Some(light) = lights.get(actor as usize).copied().flatten() else { continue };
                    let Some(shadow) = shadow_mask(model, lm, index) else { continue };
                    for ((t, p), sh) in texels.iter_mut().zip(&positions).zip(&shadow) {
                        if *sh <= 0.0 {
                            continue;
                        }
                        let value = sh * effect_value(&light, *p, n);
                        if value > 0.0 {
                            *t += (light.color * value).min(Vec3::ONE);
                        }
                    }
                }
            }
            Some(LightMapImage {
                width: w,
                height: h,
                texels: texels.iter().map(|t| t.min(Vec3::ONE).into()).collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_light_is_grey_scaled_by_brightness() {
        let c = hsb_to_rgb(0, 255, 255);
        assert!((c.x - 104.0 / 255.0).abs() < 0.01 && c.x == c.y && c.y == c.z);
        assert_eq!(hsb_to_rgb(10, 0, 0), Vec3::ZERO);
    }

    #[test]
    fn red_hue_is_reddest() {
        let c = hsb_to_rgb(0, 0, 200);
        assert!(c.x > c.y && c.x > c.z, "{c}");
    }

    #[test]
    fn falloff_reaches_zero_at_radius() {
        assert!(falloff(0.01) > 0.99);
        assert!(falloff(0.999).abs() < 0.01);
    }

    #[test]
    fn texel_positions_follow_the_grid() {
        let frame = LightMapFrame { origin: Vec3::ZERO, u: Vec3::X, v: Vec3::Y, normal: Vec3::Z };
        let lm = LightMap {
            data_offset: 0,
            pan: [0.0; 3],
            u_clamp: 2,
            v_clamp: 1,
            u_scale: 16.0,
            v_scale: 16.0,
            first_light: -1,
        };
        let p = frame.texel_positions(&lm);
        assert_eq!(p.len(), 2);
        // Texel centres sit half a texel in, minus the half-texel shift the format uses.
        assert!((p[0] - Vec3::new(0.0, 0.0, 0.0)).length() < 1e-4, "{:?}", p);
        assert!((p[1] - Vec3::new(16.0, 0.0, 0.0)).length() < 1e-4, "{:?}", p);
    }
}
