# Cutscenes

Used in: every level with scripted scenes (Lev_Tut1 has 16)
Placed objects: HPBase.CutScene, with HPBase.CutMark / HPBase.CutCameraPos as places, and the
characters, Harry and the camera (HarryPotter.PotCam) as its cast.

Sources: read from the original cutscene, character, Harry, camera, narrator and HUD scripts.
Where those lean on the engine underneath (state changes, turning, smooth moves, animation
ends), the rules were checked against SurrealEngine. Nothing here is guessed; open points are
listed at the end.

## The scene object
A cutscene has up to seven cast members (slots 0–6; the map can name up to twelve, but only the
first seven are ever run). Each member is an actor in the level plus an alias, and has its own
script of up to 40 lines. The scene also has a list of up to 40 places (an actor plus an alias),
usually cut marks and camera positions, but any actor can be a place.

### Starting
- `bLevelLoadStarts`: starts as soon as the level is running.
- `bTriggerStarts` (on by default): starts when triggered.
- `bTouchStarts` (on by default): starts when Harry touches it (only Harry; others are ignored).
- It starts only if it isn't already playing and may still play. `bPlayOnce` (on by default)
  makes the first start the only one.
- `bDelayLevelFadeIn` stops a level fade-in that is under way. No script starts one at level
  load, so on its own it changes nothing; scenes that use it fade in with their own `FadeIn`.

