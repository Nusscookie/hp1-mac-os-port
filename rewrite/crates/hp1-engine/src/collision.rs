//! Collision against the level geometry.
//!
//! The level stores, for many BSP nodes, a convex "hull": a list of planes (the solid lies behind
//! all of them) plus its bounding box. Moving a box through the level means finding the first
//! hull the box touches along its path.
//!
//! For one hull, the path is clipped against every plane, each pushed outwards by how far the box
//! reaches in that plane's direction, so the box can be treated as a point. Where the path is
//! behind all planes at once, it is inside the solid; the start of that stretch is the hit. Pushing
//! planes out makes hull corners too big, so extra planes are added: the hull's bounding box and
//! "bevel" planes along edges where two hull planes meet at a sharp angle.
//!
//! The BSP tree is walked only into the sides the moving box can touch. Actors are upright
//! cylinders in the game; like the SurrealEngine build on `main` (zlib licence), we sweep the
//! cylinder's bounding box instead.

use glam::{DVec3, DVec4, Vec3};
use hp1_data::level::Model;

/// Where a sweep stopped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// 0..1 along the path, already backed off a little so the box ends up just clear.
    pub time: f32,
    /// Surface normal at the hit, pointing away from the solid.
    pub normal: Vec3,
    /// The BSP node whose hull was hit (of the level, or of the actor's brush).
    pub node: usize,
    /// The actor that was hit; None for the level itself.
    pub actor: Option<usize>,
}

/// Anything boxes can be swept against: the level alone, or the level with its actors.
pub trait Collider {
    /// The first hit of a box with half-size `extent` moving from `start` to `end`.
    fn trace(&self, start: Vec3, end: Vec3, extent: Vec3) -> Option<Hit>;
    /// Whether a box at `pos` is inside something solid.
    fn overlaps(&self, pos: Vec3, extent: Vec3) -> bool;
}

impl Collider for Model {
    fn trace(&self, start: Vec3, end: Vec3, extent: Vec3) -> Option<Hit> {
        trace_model(self, start, end, extent)
    }
    fn overlaps(&self, pos: Vec3, extent: Vec3) -> bool {
        overlaps_model(self, pos, extent)
    }
}

/// Bit in a hull's plane index that means "use the plane reversed".
const HULL_FLIP: i32 = 0x4000_0000;

/// Hits stop this far (world units, along the path) before touching. The hull boxes are shaved
/// by `BOX_SHAVE`, which can let a box reach that much further, so this is a little larger.
const BACK_OFF: f64 = 0.2;

/// Hull bounding boxes are shrunk by this much so small boxes don't snag on hull seams.
const BOX_SHAVE: f64 = 0.1;

const EPS: f64 = 1e-5;

struct Sweep {
    start: DVec3,
    end: DVec3,
    extent: DVec3,
    /// When the path enters the (pushed-out) solid, and the plane it crossed.
    t_in: f64,
    normal: DVec3,
    /// When it leaves again.
    t_out: f64,
}

impl Sweep {
    fn new(start: DVec3, end: DVec3, extent: DVec3) -> Self {
        Self { start, end, extent, t_in: -1.0, normal: DVec3::ZERO, t_out: 2.0 }
    }

    /// Clips the path against one plane (solid behind). Returns false once the hull is missed.
    fn clip(&mut self, plane: DVec4) -> bool {
        let n = plane.truncate();
        let push = n.abs().dot(self.extent);
        let dist0 = n.dot(self.start) - plane.w;
        let dist1 = n.dot(self.end) - plane.w;
        let toward = dist0 - dist1;
        let mut gap = dist0 - push;
        // A box already touching the plane and moving into it is hit right away.
        if toward > 0.0 && gap < 0.0 && dist0 >= 0.0 {
            gap = 0.0;
        }
        if toward > EPS {
            let t = gap / toward;
            if t > self.t_in {
                self.t_in = t;
                self.normal = n;
            }
        } else if toward < -EPS {
            self.t_out = self.t_out.min(gap / toward);
        } else if gap > 0.0 {
            // Moving along the plane, in front of it.
            return false;
        }
        self.t_in < self.t_out
    }

