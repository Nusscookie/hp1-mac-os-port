//! Particle systems (HP1's `ParticleFX` objects: fire, glows, sparkles, spell trails); see
//! `specs/engine/Particles.md`. The original does all of this in native code, which is lost; the
//! rules here follow SurrealEngine's reconstruction.

use glam::{Mat3, Vec3};
use hp1_data::DataValue;

use crate::camera::View;
use crate::props::ActorProps;
use crate::units::rotator_matrix;

/// A setting with a random part: `base + rand × (0..1)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Range {
    pub base: f32,
    pub rand: f32,
}

/// A colour with a random part per channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorRange {
    pub base: [u8; 4],
    pub rand: [u8; 4],
}

fn range(p: &ActorProps, key: &str) -> Range {
    match p.get(key).and_then(DataValue::struct_bytes) {
        Some(b) if b.len() >= 8 => Range {
            base: f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            rand: f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        },
        _ => Range::default(),
    }
}

fn color_range(p: &ActorProps, key: &str) -> ColorRange {
    match p.get(key).and_then(DataValue::struct_bytes) {
        Some(b) if b.len() >= 8 => {
            ColorRange { base: [b[0], b[1], b[2], b[3]], rand: [b[4], b[5], b[6], b[7]] }
        }
        _ => ColorRange::default(),
    }
}

/// How the particles are drawn (`RenderPrimitive`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    /// A streak along the direction of travel.
    Line,
    /// A square facing the camera (also used for the kinds not done yet).
    Billboard,
}

/// How a system's particles blend (`Style`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Masked,
    Translucent,
    Modulated,
}

/// A particle system's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub per_sec: Range,
    pub source: [Range; 3],
    pub spread: [Range; 2],
    pub speed: Range,
    pub lifetime: Range,
    pub color_start: ColorRange,
    pub color_end: ColorRange,
    pub alpha_start: Range,
    pub alpha_end: Range,
    pub width: Range,
    pub length: Range,
    pub end_scale: Range,
    pub spin_rate: Range,
    pub update: bool,
    pub velocity_relative: bool,
    pub system_relative: bool,
    pub parent_blend: f32,
    pub alpha_delay: f32,
    pub color_delay: f32,
    pub size_delay: f32,
    pub alpha_grow: f32,
    pub size_grow: f32,
    pub chaos: f32,
    pub chaos_delay: f32,
    pub attraction: Vec3,
    pub damping: f32,
    pub gravity_modifier: f32,
    pub gravity: Vec3,
    pub max_alive: usize,
    pub max_emitted: u32,
    /// Up to five textures, one picked at random per particle; else the object's `Texture`.
    pub textures: Vec<String>,
    pub primitive: Primitive,
    pub emit: bool,
    pub blend: Blend,
}

