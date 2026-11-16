# Walking and falling

Used in: everywhere
Placed objects: every character that moves on its own feet (Harry, the students, teachers, gnomes, …)

## What it does
A walking character is an upright box (half-width `CollisionRadius`, half-height `CollisionHeight`)
whose centre is its position; its feet are `CollisionHeight` below the centre. Each tick it either
walks on a floor or falls through the air.

## States
- **Walking**: the wanted direction (from the player's keys or the character's AI) speeds the
  character up by `AccelRate` per second towards `GroundSpeed`; without input, ground friction
  (`ZoneGroundFriction`) slows it down. The move is swept through the level:
  - a wall stops the move; the character first tries to *step up* (lift by `MaxStepHeight`, move,
    put back down), which lets it walk up stairs and over small bumps;
  - if that doesn't help, it *slides* along the wall with what is left of the move;
  - after moving, it looks for the floor below its feet, at most `MaxStepHeight` down. A floor
    that is flat enough (the surface faces up more than about 45°) keeps it walking and its feet are
    put onto it, so it follows ramps and steps down stairs. No floor, or a too steep one, starts
    falling.
  - Jumping sets the upward speed to `JumpZ` and starts falling.
- **Falling**: gravity (`ZoneGravity`, about −512 units/s² in this game) pulls it down. The wanted
  direction still steers it, scaled by `AirControl`. Hitting a flat enough floor lands it (back to
  walking); hitting a wall or a steep slope slides along it.

## Reacts to
- Spawning inside the level geometry: it is moved to the nearest free spot (first upwards, then
  around), so placed characters that touch the floor a little are not stuck.

## Uses from the converted data
- CollisionRadius, CollisionHeight, GroundSpeed, AccelRate, AirControl, JumpZ, MaxStepHeight
- ZoneGravity, ZoneGroundFriction of the zone the character is in

## Changes from the original
- None known yet.