    /// Adds the bevel plane along the edge of `p0` and `p1`, facing across `axis`.
    fn clip_bevel(&mut self, p0: DVec4, p1: DVec4, axis: DVec3) -> bool {
        let (n0, n1) = (p0.truncate(), p1.truncate());
        if axis.cross(n0).dot(axis.cross(n1)) <= EPS {
            return true;
        }
        let edge = n0.cross(n1);
        let len2 = edge.length_squared();
        if len2 < 1e-6 {
            return true;
        }
        // A point on the line where both planes meet.
        let point = (p0.w * n1.cross(edge) + p1.w * edge.cross(n0)) / len2;
        let mut normal = axis.cross(edge.normalize()).normalize();
        if n0.dot(normal) < 0.0 {
            normal = -normal;
        }
        self.clip(normal.extend(point.dot(normal)))
    }

    /// The hit time (0..1, backed off) if the path ran into this hull.
    fn hit(&self, length: f64) -> Option<f64> {
        (self.t_in > -1.0 && self.t_in < self.t_out && self.t_out > 0.0 && self.t_in <= 1.0)
            .then(|| (self.t_in - BACK_OFF / length).max(0.0))
    }
}

/// Reads a hull: its planes into `planes` and its bounding box.
fn hull(model: &Model, bound: usize, planes: &mut Vec<DVec4>) -> Option<(DVec3, DVec3)> {
    planes.clear();
    let list = model.leaf_hulls.get(bound..)?;
    let count = list.iter().position(|&e| e < 0)?;
    for &entry in &list[..count] {
        let node = model.nodes.get((entry & !HULL_FLIP) as usize)?;
        let p = DVec4::from(node.plane.map(f64::from));
        planes.push(if entry & HULL_FLIP != 0 { -p } else { p });
    }
    // Six floats follow the terminator: the hull's bounding box, stored as raw bits.
    let b = list.get(count + 1..count + 7)?;
    let f = |k: usize| f32::from_bits(b[k] as u32) as f64;
    Some((DVec3::new(f(0), f(1), f(2)), DVec3::new(f(3), f(4), f(5))))
}

/// Which side of a node's plane a box is on: 1 front, -1 back, 0 both.
fn side(plane: [f32; 4], center: DVec3, extent: DVec3) -> i32 {
    let p = DVec4::from(plane.map(f64::from));
    let s = p.truncate().dot(center) - p.w;
    let e = p.truncate().abs().dot(extent);
    if s - e > 0.0 {
        1
    } else if s + e < 0.0 {
        -1
    } else {
        0
    }
}