impl Settings {
    pub fn from_props(p: &ActorProps) -> Self {
        let mut textures: Vec<String> = (0..5)
            .map(|i| p.text(&if i == 0 { "Textures".to_owned() } else { format!("Textures[{i}]") }))
            .filter(|t| !t.is_empty())
            .collect();
        if textures.is_empty() && !p.text("Texture").is_empty() {
            textures.push(p.text("Texture"));
        }
        Self {
            per_sec: range(p, "ParticlesPerSec"),
            // Depth runs along the system's forward axis, width to its right, height up.
            source: [range(p, "SourceDepth"), range(p, "SourceWidth"), range(p, "SourceHeight")],
            spread: [range(p, "AngularSpreadWidth"), range(p, "AngularSpreadHeight")],
            speed: range(p, "Speed"),
            lifetime: range(p, "Lifetime"),
            color_start: color_range(p, "ColorStart"),
            color_end: color_range(p, "ColorEnd"),
            alpha_start: range(p, "AlphaStart"),
            alpha_end: range(p, "AlphaEnd"),
            width: range(p, "SizeWidth"),
            length: range(p, "SizeLength"),
            end_scale: range(p, "SizeEndScale"),
            spin_rate: range(p, "SpinRate"),
            update: p.bool("bUpdate"),
            velocity_relative: p.bool("bVelocityRelative"),
            system_relative: p.bool("bSystemRelative"),
            parent_blend: p.f32("ParentBlend", 0.0),
            alpha_delay: p.f32("AlphaDelay", 0.0),
            color_delay: p.f32("ColorDelay", 0.0),
            size_delay: p.f32("SizeDelay", 0.0),
            alpha_grow: p.f32("AlphaGrowPeriod", 0.0),
            size_grow: p.f32("SizeGrowPeriod", 0.0),
            chaos: p.f32("Chaos", 0.0),
            chaos_delay: p.f32("ChaosDelay", 0.0),
            attraction: Vec3::from(p.vec3("Attraction")),
            damping: p.f32("Damping", 0.0),
            gravity_modifier: p.f32("GravityModifier", 0.0),
            gravity: Vec3::from(p.vec3("Gravity")),
            max_alive: p.int("ParticlesAlive", 0).max(0) as usize,
            max_emitted: p.int("ParticlesMax", 0).max(0) as u32,
            textures,
            primitive: if p.byte("RenderPrimitive", 1) == 0 { Primitive::Line } else { Primitive::Billboard },
            emit: p.bool("bEmit"),
            blend: match p.byte("Style", 3) {
                2 => Blend::Masked,
                4 => Blend::Modulated,
                _ => Blend::Translucent,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    pub pos: Vec3,
    pub vel: Vec3,
    pub age: f32,
    pub life: f32,
    color_start: Vec3,
    color_end: Vec3,
    alpha_start: f32,
    alpha_end: f32,
    width: f32,
    length: f32,
    end_scale: f32,
    spin: f32,
    spin_rate: f32,
    chaos_timer: f32,
    /// Index into `Settings::textures`.
    texture: usize,
}

/// Where the system is this tick.
#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub pos: Vec3,
    /// Pitch, yaw, roll.
    pub rot: [i32; 3],
    /// The object's speed, for `bVelocityRelative`.
    pub vel: Vec3,
    /// The zone's gravity, for `GravityModifier`.
    pub zone_gravity: Vec3,
}

/// The system's own random numbers.
#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn frand(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// A running particle system.
#[derive(Clone, Debug)]
pub struct System {
    pub settings: Settings,
    pub particles: Vec<Particle>,
    residue: f32,
    emitted: u32,
    last: Option<Vec3>,
    rng: Rng,
}

/// Fraction of the way through a change that starts `delay` seconds into a particle's life and
/// ends with it.
fn transition(age: f32, life: f32, delay: f32) -> f32 {
    if age <= delay || life <= delay { 0.0 } else { ((age - delay) / (life - delay)).clamp(0.0, 1.0) }
}

impl System {
    pub fn new(settings: Settings, seed: u64) -> Self {
        Self { settings, particles: Vec::new(), residue: 0.0, emitted: 0, last: None, rng: Rng(seed | 1) }
    }

    pub fn from_props(p: &ActorProps, seed: u64) -> Self {
        let mut s = Self::new(Settings::from_props(p), seed);
        // A placed system remembers what it had done in the editor.
        s.residue = p.f32("EmissionResidue", 0.0);
        s.emitted = p.int("ParticlesEmitted", 0).max(0) as u32;
        s
    }

    fn frand(&mut self) -> f32 {
        self.rng.frand()
    }

    fn pick(&mut self, r: Range) -> f32 {
        r.base + r.rand * self.frand()
    }

    fn pick_color(&mut self, c: ColorRange) -> Vec3 {
        let mut ch =
            |i: usize| ((c.base[i] as f32 + c.rand[i] as f32 * self.frand()) / 255.0).clamp(0.0, 1.0);
        Vec3::new(ch(0), ch(1), ch(2))
    }

    fn emit(&mut self, place: &Place, at: Vec3, pre_age: f32) {
        let s = self.settings.clone();
        let m = Mat3::from_mat4(rotator_matrix(place.rot[0], place.rot[1], place.rot[2]));
        let offset = Vec3::new(
            (self.frand() - 0.5) * self.pick(s.source[0]),
            (self.frand() - 0.5) * self.pick(s.source[1]),
            (self.frand() - 0.5) * self.pick(s.source[2]),
        );
        let mut pos = m * offset;
        if !s.system_relative {
            pos += at;
        }
        let yaw = ((self.frand() - 0.5) * self.pick(s.spread[0])).to_radians();
        let pitch = ((self.frand() - 0.5) * self.pick(s.spread[1])).to_radians();
        let dir = Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin());
        let mut vel = m * dir * self.pick(s.speed);
        if s.velocity_relative {
            vel += place.vel;
        }
        let p = Particle {
            pos,
            vel,
            age: pre_age,
            life: self.pick(s.lifetime).max(0.01),
            color_start: self.pick_color(s.color_start),
            color_end: self.pick_color(s.color_end),
            alpha_start: self.pick(s.alpha_start),
            alpha_end: self.pick(s.alpha_end),
            width: self.pick(s.width),
            length: self.pick(s.length),
            end_scale: self.pick(s.end_scale),
            spin_rate: self.pick(s.spin_rate),
            spin: self.frand() * std::f32::consts::TAU,
            chaos_timer: 0.0,
            texture: if s.textures.is_empty() {
                0
            } else {
                ((self.frand() * s.textures.len() as f32) as usize).min(s.textures.len() - 1)
            },
        };
        self.particles.push(p);
        self.emitted += 1;
    }

    fn simulate(&mut self, place: &Place, dt: f32) {
        let s = &self.settings;
        let mut gravity = s.gravity;
        if s.gravity_modifier != 0.0 {
            gravity += place.zone_gravity * s.gravity_modifier;
        }
        let origin = if s.system_relative { Vec3::ZERO } else { place.pos };
        let (attraction, damping, chaos, chaos_delay) = (s.attraction, s.damping, s.chaos, s.chaos_delay);
        let rng = &mut self.rng;
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.life {
                return false;
            }
            p.vel += gravity * dt;
            if attraction != Vec3::ZERO {
                p.vel -= (p.pos - origin) * attraction * dt;
            }
            if damping != 0.0 {
                p.vel *= (1.0 - damping * dt).max(0.0);
            }
            if chaos != 0.0 {
                p.chaos_timer += dt;
                if p.chaos_timer >= chaos_delay {
                    p.chaos_timer = 0.0;
                    let r = Vec3::new(rng.frand(), rng.frand(), rng.frand());
                    p.vel += (r - Vec3::splat(0.5)) * 2.0 * chaos;
                }
            }
            p.pos += p.vel * dt;
            p.spin += p.spin_rate * dt;
            true
        });
    }

    /// One tick. False once a system with a particle budget has used it up and its last
    /// particle is gone: then the object goes away.
    pub fn tick(&mut self, place: &Place, dt: f32) -> bool {
        let last = *self.last.get_or_insert(place.pos);
        if self.settings.update {
            self.simulate(place, dt);
        }
        let s = &self.settings;
        let max = s.max_emitted;
        if s.emit && (max == 0 || self.emitted < max) {
            // ParentBlend fades a system out; scripts set it to 1 to switch one off.
            let blend = s.parent_blend.clamp(0.0, 1.0);
            let rate = self.pick(self.settings.per_sec).max(0.0) * (1.0 - blend);
            self.residue += rate * dt;
            let mut count = self.residue as u32;
            self.residue -= count as f32;
            count = count.min(500);
            if max > 0 {
                count = count.min(max - self.emitted);
            }
            // New particles are spread along the way the system moved this tick, and over the
            // tick's time.
            for i in 0..count {
                let t = if count > 1 { i as f32 / (count - 1) as f32 } else { 1.0 };
                self.emit(place, last.lerp(place.pos, t), dt * (1.0 - t));
            }
        }
        self.last = Some(place.pos);
        let alive = self.settings.max_alive;
        if alive > 0 && self.particles.len() > alive {
            let extra = self.particles.len() - alive;
            self.particles.drain(..extra);
        }
        !(max > 0 && self.emitted >= max && self.particles.is_empty())
    }

    /// Lets the system run out: no more particles, then gone (`Shutdown`).
    pub fn shutdown(&mut self) -> bool {
        if self.emitted == 0 {
            return false;
        }
        self.settings.max_emitted = self.emitted;
        true
    }

    /// The particles as camera-facing quads for a view, with the system at `pos`.
    pub fn sprites(&self, pos: Vec3, view: &View) -> Vec<Sprite<'_>> {
        let s = &self.settings;
        let visibility = 1.0 - s.parent_blend.clamp(0.0, 1.0);
        let origin = if s.system_relative { pos } else { Vec3::ZERO };
        let forward = (view.target - view.pos).normalize_or_zero();
        let right = Vec3::Z.cross(forward).normalize_or_zero();
        let up = forward.cross(right);
        let mut out = Vec::new();
        for p in &self.particles {
            let Some(texture) = s.textures.get(p.texture) else { continue };
            let life = p.age / p.life;
            let mut alpha =
                p.alpha_start + (p.alpha_end - p.alpha_start) * transition(p.age, p.life, s.alpha_delay);
            if s.alpha_grow > 0.0 && life < s.alpha_grow {
                alpha *= life / s.alpha_grow;
            }
            let alpha = alpha.clamp(0.0, 1.0) * visibility;
            if alpha <= 0.002 {
                continue;
            }
            let color = p.color_start.lerp(p.color_end, transition(p.age, p.life, s.color_delay)) * alpha;
            let mut scale = 1.0 + (p.end_scale - 1.0) * transition(p.age, p.life, s.size_delay);
            if s.size_grow > 0.0 && life < s.size_grow {
                scale *= life / s.size_grow;
            }
            let (half_w, half_l) = (p.width * scale * 0.5, p.length * scale * 0.5);
            if half_w <= 0.0 || half_l <= 0.0 {
                continue;
            }
            let center = p.pos + origin;
            let speed = p.vel.length();
            let (side, along) = if s.primitive == Primitive::Line && speed > 0.001 {
                let along = p.vel * (half_l / speed);
                let side = p.vel.cross(view.pos - center);
                let len = side.length();
                (if len > 0.0001 { side * (half_w / len) } else { right * half_w }, along)
            } else {
                let (sn, cs) = p.spin.sin_cos();
                ((right * cs + up * sn) * half_w, (up * cs - right * sn) * half_l)
            };
            out.push(Sprite {
                texture,
                corners: [
                    center - side - along,
                    center + side - along,
                    center + side + along,
                    center - side + along,
                ],
                color: color.into(),
            });
        }
        out
    }
}

