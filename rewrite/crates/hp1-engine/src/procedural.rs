//! Textures the game draws itself every frame: fire.
//!
//! A fire texture is a grid of heat values (0–255) shown through a 256-colour palette. Each step,
//! "sparks" placed by the artist add heat (single dots, drifting embers, falling embers, twirls,
//! lightning), then every cell becomes the average of itself, its left and right neighbours and
//! the cell below, minus a little heat loss, and the image moves up one row when the fire rises.
//! That makes heat spread and fade like flames.
//!
//! The spark behaviours follow the SurrealEngine build on `main` so both look alike.

use hp1_data::Spark;

/// A small fast random number source (xorshift), seeded per texture.
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// 0..=255
    pub fn byte(&mut self) -> i32 {
        (self.next_u32() >> 24) as i32
    }
    /// -1..1
    pub fn signed(&mut self) -> f32 {
        self.byte() as f32 / 255.0 * 2.0 - 1.0
    }
}

#[derive(Clone, Copy, Debug)]
enum Particle {
    Twirl { x: f32, y: f32, angle: f32, spin: f32, heat: u8, age: u8 },
    Drift { x: f32, y: f32, dx: f32, dy: f32, heat: i32, decay: i32 },
    Fall { x: f32, y: f32, dx: f32, dy: f32, heat: u8, age: u8 },
}

/// Spark kinds (the numbers stored in the texture).
mod kind {
    pub const BURN: u8 = 0;
    pub const PULSE: u8 = 2;
    pub const SIGNAL: u8 = 3;
    pub const BLAZE: u8 = 4;
    pub const OZ_HAS_SPOKEN: u8 = 5;
    pub const CONE: u8 = 6;
    pub const BLAZE_RIGHT: u8 = 7;
    pub const BLAZE_LEFT: u8 = 8;
    pub const EMIT: u8 = 13;
    pub const SPHERE_LIGHTNING: u8 = 25;
    pub const WHEEL: u8 = 26;
}

#[derive(Clone, Debug)]
pub struct Fire {
    pub width: usize,
    pub height: usize,
    heat: Vec<u8>,
    work: Vec<u8>,
    sparks: Vec<Spark>,
    particles: Vec<Particle>,
    limit: usize,
    rising: bool,
    fade: Vec<u8>,
    rng: Rng,
}

impl Fire {
    pub fn new(
        width: u32,
        height: u32,
        sparks: &[Spark],
        render_heat: u8,
        rising: bool,
        limit: i64,
        seed: u32,
    ) -> Self {
        let (width, height) = (width.max(2) as usize, height.max(1) as usize);
        // Average of four cells, minus the heat loss chosen by RenderHeat.
        let loss = 1.0 - (255.0 - render_heat as f32) / 16.0;
        let fade = (0..4 * 256).map(|i| (i as f32 * 0.25 + loss).clamp(0.0, 255.0) as u8).collect();
        Self {
            width,
            height,
            heat: vec![0; width * height],
            work: vec![0; width * height],
            sparks: sparks.to_vec(),
            particles: Vec::new(),
            limit: limit.max(0) as usize,
            rising,
            fade,
            rng: Rng::new(seed),
        }
    }

