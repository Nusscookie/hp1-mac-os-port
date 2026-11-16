# Path finding

Used in: everywhere
Placed objects: Engine.NavigationPoint and its subclasses (PathNode, PatrolPoint, PlayerStart, …)

## What it does
The level designer placed navigation points; the level build stored the walkable links between
them ("reach specs"): from point, to point, length, the largest character that fits
(`collision_radius`, `collision_height`) and how the link is travelled (walk, fly, swim, jump,
through a door, special). A character that wants to get somewhere:
1. picks the nearest navigation point it can reach from where it stands, and the nearest one to
   the goal;
2. finds the shortest chain of links between them that its size and way of moving allow
   (A* on link length);
3. walks to each point in turn, then to the goal itself.

Links marked pruned are left out (they duplicate a longer chain).

HP1's maps store every link with the same length (500), size (60 × 60) and kind ("special"), so
the length used is the distance between the two points, and special links count as walkable.
HP1's level tool only joined nearby points of the same kind: cutscene marks with cutscene marks,
camera spots with camera spots, patrol points with patrol points, and HP1's own path points
(HPath_*, navShortcut, baseStation) with each other. So there is no single walking network: most
characters follow their patrol chains (`NextPatrol`) and cutscene marks, and the path finder only
helps within one group. When no route exists the character walks straight at its goal and slides
along whatever is in the way.

## Uses from the converted data
The level's reach specs, navigation point locations, the character's collision size.

## Changes from the original
- None.
