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
    pub link_tag: String,
    pub pos: Vec3,
    pub rot: [i32; 3],
    pub next: Option<usize>,
    pub prev: Option<usize>,
    /// Whether a curve runs through this point (it has a point before or after it).
    pub has_curve: bool,
    /// Direction of the curve through this point.
    pub tangent: Vec3,
    /// Length of the curve's pull leaving this point.
    pub out_len: f32,
    /// Length of the pull arriving here; 0 = take the point before's `out_len`.
    in_len: f32,
    pub pause: f32,
    pub pause_anim: String,
    /// Turn to the point's own facing while pausing.
    pub look: bool,
    pub event: String,
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
    if t <= 0.0 {
        0.0
    } else if t < 0.5 {
        2.0 * t * t
    } else if t < 1.0 {
        1.0 - 2.0 * (1.0 - t) * (1.0 - t)
    } else {
        1.0
    }
}

/// How a point finds its next one.
struct Link {
    next_tag: String,
    next_name: String,
    starts_chain: bool,
    own_facing: bool,
}

impl PatrolNet {
    /// Links the points the way the level does when it starts: each point in turn, in the
    /// level's order, finds its next point, tells it who comes before it, looks for its own
    /// point before if it has none yet, and works out its curve from what it knows then.
    pub fn build(w: &World) -> Self {
        let mut points = Vec::new();
        let mut links = Vec::new();
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
                link_tag: p.text("PatrolPointLinkTag"),
                pos: e.pos,
                rot: e.rot,
                next: None,
                prev: None,
                has_curve: false,
                tangent: Vec3::ZERO,
                out_len: 0.0,
                in_len: 0.0,
                pause: p.f32("pausetime", 0.0),
                pause_anim: p.text("PauseAnim"),
                look: p.bool("bUseLookDir"),
                event: p.text("EventToSend"),
                destroy: p.bool("bDestroyPawn"),
            });
            links.push(Link {
                next_tag: p.text("Nextpatrol"),
                next_name: p.text("NextPatrol_ObjectName"),
                starts_chain: p.bool("bStartOfUnlinkedChain"),
                own_facing: p.bool("bUseOrientationForSplineTan"),
            });
        }
        let mut net = Self { points };
        for i in 0..net.points.len() {
            if net.points[i].next.is_none() {
                net.init_next(i, &links);
            }
            if let Some(n) = net.points[i].next {
                net.points[n].prev = Some(i);
            }
            if net.points[i].prev.is_none() {
                for a in 0..net.points.len() {
                    if net.points[a].next.is_none() {
                        net.init_next(a, &links);
                    }
                    if net.points[a].next == Some(i) {
                        net.points[i].prev = Some(a);
                        break;
                    }
                }
            }
            net.calc_curve(i, links[i].own_facing);
        }
        net
    }

    fn init_next(&mut self, i: usize, links: &[Link]) {
        let l = &links[i];
        if !l.next_tag.is_empty() {
            self.points[i].next = self.points.iter().position(|q| q.tag.eq_ignore_ascii_case(&l.next_tag));
        } else if !l.next_name.is_empty() {
            self.points[i].next = self.points.iter().position(|q| q.name.eq_ignore_ascii_case(&l.next_name));
        } else if l.starts_chain && !self.points[i].link_tag.is_empty() {
            // Join the chain: from here, each time the nearest point with the same link tag
            // that has no next point yet (the original's search includes the current point
            // itself, so the last point of a chain ends up pointing at itself).
            let tag = self.points[i].link_tag.clone();
            let mut cur = i;
            loop {
                let here = self.points[cur].pos;
                let near = (0..self.points.len())
                    .filter(|&j| {
                        self.points[j].next.is_none() && self.points[j].link_tag.eq_ignore_ascii_case(&tag)
                    })
                    .min_by(|&a, &b| {
                        here.distance_squared(self.points[a].pos)
                            .total_cmp(&here.distance_squared(self.points[b].pos))
                    });
                let Some(next) = near else { break };
                self.points[cur].next = Some(next);
                self.points[next].prev = Some(cur);
                cur = next;
            }
        }
    }

    fn calc_curve(&mut self, i: usize, own_facing: bool) {
        let me = self.points[i].pos;
        let prev = self.points[i].prev.map(|j| self.points[j].pos);
        let next = self.points[i].next.map(|j| self.points[j].pos);
        let p = &mut self.points[i];
        let (dir, out_len, in_len) = match (prev, next) {
            (None, None) => return,
            (None, Some(n)) => (n - me, me.distance(n), me.distance(n)),
            (Some(b), None) => (me - b, me.distance(b), 0.0),
            (Some(b), Some(n)) => (n - b, me.distance(n), 0.0),
        };
        p.has_curve = true;
        p.out_len = out_len;
        p.in_len = in_len;
        p.tangent = if own_facing { facing(p.rot) } else { dir.normalize_or_zero() };
    }

    /// Length of the curve's pull arriving at `i`.
    pub fn in_len(&self, i: usize) -> f32 {
        let p = &self.points[i];
        if p.in_len != 0.0 { p.in_len } else { p.prev.map_or(0.0, |b| self.points[b].out_len) }
    }

    /// The point with this tag, else the one with this object name.
    pub fn find(&self, tag: &str, object: &str) -> Option<usize> {
        if !tag.is_empty() {
            self.points.iter().position(|p| p.tag.eq_ignore_ascii_case(tag))
        } else if !object.is_empty() {
            self.points.iter().position(|p| p.name.eq_ignore_ascii_case(object))
        } else {
            None
        }
    }

    /// The point nearest to `pos` (within 100000 units) that is not one of `except`, among
    /// those with `link_tag` if it is set.
    pub fn nearest(&self, pos: Vec3, except: [Option<usize>; 2], link_tag: &str) -> Option<usize> {
        (0..self.points.len())
            .filter(|i| !except.contains(&Some(*i)))
            .filter(|&i| link_tag.is_empty() || self.points[i].link_tag.eq_ignore_ascii_case(link_tag))
            .map(|i| (i, pos.distance(self.points[i].pos)))
            .filter(|&(_, d)| d < 100_000.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Where the curve from the point before `to` into `to` is at progress `t` (0..1).
    pub fn curve(&self, to: usize, t: f32) -> Option<Vec3> {
        let end = &self.points[to];
        let start = &self.points[end.prev?];
        let a = start.pos + start.tangent * start.out_len * t;
        let b = end.pos - end.tangent * self.in_len(to) * (1.0 - t);
        Some(a.lerp(b, ease(t)))
    }
}
