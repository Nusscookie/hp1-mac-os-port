//! Spell patterns and how a drawing of one is scored (`specs/spells/SpellLesson.md`, Patterns).
//! The original's scoring is native and lost; this follows SurrealEngine's.

use glam::Vec2;

/// A pattern: points in the unit square (x right, y down), one line through them in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pattern {
    pub points: Vec<Vec2>,
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len = ab.length_squared();
    let t = if len > 0.0 { ((p - a).dot(ab) / len).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

impl Pattern {
    pub fn from_data(g: &hp1_data::Gesture) -> Self {
        Self { points: g.points.iter().map(|p| Vec2::new(p[0], p[1])).collect() }
    }

    /// Distance from `p` to the line (1000 for an empty pattern).
    pub fn distance(&self, p: Vec2) -> f32 {
        match self.points.as_slice() {
            [] => 1000.0,
            [only] => p.distance(*only),
            pts => pts.windows(2).map(|w| segment_distance(p, w[0], w[1])).fold(f32::INFINITY, f32::min),
        }
    }

    pub fn length(&self) -> f32 {
        self.points.windows(2).map(|w| w[0].distance(w[1])).sum()
    }

    /// The point a part `f` (0..1) of the way along the line.
    pub fn along(&self, f: f32) -> Vec2 {
        let Some(&first) = self.points.first() else { return Vec2::splat(0.5) };
        let target = self.length() * f.clamp(0.0, 1.0);
        let mut walked = 0.0;
        for w in self.points.windows(2) {
            let len = w[0].distance(w[1]);
            if len > 0.0 && walked + len >= target {
                return w[0].lerp(w[1], (target - walked) / len);
            }
            walked += len;
        }
        *self.points.last().unwrap_or(&first)
    }

    /// Score of a drawing, 0..1: how much of the line was traced, times the share of drawn
    /// points near it. `None` entries are unused slots.
    pub fn score(&self, drawn: &[Option<Vec2>], accuracy: f32) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        let drawn: Vec<Vec2> = drawn.iter().flatten().copied().collect();
        if drawn.is_empty() {
            return 0.0;
        }
        let good = drawn.iter().filter(|&&p| self.distance(p) <= accuracy).count();
        let precision = good as f32 / drawn.len() as f32;
        const SAMPLES: usize = 100;
        let covered = (0..SAMPLES)
            .filter(|&s| {
                let spot = self.along((s as f32 + 0.5) / SAMPLES as f32);
                drawn.iter().any(|p| p.distance_squared(spot) <= accuracy * accuracy)
            })
            .count();
        let score = covered as f32 / SAMPLES as f32 * precision;
        if score >= 0.97 { 1.0 } else { score }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l_shape() -> Pattern {
        Pattern { points: vec![Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0)] }
    }

    #[test]
    fn distance_and_places_along_the_line() {
        let p = l_shape();
        assert_eq!(p.distance(Vec2::new(0.25, 0.5)), 0.25);
        assert_eq!(p.length(), 2.0);
        assert_eq!(p.along(0.75), Vec2::new(0.5, 1.0));
        assert_eq!(Pattern::default().distance(Vec2::ZERO), 1000.0);
    }

    #[test]
    fn a_full_trace_scores_one_half_a_trace_half_and_strays_scale_it_down() {
        let p = l_shape();
        let trace = |from: f32, to: f32| -> Vec<Option<Vec2>> {
            (0..=200).map(|i| Some(p.along(from + (to - from) * i as f32 / 200.0))).collect()
        };
        assert_eq!(p.score(&trace(0.0, 1.0), 0.03), 1.0);
        let half = p.score(&trace(0.0, 0.5), 0.03);
        assert!((half - 0.5).abs() < 0.02, "{half}");
        let mut strays = trace(0.0, 1.0);
        strays.extend(std::iter::repeat_n(Some(Vec2::new(0.8, 0.2)), 201));
        strays.push(None);
        let s = p.score(&strays, 0.03);
        assert!((s - 0.5).abs() < 0.01, "{s}");
        assert_eq!(p.score(&[None, None], 0.03), 0.0);
    }
}
