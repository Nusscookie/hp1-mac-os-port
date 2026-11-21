# Particle systems

Used in: everywhere (Tut1: 10 torch fires, a glow over each chocolate frog; later spells,
sparkles, smoke and trails)
Placed objects: anything of class `Engine.ParticleFX` (HP1's `HPParticle.*` presets)
Sources: the `ParticleFX` class's settings and comments, and SurrealEngine's reconstruction
of the native code. **The original emitting, moving and drawing code was native and is lost;
the rules below are SurrealEngine's, not known to be the original's.**

## Settings
Most settings are a base plus a random part: each use picks `base + rand × (0..1)`. Colours
pick each channel the same way (0–255, clamped). A system has up to five textures; each new
particle picks one at random. Without any it uses the object's `Texture`.

## Each tick
1. The system's age goes up.
2. If `bUpdate` is on, every particle ages; one that reaches its lifetime is gone. The rest
   speed up by `Gravity` (plus the zone's gravity × `GravityModifier`), are pulled back
   towards the system by `Attraction` (per axis, times their distance from it), slow down by
   `Damping` (speed × (1 − damping × tick time), not below 0), get a random push of up to
   `Chaos` in each direction every `ChaosDelay` seconds, move, and spin by their spin rate.
3. If `bEmit` is on and the system has particles left in its budget (`ParticlesMax`, 0 = no
   limit), it emits `ParticlesPerSec` (picked anew each tick) × (1 − `ParentBlend`) a second.
   Fractions carry over to the next tick. At most 500 in one tick. The new ones are spread
   evenly along the way the system moved this tick (the last one at its current place) and
   aged by the part of the tick after them.
4. If there are more than `ParticlesAlive` (0 = no limit), the oldest go.
5. A system whose budget is used up and whose particles are all gone removes its object.

A placed system starts with the `EmissionResidue` and `ParticlesEmitted` saved in the map.

## A new particle
- Position: a random point in a box `SourceDepth` deep (along the system's facing),
  `SourceWidth` wide and `SourceHeight` high, turned with the system. Added to the system's
  place unless `bSystemRelative` (then particles are kept relative to the system and move
  with it).
- Direction: the system's facing, turned by a random part of `AngularSpreadWidth` (sideways)
  and `AngularSpreadHeight` (up and down), in degrees, each from −half to +half. Speed:
  `Speed`. With `bVelocityRelative` the system's own speed is added.
- Lifetime (at least 0.01 s), start and end colour, start and end alpha, width, length, end
  scale and spin rate are picked once; the spin starts at a random angle.

## Drawing
- Alpha goes from start to end over the life after `AlphaDelay` seconds; colour likewise
  after `ColorDelay`; size from 1 to the end scale after `SizeDelay`. During the first
  `AlphaGrowPeriod` (a part of the life) alpha grows from 0; the same for size with
  `SizeGrowPeriod`. `ParentBlend` fades the whole system (1 = invisible).
- A particle with alpha ≤ 0.002 or a size of 0 or less is not drawn.
- The colour is multiplied by alpha. Style: masked or modulated if the object says so,
  otherwise added on top (translucent). Always unlit and seen from both sides.
- Lines (`RenderPrimitive` 0) are streaks along the direction of travel, turned towards the
  camera. Everything else is drawn as a square facing the camera, turned by the particle's
  spin. Texture corners: (0,0) at −side −up, (1,1) at +side +up.

## Open points
- `bPrime`, `bSteadyState`, `Period`, `Decay`, `DripTime`, `Elasticity`, `Distribution`,
  `Pattern`, wind, `ColorPalette` and the liquid / shard / tri-tube primitives are not
  handled (SurrealEngine doesn't either).
- Animated particle textures show their first frame.
- Zone gravity is the default zone's for every zone.

## Uses from the converted data
Each system's settings above (the `FloatParams` and `ColorParams` structs as raw bytes:
two little-endian floats; four colour bytes R, G, B, A then four random ones), `Style`,
`Texture`, `Engine.ZoneInfo`'s `ZoneGravity`, the textures.

## Changes from the original
- None known (the original's code is not available).
