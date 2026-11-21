//! Every Flavour Beans and Chocolate Frogs (see `specs/props/Pickups.md`).

use glam::Vec3;

use crate::props::ActorProps;
use crate::world::{ActorId, Animation, Behaviour, World};

/// How long a collected item flies towards the HUD.
const FLY_TIME: f32 = 0.25;
/// How often the original checks whether the flight is over.
const FLY_POLL: f32 = 0.1;

/// A collected item flying to a point on the screen, then gone.
struct Flight {
    /// The point, in `World::camera_to_world` terms.
    to: Vec3,
    left: f32,
    poll: f32,
}

impl Flight {
    fn new(to: Vec3) -> Self {
        Self { to, left: FLY_TIME, poll: FLY_POLL }
    }

    /// One tick; true once the item is gone.
    fn tick(&mut self, w: &mut World, me: ActorId, dt: f32) -> bool {
        self.left -= dt;
        if let Some(dest) = w.camera_to_world(self.to)
            && let Some(e) = w.get_mut(me)
        {
            let steps = self.left / dt;
            e.pos = if steps > 1.0 { e.pos + (dest - e.pos) / steps } else { dest };
        }
        self.poll -= dt;
        if self.poll <= 0.0 {
            if self.left <= 0.0 {
                w.remove(me);
                return true;
            }
            self.poll = FLY_POLL;
        }
        false
    }
}

fn is_player(w: &World, id: ActorId) -> bool {
    w.player == Some(id)
}

// ---------------------------------------------------------------------------------------------
// Beans

/// A bean lying in the level, spinning until Harry collects it.
pub struct Jellybean {
    flight: Option<Flight>,
}

/// Rotation units a second.
const BEAN_SPIN: f32 = 30000.0;
/// Harry collects a bean whose centre is this close to his.
const BEAN_REACH: f32 = 60.0;
pub const BEAN_SOUND: &str = "HPSounds.magic_sfx.pickup11";

impl Jellybean {
    pub fn new() -> Self {
        Self { flight: None }
    }

    fn collect(&mut self, w: &mut World, me: ActorId) {
        w.sound(me, BEAN_SOUND);
        w.stats.beans += 1;
        w.hud.show_beans();
        w.set_collide_actors(me, false);
        self.flight = Some(Flight::new(Vec3::new(0.75, 0.75, 150.0)));
    }
}

impl Default for Jellybean {
    fn default() -> Self {
        Self::new()
    }
}

impl Behaviour for Jellybean {
    fn tick(&mut self, w: &mut World, me: ActorId, dt: f32) {
        if let Some(f) = &mut self.flight {
            f.tick(w, me, dt);
            return;
        }
        let harry = w.player.and_then(|h| w.get(h)).map(|h| h.pos);
        let Some(e) = w.get_mut(me) else { return };
        // The original adds the turn to the whole-number yaw, dropping the fraction.
        e.rot[1] = ((e.rot[1] as f32 + BEAN_SPIN * dt) as i32) & 0xffff;
        if harry.is_some_and(|h| h.distance(e.pos) < BEAN_REACH) {
            self.collect(w, me);
        }
    }

