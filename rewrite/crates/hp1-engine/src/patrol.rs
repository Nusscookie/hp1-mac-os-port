//! Patrol point chains that characters walk along (`specs/characters/Patrol.md`).

use glam::Vec3;

use crate::props::ActorProps;
use crate::units::rotator_matrix;
use crate::world::{ActorId, World};

#[derive(Clone, Debug)]
pub struct Point {
    pub actor: ActorId,
    pub name: String,
    pub tag: String,
    pub pos: Vec3,
    pub rot: [i32; 3],
    pub next: Option<usize>,
    pub prev: Option<usize>,
    /// Direction of the smooth curve through this point.
    pub tangent: Vec3,
    /// Distance to the next point.
    pub out_len: f32,
    pub pause: f32,
    pub pause_anim: String,
    /// Turn to the point's own facing while pausing.
    pub look: bool,
    pub event: String,
    pub sound: String,
    /// The character is removed when it gets here.
    pub destroy: bool,
}

/// Every patrol point of a level, linked into chains.
#[derive(Clone, Debug, Default)]
pub struct PatrolNet {
    pub points: Vec<Point>,
}

fn facing(rot: [i32; 3]) -> Vec3 {
    rotator_matrix(rot[0], rot[1], rot[2]).transform_vector3(Vec3::X)
}

/// Slow at both ends, fast in the middle.
pub fn ease(t: f32) -> f32 {
    if t < 0.5 { 2.0 * t * t } else { 1.0 - 2.0 * (1.0 - t) * (1.0 - t) }
}

impl PatrolNet {
    pub fn build(w: &World) -> Self {
        let mut points = Vec::new();
        // Next tag, next object name, starts an unlinked chain, link tag, curve along own facing.
        let mut links: Vec<(String, String, bool, String, bool)> = Vec::new();
        for (i, a) in w.level.actors.iter().enumerate() {
            let Some(a) = a else { continue };
            if !w.classes.is_a(&a.class, "Engine.PatrolPoint") {
                continue;
            }
            let Some(e) = w.get(i) else { continue };
            let p = ActorProps::new(a, &w.classes);
            points.push(Point {
                actor: i,
                name: a.name.clone(),
                tag: e.tag.clone(),
                pos: e.pos,
                rot: e.rot,
                next: None,
                prev: None,
                tangent: Vec3::ZERO,
                out_len: 0.0,
                pause: p.f32("pausetime", 0.0),
                pause_anim: p.text("PauseAnim"),
                look: p.bool("bUseLookDir"),
                event: p.text("EventToSend"),
                sound: p.text("PatrolSound"),
                destroy: p.bool("bDestroyPawn"),
            });
            links.push((
                p.text("Nextpatrol"),
                p.text("NextPatrol_ObjectName"),
                p.bool("bStartOfUnlinkedChain"),
                p.text("PatrolPointLinkTag"),
                p.bool("bUseOrientationForSplineTan"),
            ));
        }
        let n = points.len();

        for i in 0..n {
            let (tag, object, ..) = &links[i];
            let next = if !tag.is_empty() {
                points.iter().position(|q| q.tag.eq_ignore_ascii_case(tag))
            } else if !object.is_empty() {
                points.iter().position(|q| q.name.eq_ignore_ascii_case(object))
            } else {
                None
            };
            points[i].next = next.filter(|&j| j != i);
        }

        // Chains joined by a common link tag: each point to the nearest one not joined yet.
        for start in 0..n {
            let (_, _, chain, link, _) = &links[start];
            if !*chain || link.is_empty() {
                continue;
            }
            let mut joined = vec![start];
            let mut cur = start;
            loop {
                let here = points[cur].pos;
                let near = (0..n)
                    .filter(|&j| !joined.contains(&j) && links[j].3.eq_ignore_ascii_case(link))
                    .min_by(|&a, &b| {
                        here.distance_squared(points[a].pos).total_cmp(&here.distance_squared(points[b].pos))
                    });
                let Some(next) = near else { break };
                if points[cur].next.is_none() {
                    points[cur].next = Some(next);
                }
                joined.push(next);
                cur = next;
            }
        }

        for i in 0..n {
            if let Some(next) = points[i].next
                && points[next].prev.is_none()
            {
                points[next].prev = Some(i);
            }
        }

        for i in 0..n {
            let me = points[i].pos;
            let prev = points[i].prev.map(|j| points[j].pos);
            let next = points[i].next.map(|j| points[j].pos);
            points[i].tangent = if links[i].4 {
                facing(points[i].rot)
            } else {
                match (prev, next) {
                    (Some(a), Some(b)) => b - a,
                    (None, Some(b)) => b - me,
                    (Some(a), None) => me - a,
                    (None, None) => Vec3::ZERO,
                }
                .normalize_or_zero()
            };
            points[i].out_len = next.map_or(0.0, |b| me.distance(b));
        }
        Self { points }
    }

    /// Distance from the point before `i` to `i`.
    pub fn in_len(&self, i: usize) -> f32 {
        self.points[i].prev.map_or(0.0, |p| self.points[p].out_len)
    }

    /// The point with this tag, else the one with this object name.
    pub fn find(&self, tag: &str, object: &str) -> Option<usize> {
        let by_tag = (!tag.is_empty())
            .then(|| self.points.iter().position(|p| p.tag.eq_ignore_ascii_case(tag)))
            .flatten();
        by_tag.or_else(|| {
            (!object.is_empty())
                .then(|| self.points.iter().position(|p| p.name.eq_ignore_ascii_case(object)))?
        })
    }

    /// The nearest point to `pos` that is not one of `except`.
    pub fn nearest(&self, pos: Vec3, except: [Option<usize>; 2]) -> Option<usize> {
        (0..self.points.len()).filter(|i| !except.contains(&Some(*i))).min_by(|&a, &b| {
            pos.distance_squared(self.points[a].pos).total_cmp(&pos.distance_squared(self.points[b].pos))
        })
    }

    /// Where the curve from the point before `to` into `to` is at progress `t` (0..1).
    pub fn curve(&self, to: usize, t: f32) -> Option<Vec3> {
        let end = &self.points[to];
        let start = &self.points[end.prev?];
        let a = start.pos + start.tangent * start.out_len * t;
        let b = end.pos - end.tangent * start.out_len * (1.0 - t);
        Some(a.lerp(b, ease(t)))
    }
}
