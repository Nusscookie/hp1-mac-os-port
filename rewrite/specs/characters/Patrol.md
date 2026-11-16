# Patrolling characters

Used in: everywhere (students in the halls, Filch, ghosts, Peeves, gnomes, …)
Placed objects: every HP1 character class (HPBase.baseChar and below) with
`bFollowPatrolPoints`, plus Engine.PatrolPoint.

## Patrol points
Patrol points form chains. Each point knows the next one:
- by tag (`Nextpatrol`) or by object name (`NextPatrol_ObjectName`), or
- for a chain marked `bStartOfUnlinkedChain` with a `PatrolPointLinkTag`: starting there, each
  point is joined to the nearest not-yet-joined point with the same link tag, until none is left.

Every point also learns which point leads to it (its previous one). From the previous and next
point each point gets a direction for a smooth curve through it: along the line from the
previous point to the next one (or towards its only neighbour at a chain end, or its own facing
with `bUseOrientationForSplineTan`). The "out" length of a point is the distance to its next
point; the "in" length is the previous point's out length.

A point can make a visiting character pause (`pausetime`, playing `PauseAnim`, turned to the
point's own facing if `bUseLookDir`), send an event (`EventToSend`), play `PatrolSound`, or
remove the character (`bDestroyPawn`).

## What a patrolling character does
1. Starts at the patrol point tagged `firstPatrolPointTag` (or named
   `firstPatrolPointObjectName`) and plays its walk animation (`walkAnimName`, else `Walk`).
2. Walks to the current target point at `GroundSpeed`:
   - with `bUseFraySplines`, along the smooth curve: a point on the curve is found between the
     previous point pushed out along its direction and the target pulled back along its
     direction, blended with ease-in/ease-out; progress along the leg grows by distance walked
     divided by the target's in length. It never moves faster than 1.5 × `GroundSpeed`.
   - otherwise straight at it, flat on the ground, until it has passed the point.
   It turns to face the way it actually moved.
3. At the point: pause / event / sound / removal as set on the point, then the next point of
   the chain. At a chain end without a next point it picks the nearest patrol point other than
   the one it is at and the one it came from.
4. With no point to go to it stands still playing `idleAnimName` (else `Breath`); with
   `bLoopPath` it starts the patrol over from the first point.

Characters without `bFollowPatrolPoints` use HP1's station paths (HPath_*, baseStation) or just
stand idle; those come later.

## Spawning characters onto a route
`TriggerSpwnBsChrOnPPnt` (a trigger) holds up to eight character classes (`BaseCharToSpawn`),
each with a walking speed (`BaseCharGroundSpeed`, 0 = the class's own). When triggered, or
touched, it picks one of them at random, puts it on its start patrol point
(`StartPatrolPoint_Tag`, else `StartPatrolPoint_ObjectName`) facing the next point, and sends it
patrolling from there. In Tut1 the dispatchers MOMDis1–4 set these off one after another, so a
stream of students runs from the entrance hall up the stairs, each removed at the end of its
route.

## Uses from the converted data
bFollowPatrolPoints, firstPatrolPointTag, firstPatrolPointObjectName, bUseFraySplines,
bLoopPath, walkAnimName, idleAnimName, GroundSpeed, CollisionRadius/Height; on points:
Nextpatrol, NextPatrol_ObjectName, bStartOfUnlinkedChain, PatrolPointLinkTag, pausetime,
PauseAnim, bUseLookDir, EventToSend, PatrolSound, bDestroyPawn, bUseOrientationForSplineTan.

## Changes from the original
- The v0.1.1 port showed characters overshooting patrol points; here arriving is decided by
  passing the point (straight walks) or reaching the end of the curve (splines), so they
  stop at each point.
- A character that cannot reach a point for much longer than the walk should take (a closed
  door, another character in the way) counts it as reached and carries on, instead of
  running on the spot forever.
- The spawn trigger reacts to touches only from what its TriggerType allows (the player by
  default). Reacting to any touch would let each new character, standing on the trigger,
  spawn the next one.
