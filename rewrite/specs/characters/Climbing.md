# Climbing onto ledges (Harry)

Used in: everywhere Harry can pull himself up (Tut1's training course, hubs, sneaking levels)
Placed objects: none; Harry (`HarryPotter.Harry`)
Sources: Harry's mount states (`Mount`, `FallingMount`, `Mounting`, `MountFinish`), his class
setting `MaxMountHeight`, the climb animations' own movement (their root bone), and Pawn's
`Mount` event comment ("called when physics detects a mount situation").

## When a climb starts — OUR OWN RULE, NOT THE ORIGINAL'S
The original engine's physics calls Harry's `Mount(delta)` from native code that is lost, and
SurrealEngine doesn't implement it. So **when** a climb starts is our own rule:

- Harry is walking and walks into something his normal step-up (`MaxStepHeight`, 25) can't get
  over, or he is falling (jumping) and moves into a wall.
- He wants to go into it: his wanted direction points at the wall (at least half-way).
- There is a ledge: lifting his box by `MaxMountHeight` (96.5 for Harry), moving it forward by
  his width (twice `CollisionRadius`) towards the wall, and putting it down again finds a
  walkable floor where his box fits.
- The ledge's floor puts his centre more than `MaxStepHeight` and at most `MaxMountHeight`
  above where it is now.

Then `delta` is the way from his centre now to his centre standing on the ledge (the
destination), as the original's `Mount(delta)` gets it.

## What a climb does (from Harry's script)
1. **Mount**: the destination is his position plus `delta`. If he is falling, he hangs on
   (step 2); otherwise he goes straight to step 3.
2. **Falling mount**: he keeps falling (steered as usual) and the destination moves sideways
   with him. He holds the first frame of `climb96end`. Once his centre is 82 or more below the
   destination, he is put exactly 82 below and goes to step 3; landing first also goes to
   step 3 (playing `HPSounds.HAR_emotes.landing5`).
3. **Mounting**: his speed and input stop and he turns to face the destination. The rest of the
   way, `mount delta`, is the destination minus his position minus 30 units forward, and then:
   - after a falling mount: minus 82 up; plays `climb96end` and
     `HPSounds.HAR_emotes.pull_up3`;
   - else if it is under 48 up: minus 32 up; plays `climb32` and
     `HPSounds.HAR_emotes.EmotiveHarry5_b_pullup6`;
   - else if under 80 up: minus 64 up; plays `climb64` and
     `HPSounds.HAR_emotes.EmotiveHarry5_a_pullup5`;
   - else: minus 96 up; plays `climb96start`.
   Sounds are at half volume.
4. **Finishing**: each tick he moves by `mount delta` × tick time ÷ the animation's loop length,
   while the animation plays to its last frame. After `climb96start`, `mount delta` is halved
   and `climb96end` follows (with `HPSounds.HAR_emotes.EmotiveHarry5_a_pullup5`), then the
   halved delta again. Then he walks again, playing `breath`.

The climb animations move Harry themselves: their root bone travels 30 forward and 31
(`climb32`), 64 (`climb64`), 12 + 82 (`climb96start` + `climb96end`) or, from hanging, 82 up.
The script's 30 / 32 / 64 / 96 / 82 are taken off because of that.

## How the animations' own movement is applied — also our choice
How the original engine turned the root bone's travel into Harry's movement is native and
lost. Here: while an animation plays, the picture shows it as made (the root bone moves the
body), Harry's place only takes the `mount delta` part, and at the animation's end his place
jumps by the root bone's travel (last key minus first key, turned to his facing), so the
picture doesn't jump. The camera follows smoothly along that travel meanwhile.

## Open points
- The original shrinks Harry's collision to half while finishing and moves him with collision;
  here he moves without collision during the climb and settles onto the floor afterwards.
- Turning to the destination happens at once here; the original turns at his turning speed.

## Uses from the converted data
Harry's `MaxMountHeight`, `MaxStepHeight`, `CollisionRadius`, `CollisionHeight`; the climb
animations and their root bone's travel.

## Changes from the original
- See the two own rules above (when a climb starts, how the animation's movement is applied).