### Running
Every tick, while playing, the members run in slot order. For each member:
1. A member without an actor is finished at once.
2. A member waiting for a cue checks whether that cue has been given at any time so far in this
   scene (letter case doesn't matter); if so it stops waiting.
3. A waiting or finished member does nothing more this tick.
4. Its delay counts down by the tick's time. While some delay is left it does nothing more.
5. Otherwise it runs its next script line, then moves on to the next line. After line 40 it is
   finished.

So a member runs at most one line per tick, empty lines included (they take a tick each), and
only `Sleep`, `Say`, `Talk` and `Emote` set a delay. The delay is set, not added, so time a
delay overshot by is dropped.

When every member has finished, the scene stops. A scene that may play again is then reset:
all members back to their first line, no delays, no waiting, no cues remembered.

### Cues
A cue is a word one member gives and others wait for. Cues are remembered for the whole scene
(in capitals), so a member that starts waiting for a cue already given goes straight on. Only
the first 60 cues of a scene are remembered; later ones only release members already waiting
with exactly the same spelling.

Moving and turning give their own cue when done: the line number and the actor's name joined
together. The member that started the move waits for it.

### Script lines
A line is read in capitals and split at the first space into the command and the rest (the
argument). An empty line or `NONE` does nothing, which is how lines are commented out
(`NONE Moveto X`). An unknown command does nothing either.

Targets (actors named by the argument) are looked up in this order: `HARRY` is the player;
then the cast aliases (slots 0–6); then the aliases of the places that have an actor; then
`CASTn` for slots 0–5; then `LOCn` for places 0–39. Nothing found is an error and the target is
missing.

| Command | What it does |
|---|---|
| `GOTO x`, `TELEPORT x` | Puts the actor at x. A camera with a *preface* target set also turns at once to look from x at that target, keeps looking at it, and the preface is used up; a camera also keeps x as its position spot. |
| `MOVETO x` | Characters and Harry walk to x; the camera flies to x (and takes the preface target as its look target if one is set, using it up). The member waits until the move is done. A missing target finishes at once. Other actors never finish, so the member waits forever. |
| `TURNTO x` | Characters and Harry turn towards x and the member waits until they face it. The camera just takes x as its look target and the member goes on. |
| `FACE x` | Turns the actor to face x at once (level for characters and Harry; the camera also tilts and keeps looking at x). |
| `PREFACE x` | Sets the scene's preface target, used by the next camera `GOTO` or `MOVETO`. |
| `ANIMATE a` | Characters and Harry play animation a once (see below). |
| `CAPTURE` / `RELEASE` | Takes over / gives back the actor (see below). |
| `SLEEP s` | Waits s seconds. |
| `CUE c` | Gives cue c. |
| `WAITFOR c` | Waits for cue c. |
| `SAY id` | Plays dialogue line id (see Dialogue) and waits as long as it lasts. |
| `TALK id`, `TALK0`–`TALK6` | Like `SAY`, and the speaker plays its talk animations. The speaker is the member itself, or cast slot n when the character right after `TALK` is a digit n. |
| `EMOTE line`, `EMOTE0`–`EMOTE6` | A character's emote line (speaker chosen like `TALK`): plays the dialogue in `<…>`, fires the event in `[…]`, and waits that long plus 0.75 s (2.75 s without dialogue). Only characters can emote. |
| `TEXT t` | Shows t (in capitals) as a message for 4 s. |
| `TRIGGER e` | Triggers every object tagged e. |
| `CHANGELEVEL m` | Goes to level m. |
| `SETIDLE a` / `SETWALK a` | Sets a character's cutscene idle / walk animation. |
| `CAMSPEED f [r]` | Camera only: flying speed = f × normal, and if r is given, turning speed = r × normal. |
| `CAMPROX d` | Camera only: a camera move counts as done within d units (normally 15). |
| `CAMRESTORE` | Camera only: back to its saved settings and its saved place behind Harry, leaving cutscene mode (see Camera). |
| `FADEIN s` / `FADEOUT s` | The screen goes black at once and brightens over s seconds / darkens to black over s seconds. |
| `SOUND …` | Does nothing. |

## Characters in a cutscene (HP1 characters)
- **Capture** only remembers what the character was doing; it carries on with it until a
  cutscene command changes that.
- **Release** puts it back to what it was doing at capture, started over from its beginning
  (a patrolling character walks to its current point again, playing its walk animation; a
  pausing one pauses again in full).
- **Walk to x**: loops its cutscene walk animation and heads straight for x at `GroundSpeed`,
  turning towards it at its turning speed. Each tick it lifts by 15, moves (with a nudge of 0.1
  upwards), and goes back down by 15, sliding along anything it hits. Height differences don't
  count: the walk is done when the step would reach x. If it takes longer than (distance ÷
  `GroundSpeed`) + 1 s, it is put right onto x and dropped onto the floor below (up to 100). The
  cue comes at the next tenth of a second after that; then it loops its cutscene idle animation.
- **Turn to x**: turns towards x at its turning speed and is done when facing it within about
  11° (2000 rotation units); then it loops its idle animation.
- **Animate a**: plays a once, then loops its idle animation.
- **Talk**: plays `CutTalkStartAnim` once, loops `CutTalkAnim` until the line's time is up
  (checked every 0.2 s), plays `CutTalkEndAnim` once, then loops its idle animation.
- The cutscene idle and walk animations are `CutIdleAnim` / `CutWalkAnim`, else the character's
  `idleAnimName` / `walkAnimName`, else `Breath` / `Walk`.

## Harry in a cutscene
- **Capture** starts cutscene mode (the HUD's black bars) and makes Harry stand still: no input,
  dropped onto the floor below (up to 100), looping `breath`.
- **Release** ends cutscene mode and gives him back to the player.
- **Walk to x** works like a character's, looping `run`; afterwards he loops `breath` and
  stands still as when captured.
- **Turn to x** like a character's; afterwards he stays still and keeps his animation.
- **Animate a** plays a once, then goes back to what he was doing before (standing still, or
  the player's control if he wasn't captured).
- Talking and emoting don't animate Harry.

The player is the Harry placed in the level (the original reuses it rather than spawning a new
one), so cutscenes can name him in their cast.

## Animations
Playing an animation the character's mesh doesn't have (or an empty name) changes nothing.
Waiting for the end of one then waits until the animation that is playing anyway reaches the
end of its current loop, and that one then stops. A one-shot animation of n frames at rate r
ends after (n − 1) ÷ r seconds; one of a single frame after 0.1 s.

## Camera
The camera is the one the player looks through. Outside cutscenes it is the follow camera
(`specs/engine/Camera.md`).
- **Capture** saves its settings (speeds, look and position targets, where it is relative to
  Harry, its facing) on a stack and switches to cutscene mode. If it already has a look target
  it aims at it; if it has a position spot it jumps there.
- In cutscene mode, each tick it flies towards its position spot, covering the fraction
  speed × tick time of the way left (at most all of it; the normal speed is 2), through walls.
  Within `CAMPROX` of the spot the move counts as done. It always aims at its look target and
  turns towards that at its turning speed (normally 20000 units per second, for yaw and pitch
  alike).
- **Release** sets `CAMPROX` back to 15, takes the saved settings off the stack and leaves
  cutscene mode where it is; the follow camera carries on from there.
- **CAMRESTORE** restores the saved settings without taking them off the stack, puts the camera
  back at its saved place relative to Harry (turned by how much Harry's facing changed) with its
  saved facing, and leaves cutscene mode.

## Dialogue
A dialogue line has an id. Its text is the id's entry in the dialogue text of the chosen
language (`HPdialog`, section `all`); its voice is the sound `AllDialog.<id>`.
- With a voice, the line lasts the sound's length + 0.5 s, and the voice plays loud from Harry
  (volume 3.2, radius 20000).
- Without one it lasts 3 s and the text gets a `*` in front.
- The text shows as a message for the line's time, except for ids containing `EMOTIV`.

## Uses from the converted data
On scenes: bPlayOnce, bTouchStarts, bTriggerStarts, bLevelLoadStarts, bDelayLevelFadeIn,
Cast[i].actor/alias, Locs[i].actor/alias, Cast0Script…Cast6Script. On characters:
CutIdleAnim, CutWalkAnim, CutTalkAnim, CutTalkStartAnim, CutTalkEndAnim, idleAnimName,
walkAnimName, GroundSpeed, RotationRate. The dialogue text (`hpdialog.*`) and voices
(`AllDialog`). Animation lengths from the meshes' sequences.

## Open points and changes
- Turning: the original also tilts a character towards a target above or below it (turning by
  `RotationRate.Pitch`) when not walking. Here characters only turn level. (Dumbledore in Tut1
  has a pitch rate of 0, so he doesn't tilt in the original either.)
- Release of a character captured while it was in a cutscene state other than standing idle
  (e.g. captured twice, mid-walk) puts it back to standing idle.
- Emotes: the emote animation (`(…)` in the line, from the character's `Emotes` list) isn't
  played yet; the dialogue, event and waiting are.
- What happens within one tick follows our objects' order (the level's order), as in the
  original; the exact sub-tick timing of the original's latent waits isn't knowable beyond that.
- Fixed on purpose: a camera GOTO/TELEPORT to a name that can't be found is skipped. The
  original sends the camera to the world origin then.
- Voices: a new line stops the previous voice. The original plays each line on Harry's interact sound
  slot, which has the same effect.
- Tut1's intro (CutScene4) plays through to the end in the viewer: 5 lines, the music change,
  Dumbledore at his end mark and Harry free to walk.
