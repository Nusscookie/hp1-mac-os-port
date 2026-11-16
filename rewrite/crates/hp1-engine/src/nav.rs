//! Path finding over the level's navigation points (see `specs/engine/Paths.md`).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::Vec3;
use hp1_data::ClassTable;
use hp1_data::level::LevelFile;

use crate::collision::Collider;
use crate::props::ActorProps;

/// How a link may be travelled (bits of a reach spec's flags).
pub mod reach {
    pub const WALK: i32 = 1;
    pub const FLY: i32 = 2;
    pub const SWIM: i32 = 4;
    pub const JUMP: i32 = 8;
    pub const DOOR: i32 = 16;
    pub const SPECIAL: i32 = 32;
    pub const PLAYER_ONLY: i32 = 64;
}

#[derive(Clone, Copy, Debug)]
struct Link {
    to: usize,
    length: f32,
    radius: f32,
    height: f32,
    flags: i32,
}

/// The navigation network of a level. Points are numbered 0..n; `actor(i)` gives the level actor.
pub struct NavGraph {
    pub points: Vec<Vec3>,
    actors: Vec<usize>,
    links: Vec<Vec<Link>>,
}

/// What kind of character is asking.
#[derive(Clone, Copy, Debug)]
pub struct Mover {
    pub radius: f32,
    pub height: f32,
    /// Allowed link kinds, e.g. `WALK | JUMP | DOOR`.
    pub can: i32,
    pub is_player: bool,
}

