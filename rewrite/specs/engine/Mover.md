# Mover

Used in: everywhere
Placed objects: Engine.Mover (doors, lifts, secret walls, gates)

## What it does
A mover is a piece of level geometry (its own brush) that slides and turns between up to 8 key
positions. Key 0 is where it rests ("closed"); opening travels key 0 → 1 → … → last, closing
travels back. Each leg takes `MoveTime` seconds. Positions are `BasePos + KeyPos[n]`, rotations
`BaseRot + KeyRot[n]`. With `MoverGlideType` 1 the leg starts and ends smoothly
(smoothstep of the time); 0 moves at constant speed.

While it moves it blocks and carries actors like the level geometry does: a character standing on
it moves along with it.

## States (`InitialState`)
- **TriggerToggle**: each trigger flips it: opens if closed or closing, closes if open or opening.
- **TriggerOpenTimed**: a trigger opens it; after `StayOpenTime` it closes by itself. Ignores
  triggers while busy. With `bTriggerOnceOnly` it stays open for good.
- **TriggerControl**: opens while triggered, closes when untriggered.
- **TriggerPound**: keeps opening and closing while triggered.
- **BumpOpenTimed**: a pawn walking into it opens it; closes after `StayOpenTime`.
- **StandOpenTimed**: a pawn standing on it opens it (lifts); closes after `StayOpenTime`.
- `DelayTime` waits before opening in the triggered states.

## Reacts to
- Trigger / untrigger by `Tag`, bump, stand-on as above.
- When fully open it triggers its `Event`; when it starts closing it untriggers it.
- Something in the way (`MoverEncroachType`): 0 stop, 1 go back the other way, 2 and 3 keep
  moving (the character is pushed out).

## Sounds
`OpeningSound` when it starts opening, `OpenedSound` when open, `ClosingSound` when it starts
closing, `ClosedSound` when closed, `MoveAmbientSound` looped while moving.

## Uses from the converted data
KeyPos, KeyRot, BasePos, BaseRot, NumKeys, KeyNum, MoveTime, StayOpenTime, DelayTime,
MoverGlideType, MoverEncroachType, InitialState, bTriggerOnceOnly, Tag, Event, the sounds, the
brush and PrePivot.

## Changes from the original
- None yet.
