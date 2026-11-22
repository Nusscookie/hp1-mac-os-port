# Spell lesson (learning a spell by tracing it)

Used in: Lev_Tut1 (Flipendo with Quirrell), later every lesson (Lumos, Wingardium Leviosa, …)
Placed objects: `HPBase.SpellLearnTrigger` (Tut1: `SpellLearnTrigger0`, started by a cutscene)
Sources: the lesson trigger's script, its score popups, the HUD's countdown, Harry's
`SpellLearning` state, and SurrealEngine's gesture scoring (`CompareGesture`,
`CompareGesturePoint`; the original native code is lost).

## What it does
When triggered, the lesson takes over Harry and the camera. The teacher shows the spell's
pattern as a line of sparkles in front of Harry. Harry then has a limited time to trace it with
the wand. The tracing is scored; passing four rounds with rising pass marks earns house points.

## Patterns
A spell's pattern (`Gesture` on the spell class, e.g. `HPBase.FlipPattern`) is a list of points
in the unit square (x to the right, y downwards), read as one line through them in order. A
drawn point's **distance** to the pattern is its distance to the nearest point of that line.

**Score of a drawing** (SurrealEngine's rule):
- Unused slots are left out. With no points the score is 0.
- *Precision*: the share of drawn points within `fAccuracy` of the pattern.
- *Coverage*: 100 evenly spaced spots along the pattern (in the middle of each hundredth of its
  length); the share of them with a drawn point within `fAccuracy`.
- The score is coverage × precision, and 0.97 or more counts as a full 1.

## Places
Everything is set out from the trigger's place and facing (its yaw):
- the **look point**, `TemplateDist` ahead of the trigger;
- the **camera**, 20 ahead of the trigger, looking at the look point;
- the **template** and the effects, `TemplateDist` ahead of the camera;
- the **blackboard** (a picture, `SpellBlackboard`), `TemplateDist` + 50 ahead of the trigger;
- the **wand** for wand position (wx, wy), `WandDist` × (1 ahead, wx × `FOVRatio` right,
  wy × `FOVRatio` up) from the camera. A pattern point (x, y) is wand position
  (x − 0.5, 0.5 − y).

`TemplateDist` is raised to at least `WandDist` + 5 once Harry has turned.

## States
- **Start** (on trigger): Harry stops and plays `breathe`. Input no longer moves him. The
  lesson selects the spell. The camera holds still as above (its place and settings saved). The
  blackboard appears. The teacher (`Teacher`) stops what it was doing. Round 0 begins with
  *Settle*.
- **Settle**: Harry is moved onto the trigger's place, 2 seconds pass, then Harry faces the look
  point and the teacher faces the camera.
- **Show** (each round): the round's pass mark is `PassMark[round]`; no grade shows yet. The
  drawing is cleared (500 empty slots), cutscene mode is on, and the teacher loops `talk`. The
  teacher says one line, picked at random from the filled entries:
  - in round 0 the first time, `LessonIntro`;
  - in round 0 after a failure, `LessonRepeatFailure`;
  - otherwise `LessonRepeatSuccess`.

  A line lasts its voice's length + 1 second (5 seconds without a voice) and shows as a
  subtitle for that long. The teacher plays its spell animation once (Flipendo: `flipendo`;
  Incendio: `Incendio`; Wingardium Leviosa: `Wingardium`; Lumos: `Lumos`). The template starts
  drawing itself. When the line is over, the teacher loops `breathe`, cutscene mode goes off,
  and the grade popup shows. Then comes *Trace*.
- **Trace**: the wand (`WandClass`, a mesh held at a fixed tilt) appears at the pattern's
  first point, with a wand glow (`WandFXClass[0]`) and a trail (`DrawFXClass[0]`, 5 wide)
  following it. The HUD countdown starts at `DrawTime`. Every tick:
  - the mouse and the move keys move the wand position, each axis kept within −1..1;
  - while the *draw button* (the original's AltFire, the right mouse button) is held and the
    time is not up: the first time, the begin sound plays here and the trace sound plays
    from the wand. The trail emits. The wand position is stored as a pattern point in slot
    500 × time ÷ `DrawTime`. Time counts from the start of *Trace*, not from the first press;
  - letting go after having started, or the time running out, goes to *Judge*;
  - otherwise the trail stops emitting.
- **Judge**: the countdown stops and the end sound plays. The drawing is scored and shown as
  the grade. The wand goes back to the pattern's start. The trail is replaced by a new one,
  plus a red "bad point" one. Then the drawing replays at double speed: the wand moves through
  the stored points in order. Each point off the pattern by more than `fAccuracy` switches to
  the bad trail (with the error sound at pitch 0.7). Each point on the pattern switches back
  (with the trace sound). Meanwhile the grade on screen counts up from 0 to the score. If Harry
  never pressed, there's no replay. Once the replay is over, 1 more second passes and the wand
  and its effects go. Then:
  - **Pass** (score ≥ the round's pass mark): the incantation (the spell's `SpellIncantation`
    line, voice only) plays, with the correct-trace sound. Two effects spread over the pattern:
    the spell's hit effect (12 wide, living 3 seconds) and `WinFXClass[2]`. The teacher loops
    `talk_pos`; after 3 seconds it loops `breathe`. The popup goes, cutscene mode comes on, and
    the teacher praises: `LessonFullMarks` for a full score (its voice + 1 s, else 3 s), a random
    `LessonJudgeVGood` above `fVGoodThreshold`, else a random `LessonJudgeSuccess`. After the line
    the round goes up by one and `iNumHousePoints[round − 1]` house points are won:
    - after round 4 (the last): the template goes, then the `LessonPoints[3]` line. Cutscene mode
      goes off and the final popup shows for 5 seconds; then *End*;
    - otherwise the teacher loops `breathe`, cutscene mode goes off, the template goes, the
      `LessonPoints[round − 1]` line plays (cutscene mode on during it), and the next *Show*
      begins.
  - **Fail**: the wrong-trace sound and the fail effect over the pattern. The teacher loops
    `talk_neg` for 3 seconds. The popup goes, cutscene mode comes on, and a random
    `LessonJudgeFailure` line plays (score above `fVBadThreshold`), else `LessonJudgeVBad`. Then
    the teacher loops `breathe`, cutscene mode goes off, and the wand and template go. In round 0
    it's *Show* again; otherwise the final popup shows for 5 seconds; then *End*.
- **End**: the camera goes back to how it was, Harry walks again, everything the lesson made
  goes, the teacher loops `breathe`, and the objects tagged with the trigger's `Event` are
  triggered (Tut1: `CUTFLIPBEGIN`).

## Template drawing
The template is a sparkle system (`HPParticle.SilverSparkle01` with a practically endless
life and 3 × `fAccuracy` ÷ 0.01 wide), its particles spread along the pattern. Over a quarter
of `DrawTime` it advances its place along the pattern from 0 to 1. Each tick it emits along the
stretch it covered in that tick. It stays until the round's end.

## Popups and HUD
- **Grade popup**: in the big font, yellow, centred: "Grade: N%" 80 pixels above the bottom
  and "Pass Mark: M%" 48 above (texts `lesson_text_02` / `lesson_text_01` of `Pickup`). N is the
  score × 100 rounded down; while counting up, N × the part of the replay done. With no grade
  yet only the pass mark shows, at the left (x 32, 48 above the bottom). It is drawn in
  cutscene mode too.
- **Final popup**: centred, yellow, big font: "Lesson Score R/4" (`lesson_text_03`, R = rounds
  passed) 40 above the middle, and below it the house points text for 5, 15, 30 or 50 points
  (`lesson_text_04`–`07`).
- **Countdown**: while running, the empty timer bar (`HPBase.Icons.TimerBarEmpty`) 8 pixels from
  the right edge and 160 above the bottom, with the full bar over it showing 97 pixels plus the
  rest of its width × time left. The time ticks down each tick. Each time it has moved more than a
  second from the last tick sound, a tick sound plays at half volume: `timer_1` above 30 s
  left, `timer_2` above 20, `timer_3` above 15, `timer_4` above 10, `timer_5` above 5, else
  `timer_6` (`HPSounds.menu_sfx`). The first tick sounds right away.
- The big font is `HPBase.HugeInkFont` for English, the international text and Polish, and
  `HPBase.BigInkFont` otherwise.

## Wand movement
In the original, the mouse moves the wand by 30 ÷ 64000 per input unit per second. The forward
and side move keys move it by 20 ÷ 64000 times their axis value (the default keys give forward
300, back 150, sideways 150). Up on the mouse and forward move the wand up.

## Reacts to
- Trigger: starts the lesson (once; `bTriggerOnceOnly`).

## Open points
- **Our rule:** how far a pixel of mouse movement moves the wand. The original's mouse scaling
  is native. Here one pixel moves the wand 1/2000 of the pattern square, so that tracing feels
  like turning Harry. The keys use the original numbers.
- **Our rule:** how the template, trail and effects spread their particles (see
  `specs/engine/Particles.md`, Patterns). The original's native code is lost and SurrealEngine
  has none.
- The trigger's own voice lines play like cutscene voices (heard everywhere) here; the original
  plays them from the trigger, which stands where Harry is.
- `SpellLevel` (the template's look) is always 0 in the scripts, so `DrawTime` never shrinks.
- The original moves Harry with collision onto the trigger's place; here he is put there.
- The error sound plays at normal pitch (the original uses 0.7); sound pitch is not supported yet.
- The original's popups also close when the player skips (`clearMessages`); not done here.
- The blackboard's texture is an ice texture (a moving glass layer over a picture). Here it shows
  its picture in its own palette, as SurrealEngine does; the glass isn't drawn.
- If a cutscene lets go of Harry or the teacher after starting the lesson, the lesson takes them
  back (the Tut1 cutscene releases them around the time it triggers the lesson).

## Uses from the converted data
The trigger's `Spell`, `Teacher`, `Event`, `DrawTime`, `TemplateDist`, `WandDist`, `FOVRatio`,
`fAccuracy`, `PassMark`, `iNumHousePoints`, `fVGoodThreshold`, `fVBadThreshold`, the `Lesson*`
line names, `WandClass`, `WandFXClass`, `DrawFXClass`, `WinFXClass`; the spell class's
`Gesture`, `SpellIncantation` and `hitParticleEffectClass`; the gesture's points (converted to
`gestures.json`); the popup texts in `Pickup`.

## Changes from the original
- None.