#[derive(PartialEq)]
struct Open(f32, usize);
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then(self.1.cmp(&o.1))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl NavGraph {
    pub fn new(level: &LevelFile, classes: &ClassTable) -> Self {
        let mut index = HashMap::new();
        let (mut points, mut actors) = (Vec::new(), Vec::new());
        for (i, a) in level.actors.iter().enumerate() {
            let Some(a) = a else { continue };
            if classes.is_a(&a.class, "Engine.NavigationPoint") {
                index.insert(i, points.len());
                points.push(Vec3::from(ActorProps::new(a, classes).vec3("Location")));
                actors.push(i);
            }
        }
        let mut links = vec![Vec::new(); points.len()];
        for r in &level.reach_specs {
            let (Ok(s), Ok(e)) = (usize::try_from(r.start), usize::try_from(r.end)) else { continue };
            let (Some(&from), Some(&to)) = (index.get(&s), index.get(&e)) else { continue };
            if r.pruned || from == to {
                continue;
            }
            // HP1's maps store every link as 500 long, so the real distance is used instead.
            links[from].push(Link {
                to,
                length: points[from].distance(points[to]).max(1.0),
                radius: r.collision_radius as f32,
                height: r.collision_height as f32,
                flags: r.flags,
            });
        }
        Self { points, actors, links }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn link_count(&self) -> usize {
        self.links.iter().map(Vec::len).sum()
    }

    /// Sizes of the groups of points joined by links (points without links are left out).
    pub fn groups(&self) -> Vec<usize> {
        let n = self.points.len();
        let mut group = vec![usize::MAX; n];
        let mut sizes = Vec::new();
        for start in 0..n {
            if group[start] != usize::MAX || !self.linked(start) {
                continue;
            }
            let id = sizes.len();
            let (mut stack, mut size) = (vec![start], 0);
            group[start] = id;
            while let Some(p) = stack.pop() {
                size += 1;
                for l in &self.links[p] {
                    if group[l.to] == usize::MAX {
                        group[l.to] = id;
                        stack.push(l.to);
                    }
                }
            }
            sizes.push(size);
        }
        sizes
    }

    /// The level actor index of point `i`.
    pub fn actor(&self, i: usize) -> usize {
        self.actors[i]
    }

    fn usable(l: &Link, m: &Mover) -> bool {
        let kinds = l.flags & !(reach::PLAYER_ONLY);
        (kinds == 0 || kinds & !m.can == 0)
            && (l.flags & reach::PLAYER_ONLY == 0 || m.is_player)
            && l.radius >= m.radius
            && l.height >= m.height
    }

    /// The shortest chain of points from `from` to `to`, both included.
    pub fn route(&self, from: usize, to: usize, m: &Mover) -> Option<Vec<usize>> {
        let n = self.points.len();
        if from >= n || to >= n {
            return None;
        }
        let mut cost = vec![f32::INFINITY; n];
        let mut prev = vec![usize::MAX; n];
        let mut open = BinaryHeap::new();
        cost[from] = 0.0;
        open.push(Open(self.points[from].distance(self.points[to]), from));
        while let Some(Open(_, p)) = open.pop() {
            if p == to {
                let mut path = vec![to];
                while *path.last()? != from {
                    path.push(prev[*path.last()?]);
                }
                path.reverse();
                return Some(path);
            }
            for l in self.links[p].iter().filter(|l| Self::usable(l, m)) {
                let c = cost[p] + l.length;
                if c < cost[l.to] {
                    cost[l.to] = c;
                    prev[l.to] = p;
                    open.push(Open(c + self.points[l.to].distance(self.points[to]), l.to));
                }
            }
        }
        None
    }

    /// Whether a point belongs to the network (has links).
    pub fn linked(&self, i: usize) -> bool {
        !self.links[i].is_empty()
    }

    /// The nearest point of the network that can be seen from `pos` (a clear line between them),
    /// within `range`.
    pub fn nearest_visible(&self, world: &dyn Collider, pos: Vec3, range: f32) -> Option<usize> {
        let mut by_distance: Vec<(f32, usize)> = self
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| (p.distance(pos), i))
            .filter(|&(d, i)| d <= range && self.linked(i))
            .collect();
        by_distance.sort_by(|a, b| a.0.total_cmp(&b.0));
        by_distance
            .into_iter()
            .take(16)
            .find(|&(_, i)| world.trace(pos, self.points[i], Vec3::ZERO).is_none())
            .map(|(_, i)| i)
    }

    /// Positions to walk through from `start` to `goal` (ending with `goal`), or None.
    pub fn find_path(&self, world: &dyn Collider, start: Vec3, goal: Vec3, m: &Mover) -> Option<Vec<Vec3>> {
        if world.trace(start, goal, Vec3::ZERO).is_none() {
            return Some(vec![goal]);
        }
        let a = self.nearest_visible(world, start, 2000.0)?;
        let b = self.nearest_visible(world, goal, 2000.0)?;
        let mut out: Vec<Vec3> = self.route(a, b, m)?.into_iter().map(|i| self.points[i]).collect();
        out.push(goal);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hp1_data::DataValue;
    use hp1_data::level::{Actor, ReachSpec};

    fn graph() -> NavGraph {
        let at = |x: f32, y: f32| {
            Some(Actor {
                name: "PathNode".into(),
                class: "Engine.PathNode".into(),
                props: [("Location".to_owned(), DataValue::Vector { x, y, z: 0.0 })].into_iter().collect(),
                brush: None,
            })
        };
        // A square 0-1-2-3 plus a narrow shortcut 0-2.
        let spec = |s, e, r| ReachSpec {
            distance: 100,
            start: s,
            end: e,
            collision_radius: r,
            collision_height: 60,
            flags: reach::WALK,
            pruned: false,
        };
        let mut reach_specs = Vec::new();
        for (s, e) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
            reach_specs.push(spec(s, e, 40));
            reach_specs.push(spec(e, s, 40));
        }
        reach_specs.push(spec(0, 2, 10));
        let level = LevelFile {
            actors: vec![at(0.0, 0.0), at(100.0, 0.0), at(100.0, 100.0), at(0.0, 100.0)],
            reach_specs,
            ..Default::default()
        };
        let classes: ClassTable = serde_json::from_str(
            r#"{"classes": {"Engine.Actor": {"defaults": {}},
            "Engine.NavigationPoint": {"parent": "Engine.Actor", "defaults": {}},
            "Engine.PathNode": {"parent": "Engine.NavigationPoint", "defaults": {}}}}"#,
        )
        .unwrap();
        NavGraph::new(&level, &classes)
    }

    #[test]
    fn small_characters_take_the_narrow_shortcut() {
        let g = graph();
        let small = Mover { radius: 8.0, height: 20.0, can: reach::WALK, is_player: false };
        assert_eq!(g.route(0, 2, &small), Some(vec![0, 2]));
        let big = Mover { radius: 20.0, ..small };
        let r = g.route(0, 2, &big).unwrap();
        assert_eq!(r.len(), 3, "{r:?}");
        assert_eq!(g.route(2, 0, &small).unwrap().len(), 3, "the shortcut goes one way only");
    }
}