    fn touch(&mut self, w: &mut World, me: ActorId, other: ActorId) {
        if self.flight.is_none() && is_player(w, other) {
            self.collect(w, me);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Chocolate frogs

pub const RIBBIT: &str = "HPSounds.critters_sfx.frog_ribbit";

/// What the frog is doing in its loop.
#[derive(Clone, Copy, Debug, PartialEq)]
enum FrogStep {
    Croak,
    Hop,
    /// Breathing slowly for a random time.
    Breath,
    /// Breathing on to the end of the current loop.
    BreathEnd,
    HopAgain,
}

pub struct ChocolateFrog {
    pickup: String,
    step: Option<FrogStep>,
    /// Seconds before the next step.
    wait: f32,
    /// Animation speed.
    rate: f32,
    flight: Option<Flight>,
}

impl ChocolateFrog {
    pub fn new(p: &ActorProps) -> Self {
        Self { pickup: p.text("Pickup"), step: None, wait: 0.0, rate: 1.0, flight: None }
    }

    fn play(&mut self, w: &mut World, me: ActorId, name: &str, rate: f32) {
        self.rate = rate;
        if let Some(e) = w.get_mut(me) {
            e.anim = Some(Animation::looping(name));
        }
    }

    /// The time from the start of `name` to its last frame, at `rate`.
    fn once(w: &World, me: ActorId, name: &str, rate: f32) -> f32 {
        w.sequence(me, name).map_or(1.0, |s| s.once()) / rate
    }

    fn start(&mut self, w: &mut World, me: ActorId, step: FrogStep) {
        self.step = Some(step);
        match step {
            FrogStep::Croak => {
                self.play(w, me, "CROAK", 1.0);
                self.wait = Self::once(w, me, "CROAK", 1.0);
            }
            FrogStep::Hop | FrogStep::HopAgain => {
                w.sound(me, RIBBIT);
                self.play(w, me, "hop", 1.0);
                self.wait = Self::once(w, me, "hop", 1.0);
            }
            FrogStep::Breath => {
                self.play(w, me, "BREATH", 0.6);
                self.wait = w.frand() * 3.0;
            }
            FrogStep::BreathEnd => {
                // Finishing a looping animation plays on to the last frame of the loop it is
                // in; past that frame it ends at once.
                let time = w.get(me).and_then(|e| e.anim.as_ref()).map_or(0.0, |a| a.time);
                let (once, cycle) = w.sequence(me, "BREATH").map_or((1.0, 1.0), |s| (s.once(), s.cycle()));
                let at = time % cycle;
                self.wait = if at < once { (once - at) / self.rate } else { 0.0 };
            }
        }
    }

    fn next(step: FrogStep) -> FrogStep {
        match step {
            FrogStep::Croak => FrogStep::Hop,
            FrogStep::Hop => FrogStep::Breath,
            FrogStep::Breath => FrogStep::BreathEnd,
            FrogStep::BreathEnd => FrogStep::HopAgain,
            FrogStep::HopAgain => FrogStep::Croak,
        }
    }
}

impl Behaviour for ChocolateFrog {
    fn tick(&mut self, w: &mut World, me: ActorId, dt: f32) {
        let rate = self.rate;
        if let Some(a) = w.get_mut(me).and_then(|e| e.anim.as_mut()) {
            a.time += dt * rate;
        }
        if let Some(f) = &mut self.flight {
            f.tick(w, me, dt);
            return;
        }
        match self.step {
            None => self.start(w, me, FrogStep::Croak),
            Some(step) => {
                self.wait -= dt;
                if self.wait <= 0.0 {
                    self.start(w, me, Self::next(step));
                }
            }
        }
    }

    fn touch(&mut self, w: &mut World, me: ActorId, other: ActorId) {
        if self.flight.is_some() || !is_player(w, other) {
            return;
        }
        let pickup = std::mem::take(&mut self.pickup);
        w.sound(me, &pickup);
        w.stats.add_health(10.0);
        w.set_collide_actors(me, false);
        self.flight = Some(Flight::new(Vec3::new(-0.75, 0.75, 150.0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::View;
    use crate::world::{Entity, Output, Sequence};
    use hp1_data::ClassTable;
    use hp1_data::level::LevelFile;

    fn world() -> (World, ActorId) {
        let mut w = World::new(LevelFile::default(), ClassTable::default());
        let harry = w.spawn(Entity::pawn("Harry", "HarryPotter.Harry", Vec3::ZERO, 15.0, 42.0, true));
        w.player = Some(harry);
        w.view = Some(View { pos: Vec3::new(0.0, 0.0, 100.0), target: Vec3::new(1.0, 0.0, 100.0) });
        (w, harry)
    }

    fn thing(w: &mut World, at: Vec3, b: Box<dyn Behaviour>) -> ActorId {
        let mut e = Entity::pawn("Thing", "Test.Thing", at, 10.0, 10.0, false);
        (e.is_pawn, e.collide_actors) = (false, true);
        e.behaviour = Some(b);
        w.spawn(e)
    }

    #[test]
    fn a_bean_spins_is_collected_near_harry_and_flies_to_the_top_right() {
        let (mut w, harry) = world();
        let bean = thing(&mut w, Vec3::new(200.0, 0.0, 0.0), Box::new(Jellybean::new()));
        w.tick(0.1);
        assert_eq!(w.get(bean).unwrap().rot[1], 3000);
        assert_eq!(w.stats.beans, 0);
        w.get_mut(harry).unwrap().pos = Vec3::new(150.0, 0.0, 0.0);
        w.tick(0.1);
        assert_eq!(w.stats.beans, 1);
        assert!(w.outputs.iter().any(|o| matches!(o, Output::Sound { path, .. } if path == BEAN_SOUND)));
        // 150 ahead of the view, 112.5 right (+y when looking along +x) and up.
        let dest = Vec3::new(150.0, 112.5, 212.5);
        let (mut t, mut last) = (0.0f32, Vec3::ZERO);
        while let Some(e) = w.get(bean) {
            last = e.pos;
            w.tick(1.0 / 60.0);
            t += 1.0 / 60.0;
            assert!(t < 1.0);
        }
        assert!(last.distance(dest) < 1e-3, "{last}");
        // Gone at the first 0.1 s check after the 0.25 s flight (each check can take one tick
        // more when the ticks don't add up to 0.1 exactly).
        assert!((0.29..0.36).contains(&t), "{t}");
    }

    #[test]
    fn a_frog_croaks_hops_breathes_and_gives_health_when_touched() {
        let (mut w, harry) = world();
        w.stats.health = 30.0;
        let frog = thing(
            &mut w,
            Vec3::new(500.0, 0.0, 0.0),
            Box::new(ChocolateFrog {
                pickup: "Frog.Pickup".into(),
                step: None,
                wait: 0.0,
                rate: 1.0,
                flight: None,
            }),
        );
        let one = Sequence { frames: 11, rate: 10.0 };
        w.set_sequences(frog, [("CROAK".into(), one), ("hop".into(), one), ("BREATH".into(), one)]);
        w.tick(0.1);
        assert_eq!(w.get(frog).unwrap().anim.as_ref().unwrap().name, "CROAK");
        // CROAK lasts 1 s to its last frame, then the ribbit and the hop.
        for _ in 0..10 {
            w.tick(0.1);
        }
        assert_eq!(w.get(frog).unwrap().anim.as_ref().unwrap().name, "hop");
        assert!(w.outputs.iter().any(|o| matches!(o, Output::Sound { path, .. } if path == RIBBIT)));
        w.get_mut(harry).unwrap().pos = Vec3::new(500.0, 0.0, 0.0);
        w.tick(0.1);
        assert_eq!(w.stats.health, 40.0);
        assert!(w.outputs.iter().any(|o| matches!(o, Output::Sound { path, .. } if path == "Frog.Pickup")));
        for _ in 0..40 {
            w.tick(1.0 / 60.0);
        }
        assert!(w.get(frog).is_none());
    }
}