/// One particle to draw: corners with texture coordinates (0,0), (1,0), (1,1), (0,1).
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite<'a> {
    pub texture: &'a str,
    pub corners: [Vec3; 4],
    pub color: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        let one = Range { base: 1.0, rand: 0.0 };
        let zero = Range::default();
        Settings {
            per_sec: Range { base: 10.0, rand: 0.0 },
            source: [zero; 3],
            spread: [zero; 2],
            speed: Range { base: 100.0, rand: 0.0 },
            lifetime: one,
            color_start: ColorRange { base: [255, 128, 0, 0], rand: [0; 4] },
            color_end: ColorRange::default(),
            alpha_start: one,
            alpha_end: zero,
            width: Range { base: 8.0, rand: 0.0 },
            length: Range { base: 8.0, rand: 0.0 },
            end_scale: one,
            spin_rate: zero,
            update: true,
            velocity_relative: false,
            system_relative: false,
            parent_blend: 0.0,
            alpha_delay: 0.0,
            color_delay: 0.0,
            size_delay: 0.0,
            alpha_grow: 0.0,
            size_grow: 0.0,
            chaos: 0.0,
            chaos_delay: 0.0,
            attraction: Vec3::ZERO,
            damping: 0.0,
            gravity_modifier: 0.0,
            gravity: Vec3::ZERO,
            max_alive: 0,
            max_emitted: 0,
            textures: vec!["Test.Dot".into()],
            primitive: Primitive::Billboard,
            emit: true,
            blend: Blend::Translucent,
        }
    }

    fn place() -> Place {
        // Pointing straight up, as most systems do.
        Place {
            pos: Vec3::new(0.0, 0.0, 100.0),
            rot: [16384, 0, 0],
            vel: Vec3::ZERO,
            zone_gravity: Vec3::ZERO,
        }
    }

    #[test]
    fn systems_emit_at_their_rate_upwards_and_particles_die_with_age() {
        let mut s = System::new(settings(), 7);
        for _ in 0..60 {
            assert!(s.tick(&place(), 1.0 / 60.0));
        }
        // Ten a second; the first ones are about to die at one second.
        assert!((9..=10).contains(&s.particles.len()), "{}", s.particles.len());
        let p = &s.particles[0];
        assert!(p.vel.z > 99.0 && p.pos.z > 150.0);
        // Half-way through its life a particle is half as bright and half-way to its end colour.
        let mut half = System::new(settings(), 1);
        half.particles.push(Particle { age: 0.5, ..p.clone() });
        let view = View { pos: Vec3::new(-100.0, 0.0, 100.0), target: Vec3::new(0.0, 0.0, 100.0) };
        let sprite = &half.sprites(Vec3::ZERO, &view)[0];
        assert!((sprite.color[0] - 0.25).abs() < 1e-4);
        assert!((sprite.corners[2] - sprite.corners[0]).length() > 11.0);
    }

    #[test]
    fn a_budget_ends_the_system_once_its_particles_are_gone() {
        let mut s = System::new(
            Settings { max_emitted: 3, per_sec: Range { base: 100.0, rand: 0.0 }, ..settings() },
            3,
        );
        let mut ticks = 0;
        while s.tick(&place(), 0.05) {
            ticks += 1;
            assert!(ticks < 100);
        }
        assert_eq!(s.emitted, 3);
        // About a second for the last particle to die.
        assert!((19..=22).contains(&ticks), "{ticks}");
    }

    #[test]
    fn particles_alive_drops_the_oldest() {
        let mut s = System::new(Settings { max_alive: 4, ..settings() }, 5);
        for _ in 0..60 {
            s.tick(&place(), 1.0 / 60.0);
        }
        assert_eq!(s.particles.len(), 4);
        assert!(s.particles.windows(2).all(|w| w[0].age >= w[1].age));
    }
}
