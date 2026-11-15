# Behaviour specs

Every character, object or system the Rust game implements gets a short description here first, written
in our own words. The Rust code is written from that description.

## The rule

- You may read the original scripts (in the git-ignored `reference/` folder, dumped from your own CD) and
  play the game to understand **what** something does.
- Write down the behaviour as a description: what it reacts to, which states it goes through, what it
  plays, what it changes in the world.
- Never copy or translate script text line by line, and never paste it here or into the code.
- Tuning values (speeds, timings, damage, sounds, animation names) are read from the CD by the converter,
  so they don't need to be written here. Our own changes go in `tuning/`.
- If we fix a bug from the original release, say so under "Changes from the original".

## Template

Save as `specs/<area>/<Name>.md`, for example `specs/characters/Gnome.md`.

```markdown
# <Name>

Used in: <maps or "everywhere">
Placed objects: <class names in the maps that use this behaviour>

## What it does
<one paragraph>

## States
- **<State>**: <what happens, what moves it to the next state>

## Reacts to
- <touch, trigger, spell, damage, timer, …>: <what it does>

## Uses from the converted data
- <property names it reads, e.g. GroundSpeed, CutWalkAnim>

## Changes from the original
- <bug fixes or none>
```
