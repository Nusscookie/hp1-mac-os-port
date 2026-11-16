# Triggers, dispatchers and music events

Used in: everywhere
Placed objects: Engine.Trigger, Engine.Dispatcher, Engine.MusicEvent, Engine.Counter

Objects talk through names: an object with `Event = X` "triggers" every object whose `Tag` is X.
Triggering can also be undone ("untrigger"), which some receivers use (movers in TriggerControl).

## Trigger
An invisible cylinder (`CollisionRadius`, `CollisionHeight`; HP1 adds `CollideType` 2 = an
upright box using `CollisionRadius` × `CollisionWidth` × `CollisionHeight`, turned with the
object). When something it cares about (`TriggerType`: 0 the player, 1 any pawn, 2 one class
`ClassProximityType`, 3 anything) starts touching it, it triggers its `Event`; when it stops
touching, it untriggers it.
- `ReTriggerDelay`: touches closer together than this are ignored.
- `bTriggerOnceOnly`: works once, then ignores everything.
- `bInitiallyActive` false: does nothing until switched on. `InitialState` OtherTriggerToggles /
  OtherTriggerTurnsOn / OtherTriggerTurnsOff: being triggered itself switches it.

## Dispatcher
When triggered, it goes through its `OutEvents` in order, waiting `OutDelays[i]` before sending
each one (so the delays add up). It ignores new triggers while it's busy.

## Counter
Counts triggers; on the `NumToCount`-th it triggers its `Event`.

## MusicEvent
When triggered it switches the music to `Song` (`bSilence` stops the music instead).
`bOnceOnly` makes it work once. HP1 adds a temporary volume change (`bDoBoost`,
`PercentMusicVolume`, `BoostTime`) and a message when the song ends (`TriggerToSendWhenDone`);
those come later.

## Changes from the original
- None yet.
