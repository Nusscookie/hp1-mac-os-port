//! Queries on the level's BSP tree.

use glam::Vec3;
use hp1_data::level::Model;

/// The zone number at a point (0 when the level has no zones or the point is outside).
pub fn zone_at(model: &Model, p: Vec3) -> usize {
    let mut index = 0usize;
    for _ in 0..model.nodes.len() {
        let Some(n) = model.nodes.get(index) else { return 0 };
        let front = Vec3::new(n.plane[0], n.plane[1], n.plane[2]).dot(p) - n.plane[3] >= 0.0;
        let next = if front { n.front } else { n.back };
        match usize::try_from(next) {
            Ok(i) => index = i,
            Err(_) => return usize::try_from(n.zone[front as usize]).unwrap_or(0),
        }
    }
    0
}
