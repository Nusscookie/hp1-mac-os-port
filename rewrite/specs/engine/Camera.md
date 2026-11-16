# Follow camera (standard mode)

Used in: everywhere Harry walks
Placed objects: none (the camera is created with the player). HP1 class: BaseCam / PotCam.

## What it does
The camera looks at an *aim point* 75 units in front of Harry and 50 up (turned with Harry).
Its goal position lies on the line from the aim point through a spot `CameraHeight` (80) above
Harry's eyes, `CameraDistance` (150) away from Harry, so it hangs behind and above him and
looks slightly down past him. It glides towards the goal instead of jumping, so quick turns
swing it round smoothly.

While Harry stands still, moving the mouse up and down tilts the camera (up to about ±66°); as
soon as he moves the tilt eases back to level.

The camera never ends up inside walls: the way from Harry's head to the goal is swept with a
small box and the camera stops where it would hit.

## Other modes (later)
HP1 has many camera modes switched by scripts and cutscenes: far, high, reverse, boss, troll
chase, cutscene, patrol, top-down, Quidditch, free. They get their own sections when the levels
that use them are done.

## Uses from the converted data
Harry's eye height. The distances above are fixed in the original camera.

## Changes from the original
- None yet.
