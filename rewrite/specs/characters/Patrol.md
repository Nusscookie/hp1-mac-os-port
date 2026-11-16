# Patrolling characters

Used in: everywhere (students in the halls, Filch, ghosts, Peeves, gnomes, …)
Placed objects: every HP1 character class (HPBase.baseChar and below) with
`bFollowPatrolPoints`, plus Engine.PatrolPoint and HPBase.TriggerSpwnBsChrOnPPnt.

Sources: the behaviour comes from reading the original scripts (baseChar, PatrolPoint,
TriggerSpwnBsChrOnPPnt, Pawn). Where they rely on the engine underneath (how walking to a spot
ends, touching, spawning), the rules are checked against SurrealEngine's re-implementation of
that engine, so nothing here is guessed.

## Patrol points
When the level starts, each patrol point in turn (in the level's order):
1. finds its next point, if it has none yet: the point tagged `Nextpatrol`, else the one named
   `NextPatrol_ObjectName`, else, for the start of a chain (`bStartOfUnlinkedChain` with a
   `PatrolPointLinkTag`), links the chain: from the current point, the nearest point with the
   same link tag that has no next point yet becomes its next, and so on. (The search includes the
   current point itself, so a chain's last point ends up leading to itself.)
2. tells its next point that it comes before it;
3. if it still has no point before it, looks for one whose next point is itself;
4. works out its curve from what it knows at that moment: with neither neighbour it has no
   curve. With only a next point, the curve points at it and both lengths are the distance to
   it. With only a point before, the curve points away from that one and the out length is the
   distance from it. With both, the curve runs from the point before to the next one and the out
   length is the distance to the next. `bUseOrientationForSplineTan` uses the point's own facing
   as the curve's direction instead. A point's in length, unless set above, is the out length of
   the point before it.

A point can make a visiting character pause (`pausetime`, looping `PauseAnim`, turned to the
point's facing if `bUseLookDir`), send an event (`EventToSend`), or remove the character
(`bDestroyPawn`). `PatrolSound` is not used by HP1's characters.

## What a patrolling character does
1. Starts at the patrol point tagged `firstPatrolPointTag` (or named
   `firstPatrolPointObjectName`); that point also counts as the last one visited, and its link
   tag limits which points the character picks later. It loops `walkAnimName`.
2. Walks to the current point in one of three ways:
   - **Ordinary walk** (default): speeds up towards the point (`AccelRate`, up to
     `GroundSpeed`), facing it. One walk ends when the point is less than 1 unit away sideways,
     or closer than the character travels in about 0.22 s at its current speed, or after
     1 s + 1.3 × (distance ÷ `GroundSpeed`). If the character is then within
     `fNavPointColRadius` (50 for the students) of the point, it has arrived; otherwise it walks
     there again. So characters round the corners of their route, and one that is blocked keeps
     trying.
   - **Straight walk** (`bUseFrayMoveTo`): a step of `GroundSpeed` per second straight at the
     point each tick, until a step goes past it, and then also within `fNavPointColRadius`.
   - **Curve** (`bUseFraySplines`, for points that have a curve): the target moves along the
     curve from the point before into this one: from the point before, pushed out along its
     curve direction by its out length × progress, towards this point, pulled back along its
     direction by its in length × (1 − progress), blended with ease-in/ease-out. Progress grows
     by the distance walked divided by the in length. The character heads for that target at up
     to 1.5 × `GroundSpeed` and arrives when within 1 unit sideways.
   On straight walks and curves it faces the way it actually moved.
3. At the point: sends the point's event, picks the next point (the chain's next, else the
   patrol point nearest to the character other than this one and the last one, among those with
   its link tag if it has one), removes itself if the point says so, and pauses if the point
   says so. After a pause it loops the walk animation again.
4. With no point to go to it loops `idleAnimName` and checks every 0.5 s: with `bLoopPath` it
   starts over from the first point (still in the idle animation, as in the original).

An empty animation name leaves the current animation playing, like the original.

Characters without `bFollowPatrolPoints` use HP1's station paths (HPath_*, baseStation) or just
stand idle; those come later.

## Spawning characters onto a route
`TriggerSpwnBsChrOnPPnt` (a trigger) holds up to eight character classes (`BaseCharToSpawn`),
each with a walking speed (`BaseCharGroundSpeed`, 0 = the class's own). When triggered or
touched (by anything) it picks one at random and spawns it on its start patrol point
(`StartPatrolPoint_Tag`, else `StartPatrolPoint_ObjectName`), facing the next point, patrolling
from there.

As in the original engine, a solid character can't be spawned into another character or a
brush object (HP1's pawn rules refuse any overlap with another pawn); the spawn then simply
fails. This is what keeps a spawner that collides from flooding the level: the new student
touches the spawner, which tries to spawn the next one onto the same spot, and that fails while
the first one is still standing there.

In Tut1 the dispatchers MOMDis1–4 set these off one after another, so a stream of students runs
from the entrance hall up the stairs, each removed at the end of its route. Whether every
student gets through depends on the cutscene around it (the Grand Hall door's swing, Dumbledore
standing in the upstairs corridor), so that is checked once the cutscene player exists.

## Characters bumping into each other
Characters are solid to each other (`bBlockActors`/`bBlockPlayers`) as upright cylinders, so one
walking into another slides round it.

## Uses from the converted data
bFollowPatrolPoints, firstPatrolPointTag, firstPatrolPointObjectName, bUseFraySplines,
bUseFrayMoveTo, bLoopPath, walkAnimName, idleAnimName, AnimSequence, GroundSpeed, AccelRate,
fNavPointColRadius, RotationRate, CollisionRadius/Height; on points: Nextpatrol,
NextPatrol_ObjectName, bStartOfUnlinkedChain, PatrolPointLinkTag, pausetime, PauseAnim,
bUseLookDir, EventToSend, bDestroyPawn, bUseOrientationForSplineTan; on spawners:
BaseCharToSpawn, BaseCharGroundSpeed, StartPatrolPoint_Tag, StartPatrolPoint_ObjectName.

## Changes from the original
- A curve into a point that has no point before it reads a missing point in the original
  (pulling the character towards the map's origin); such points are walked to without the curve.