fn sweep_hull(model: &Model, bound: usize, planes: &mut Vec<DVec4>, s: &mut Sweep) -> bool {
    let Some((lo, hi)) = hull(model, bound, planes) else { return false };
    let (lo, hi) = (lo + BOX_SHAVE, hi - BOX_SHAVE);
    let sides = [
        DVec4::new(1.0, 0.0, 0.0, hi.x),
        DVec4::new(-1.0, 0.0, 0.0, -lo.x),
        DVec4::new(0.0, 1.0, 0.0, hi.y),
        DVec4::new(0.0, -1.0, 0.0, -lo.y),
        DVec4::new(0.0, 0.0, 1.0, hi.z),
        DVec4::new(0.0, 0.0, -1.0, -lo.z),
    ];
    if !sides.iter().chain(planes.iter()).all(|&p| s.clip(p)) {
        return false;
    }
    for i in 0..planes.len() {
        for j in 0..i {
            let (a, b) = (planes[i], planes[j]);
            for (k, axis) in [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate() {
                if a[k] * b[k] < 0.0 && !s.clip_bevel(a, b, axis) {
                    return false;
                }
            }
        }
    }
    true
}

/// Calls `visit(node, bound)` for every hull a box moving from `start` to `end` may touch.
fn for_each_hull(
    model: &Model,
    start: DVec3,
    end: DVec3,
    extent: DVec3,
    mut visit: impl FnMut(usize, usize),
) {
    if model.nodes.is_empty() {
        return;
    }
    let padded = extent * 1.1 + 0.01;
    let mut stack = vec![0usize];
    while let Some(index) = stack.pop() {
        let node = &model.nodes[index];
        if let Ok(bound) = usize::try_from(node.collision_bound) {
            visit(index, bound);
        }
        let a = side(node.plane, start, padded);
        let b = side(node.plane, end, padded);
        if let Ok(front) = usize::try_from(node.front)
            && (a >= 0 || b >= 0)
        {
            stack.push(front);
        }
        if let Ok(back) = usize::try_from(node.back)
            && (a <= 0 || b <= 0)
        {
            stack.push(back);
        }
    }
}

/// Sweeps a box with half-size `extent` from `start` to `end` and returns the first hit.
/// A zero extent traces a line. A box that starts inside solid is stopped at once.
pub fn trace_model(model: &Model, start: Vec3, end: Vec3, extent: Vec3) -> Option<Hit> {
    let (start, end, extent) = (start.as_dvec3(), end.as_dvec3(), extent.as_dvec3());
    let length = start.distance(end);
    if length < EPS {
        return None;
    }
    let mut best: Option<Hit> = None;
    let mut planes = Vec::new();
    for_each_hull(model, start, end, extent, |index, bound| {
        let mut s = Sweep::new(start, end, extent);
        if sweep_hull(model, bound, &mut planes, &mut s)
            && let Some(t) = s.hit(length)
            && best.is_none_or(|b| (t as f32) < b.time)
        {
            best = Some(Hit { time: t as f32, normal: s.normal.as_vec3(), node: index, actor: None });
        }
    });
    best
}

/// Whether a box with half-size `extent` at `pos` is inside solid (touching doesn't count).
pub fn overlaps_model(model: &Model, pos: Vec3, extent: Vec3) -> bool {
    let (pos, extent) = (pos.as_dvec3(), (extent.as_dvec3() - 0.05).max(DVec3::ZERO));
    let mut planes = Vec::new();
    let mut inside = false;
    for_each_hull(model, pos, pos, extent, |_, bound| {
        // A path that doesn't move is "behind every plane" exactly when the box is inside.
        inside = inside || sweep_hull(model, bound, &mut planes, &mut Sweep::new(pos, pos, extent));
    });
    inside
}

/// Moves a box as far as it can towards `pos + delta`. Returns where it ended and what stopped it.
pub fn move_box(c: &(impl Collider + ?Sized), pos: Vec3, delta: Vec3, extent: Vec3) -> (Vec3, Option<Hit>) {
    match c.trace(pos, pos + delta, extent) {
        Some(hit) => (pos + delta * hit.time, Some(hit)),
        None => (pos + delta, None),
    }
}

/// The nearest place to `pos` where the box is free: first straight up (up to `up`), then in rings
/// around. None if there is no room nearby.
pub fn find_spot(c: &(impl Collider + ?Sized), pos: Vec3, extent: Vec3, up: f32) -> Option<Vec3> {
    if !c.overlaps(pos, extent) {
        return Some(pos);
    }
    let mut z = 1.0;
    while z <= up {
        let p = pos + Vec3::Z * z;
        if !c.overlaps(p, extent) {
            return Some(p);
        }
        z += 1.0;
    }
    for ring in 1..=8 {
        let r = ring as f32 * 4.0;
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            for dz in [0.0, up * 0.5, up] {
                let p = pos + Vec3::new(a.cos() * r, a.sin() * r, dz);
                if !c.overlaps(p, extent) {
                    return Some(p);
                }
            }
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use hp1_data::level::Node;

    fn node(plane: [f32; 4], front: i32, back: i32, collision_bound: i32) -> Node {
        Node {
            plane,
            zone_mask: 0,
            flags: 0,
            vert_pool: 0,
            surf: -1,
            back,
            front,
            plane_next: -1,
            collision_bound,
            render_bound: -1,
            zone: [0, 0],
            num_vertices: 0,
            leaf: [-1, -1],
        }
    }

    fn push_hull(hulls: &mut Vec<i32>, planes: &[i32], lo: [f32; 3], hi: [f32; 3]) -> i32 {
        let at = hulls.len() as i32;
        hulls.extend_from_slice(planes);
        hulls.push(-1);
        hulls.extend(lo.iter().chain(&hi).map(|f| f.to_bits() as i32));
        at
    }

    /// The half-space below z = 0 (one node, one hull plane).
    pub(crate) fn floor() -> Model {
        let mut hulls = Vec::new();
        let at = push_hull(&mut hulls, &[0], [-1000.0; 3], [1000.0, 1000.0, 0.0]);
        Model { nodes: vec![node([0.0, 0.0, 1.0, 0.0], -1, -1, at)], leaf_hulls: hulls, ..Default::default() }
    }

    /// A floor plus a 45° ramp: solid below z = 0 and below z = x (for x > 0, up to z = 100).
    pub(crate) fn ramp() -> Model {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let mut hulls = Vec::new();
        let floor = push_hull(&mut hulls, &[0], [-1000.0; 3], [1000.0, 1000.0, 0.0]);
        // Ramp hull: under the slope (normal (-s, 0, s)), x <= 100 (plane 2), z >= -1 (plane 3 flipped).
        let ramp =
            push_hull(&mut hulls, &[1, 2, 3 | HULL_FLIP], [0.0, -1000.0, -1.0], [100.0, 1000.0, 100.0]);
        Model {
            nodes: vec![
                node([0.0, 0.0, 1.0, 0.0], 1, -1, floor),
                node([-s, 0.0, s, 0.0], -1, -1, ramp),
                node([1.0, 0.0, 0.0, 100.0], -1, -1, -1),
                node([0.0, 0.0, 1.0, -1.0], -1, -1, -1),
            ],
            leaf_hulls: hulls,
            ..Default::default()
        }
    }

    /// A floor plus a block of the given height for x in 0..200.
    pub(crate) fn step(height: f32) -> Model {
        let mut hulls = Vec::new();
        let floor = push_hull(&mut hulls, &[0], [-1000.0; 3], [1000.0, 1000.0, 0.0]);
        let block =
            push_hull(&mut hulls, &[2, 3, 4, 5 | HULL_FLIP], [0.0, -1000.0, -1.0], [200.0, 1000.0, height]);
        Model {
            nodes: vec![
                node([0.0, 0.0, 1.0, 0.0], 1, -1, floor),
                node([0.0, 0.0, 1.0, height], -1, -1, block),
                node([-1.0, 0.0, 0.0, 0.0], -1, -1, -1),
                node([1.0, 0.0, 0.0, 200.0], -1, -1, -1),
                node([0.0, 0.0, 1.0, height], -1, -1, -1),
                node([0.0, 0.0, 1.0, -1.0], -1, -1, -1),
            ],
            leaf_hulls: hulls,
            ..Default::default()
        }
    }

    /// A solid cube of half-size `h` around the origin.
    pub(crate) fn cube(h: f32) -> Model {
        let mut hulls = Vec::new();
        let at = push_hull(&mut hulls, &[0, 1, 2, 3, 4, 5], [-h; 3], [h; 3]);
        let planes = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        let mut nodes: Vec<Node> = planes.iter().map(|n| node([n[0], n[1], n[2], h], -1, -1, -1)).collect();
        nodes[0].collision_bound = at;
        Model { nodes, leaf_hulls: hulls, ..Default::default() }
    }

    #[test]
    fn line_hits_the_floor() {
        let hit = trace_model(&floor(), Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 0.0, -100.0), Vec3::ZERO)
            .unwrap();
        assert!((hit.time - 0.5).abs() < 0.01, "{hit:?}");
        assert_eq!(hit.normal, Vec3::Z);
    }

    #[test]
    fn box_stops_its_half_height_above_the_floor() {
        let e = Vec3::new(10.0, 10.0, 40.0);
        let hit = trace_model(&floor(), Vec3::new(0.0, 0.0, 100.0), Vec3::new(0.0, 0.0, -100.0), e).unwrap();
        let z = 100.0 - 200.0 * hit.time;
        assert!(z > 40.0 && z < 40.5, "stopped at {z}");
        // From where it stopped, it can still slide sideways.
        assert!(trace_model(&floor(), Vec3::new(0.0, 0.0, z), Vec3::new(50.0, 0.0, z), e).is_none());
        // But not move down any further.
        let again = trace_model(&floor(), Vec3::new(0.0, 0.0, z), Vec3::new(0.0, 0.0, z - 10.0), e).unwrap();
        assert!(again.time < 0.001, "{again:?}");
    }

    #[test]
    fn moving_away_does_not_hit() {
        assert!(
            trace_model(&floor(), Vec3::new(0.0, 0.0, 50.0), Vec3::new(0.0, 0.0, 200.0), Vec3::ZERO)
                .is_none()
        );
    }

    #[test]
    fn walking_into_a_ramp_hits_its_slope() {
        let m = ramp();
        let e = Vec3::new(10.0, 10.0, 20.0);
        // Walking along +x at z = 21 (box bottom just above the floor), the box's front-bottom
        // corner reaches the slope.
        let hit = trace_model(&m, Vec3::new(-50.0, 0.0, 21.0), Vec3::new(50.0, 0.0, 21.0), e).unwrap();
        let x = -50.0 + 100.0 * hit.time;
        assert!(hit.normal.x < -0.5 && hit.normal.z > 0.5, "{hit:?}");
        assert!((x - -9.0).abs() < 1.0, "stopped at x = {x}");
    }

    #[test]
    fn lines_miss_beside_the_ramp_hull() {
        let m = ramp();
        // Above the ramp top: passes over.
        assert!(
            trace_model(&m, Vec3::new(-50.0, 0.0, 150.0), Vec3::new(200.0, 0.0, 150.0), Vec3::ZERO).is_none()
        );
    }
}