    fn set(&mut self, x: i32, y: i32, v: u8) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            self.heat[x as usize + y as usize * self.width] = v;
        }
    }

    fn wrap(&self, x: f32, y: f32) -> (i32, i32) {
        let (w, h) = (self.width as i32, self.height as i32);
        let (mut x, mut y) = (x as i32, y as i32);
        if x < 0 {
            x += w
        } else if x >= w {
            x -= w
        }
        if y < 0 {
            y += h
        } else if y >= h {
            y -= h
        }
        (x, y)
    }

    /// One simulation step.
    pub fn step(&mut self) {
        let (w, h) = (self.width as i32, self.height as i32);
        for i in 0..self.sparks.len() {
            let s = self.sparks[i];
            let can_emit = self.sparks.len() + self.particles.len() < self.limit;
            let [a, b, c, d] = s.params;
            let (sx, sy) = (s.x as f32 + 0.5, s.y as f32 + 0.5);
            let jitter = |rng: &mut Rng| {
                (
                    (s.x as i32 + (rng.byte() * a as i32 + 128) / 256) % w,
                    (s.y as i32 + (rng.byte() * b as i32 + 128) / 256) % h,
                )
            };
            match s.kind {
                kind::BURN => {
                    let v = self.rng.byte() as u8;
                    self.set(s.x as i32, s.y as i32, v);
                }
                kind::PULSE => {
                    let (x, y) = jitter(&mut self.rng);
                    self.set(x, y, s.heat);
                    self.sparks[i].heat = s.heat.wrapping_add(d);
                }
                kind::SIGNAL => {
                    let (x, y) = jitter(&mut self.rng);
                    if s.heat > c {
                        self.set(x, y, s.heat);
                    }
                    self.sparks[i].heat =
                        if (s.heat as i32 + d as i32) < 256 { s.heat + d } else { self.rng.byte() as u8 };
                }
                kind::WHEEL => {
                    if can_emit {
                        self.particles.push(Particle::Twirl {
                            x: sx,
                            y: sy,
                            heat: s.heat,
                            angle: (a as f32 * 360.0 / 256.0).to_radians(),
                            spin: (d as f32 * 16.0 / 256.0 * 360.0 / 256.0).to_radians(),
                            age: b,
                        });
                    }
                    self.sparks[i].params[0] = a.wrapping_add(c);
                }
                kind::EMIT => {
                    if can_emit && self.rng.byte() < 64 {
                        self.particles.push(Particle::Drift {
                            x: sx,
                            y: sy,
                            heat: s.heat as i32,
                            decay: d as i32,
                            dx: (a as i8) as f32 / 128.0,
                            dy: (b as i8) as f32 / 128.0,
                        });
                    }
                }
                kind::OZ_HAS_SPOKEN | kind::BLAZE => {
                    if can_emit && self.rng.byte() < 128 {
                        let dx = self.rng.signed();
                        let dy = if s.kind == kind::BLAZE { self.rng.signed() } else { -0.5 };
                        self.particles.push(Particle::Drift {
                            x: sx,
                            y: sy,
                            heat: s.heat as i32,
                            decay: 5,
                            dx,
                            dy,
                        });
                    }
                }
                kind::BLAZE_LEFT | kind::BLAZE_RIGHT | kind::CONE => {
                    if can_emit && self.rng.byte() < 64 {
                        let r = self.rng.byte() as f32 / 255.0;
                        let (dx, dy, age) = match s.kind {
                            kind::BLAZE_LEFT => (-0.5 - r * 0.5, -0.1, c),
                            kind::BLAZE_RIGHT => (0.5 + r * 0.5, -0.1, c),
                            _ => (r - 0.5, 0.0, 50),
                        };
                        self.particles.push(Particle::Fall { x: sx, y: sy, dx, dy, heat: s.heat, age });
                    }
                }
                kind::SPHERE_LIGHTNING => {
                    if self.rng.byte() >= d as i32 {
                        let angle = (self.rng.byte() as f32 * 360.0 / 256.0).to_radians();
                        let radius = c as f32 * 0.5;
                        let (mut x0, mut y0) = (sx, sy);
                        let (dx, dy) = (angle.cos(), angle.sin());
                        let (c0, c1) = (s.heat as f32, (s.heat / 4) as f32);
                        let mut i = 0.0;
                        while i < radius {
                            let v = (c0 + (c1 - c0) * (i / radius) + 0.5) as u8;
                            let (x, y) = self.wrap(x0 + dx * i, y0 + dy * i);
                            self.set(x, y, v);
                            x0 += self.rng.signed();
                            y0 += self.rng.signed();
                            i += 0.5;
                        }
                    }
                }
                // Sparkle (1) and every kind not listed: a heat dot near the spark.
                _ => {
                    let (x, y) = jitter(&mut self.rng);
                    self.set(x, y, s.heat);
                }
            }
        }

        let mut i = 0;
        while i < self.particles.len() {
            let alive = match self.particles[i] {
                Particle::Twirl { x, y, angle, spin, heat, age } => {
                    if age > 0 {
                        let (px, py) = self.wrap(x, y);
                        self.set(px, py, heat);
                        self.particles[i] = Particle::Twirl {
                            x: x + angle.sin() * 0.5,
                            y: y + angle.cos() * 0.5,
                            angle: angle + spin,
                            spin,
                            heat,
                            age: age - 1,
                        };
                        true
                    } else {
                        false
                    }
                }
                Particle::Drift { x, y, dx, dy, heat, decay } => {
                    let heat = heat - decay;
                    if heat > 0 {
                        let (px, py) = self.wrap(x, y);
                        self.set(px, py, heat.min(255) as u8);
                        self.particles[i] = Particle::Drift { x: x + dx, y: y + dy, dx, dy, heat, decay };
                        true
                    } else {
                        false
                    }
                }
                Particle::Fall { x, y, dx, dy, heat, age } => {
                    let age = age.saturating_sub(1);
                    if age > 0 {
                        let (px, py) = self.wrap(x, y);
                        self.set(px, py, heat);
                        let dy2 = (dy + 0.025).min(1.0);
                        self.particles[i] =
                            Particle::Fall { x: x + dx * 0.5, y: y + dy * 0.5, dx, dy: dy2, heat, age };
                        true
                    } else {
                        false
                    }
                }
            };
            if alive {
                i += 1;
            } else {
                self.particles.swap_remove(i);
            }
        }

        // Spread and cool: each cell from its row (shifted up one when rising) and the row below.
        let (w, h) = (self.width, self.height);
        let rise = self.rising as usize;
        for y in 0..h {
            let src = &self.heat[((y + rise) % h) * w..][..w];
            let next = &self.heat[((y + rise + 1) % h) * w..][..w];
            let dst = &mut self.work[y * w..][..w];
            for x in 0..w {
                let left = src[(x + w - 1) % w] as usize;
                let right = src[(x + 1) % w] as usize;
                dst[x] = self.fade[left + src[x] as usize + right + next[x] as usize];
            }
        }
        std::mem::swap(&mut self.heat, &mut self.work);
    }

    /// The current image through `palette` (256 RGBA colours). With `masked`, heat 0 is clear.
    pub fn rgba(&self, palette: &[[u8; 4]], masked: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.heat.len() * 4);
        for &h in &self.heat {
            let mut c = palette.get(h as usize).copied().unwrap_or([h, h, h, 255]);
            c[3] = if masked && h == 0 { 0 } else { 255 };
            out.extend_from_slice(&c);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heat_rises_and_fades() {
        let spark = Spark { kind: kind::BURN, heat: 255, x: 4, y: 7, params: [0; 4] };
        let mut f = Fire::new(8, 8, &[spark], 255, true, 1024, 1);
        for _ in 0..20 {
            f.step();
        }
        let top: u32 = f.heat[..8].iter().map(|&v| v as u32).sum();
        let bottom: u32 = f.heat[7 * 8..].iter().map(|&v| v as u32).sum();
        assert!(bottom > 0, "the burning spark heats its row");
        assert!(top < bottom * 4 + 1, "heat thins out as it rises");
    }
}
