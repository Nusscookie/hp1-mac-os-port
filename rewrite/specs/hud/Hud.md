# HUD: cutscene borders and spoken lines

Used in: everywhere
Sources: HP1's HUD classes (baseHud, HPHud), the console's font choice (HPConsole), and, for how
text is measured and wrapped, SurrealEngine's canvas code.

## Each frame
The HUD draws, in this order: the cutscene borders, the current spoken line, the counters (the
bean counter so far), then health, the countdown and enemy health, which are only drawn outside
cutscene mode (health so far; see `specs/props/Pickups.md`). Nothing is drawn while the menu
book is open. Everything is drawn white-tinted in `MedInkFont` unless said otherwise.

## Cutscene borders
Two black bars, at the top and the bottom, both `border` pixels high. They are filled with the
first texel of `HPBase.Icons.leftPanel`.

Each drawn frame, while cutscene mode is on **or** a spoken line is showing, `border` moves a
fifth of the way towards one eighth of the screen height. Otherwise it shrinks by a fifth of
itself. So the bars slide in and out. As in the original, this happens per drawn frame, so it
is quicker at a higher frame rate.

## Spoken lines
A cutscene line (see `specs/cutscenes/CutScene.md`) hands the HUD its text and how long it lasts.
A new line replaces the one showing. The time counts down every game tick, and the line is gone
once it is below zero. The `ToggleDialog` setting (on by default) can hide the text; the bars
still come in.

Drawing a line:
- Font: for English, international and Polish text, the icon-message font is `MedInkFont` and the
  small one `SmallInkFont`. For the other Western languages they are `SmallInkFont` and
  `TinyInkFont`. (Asian and Thai text used fonts drawn by Windows; not done yet.)
- Colour: light blue (127, 127, 255); the font's background is see-through.
- Width `w` is the text's width in the icon-message font, unwrapped. If `w` is less than the
  screen width minus 16, the text starts at `(screen width − w) ÷ 2` (rounded down), else at 0.
- The number of lines it needs is `(w + 90) ÷ screen width + 1` and the number that fit is
  `(screen height ÷ 8) ÷ letter height`, both rounded down. If it needs more than fit, the small
  font is used instead (keeping the start worked out with the big one).
- It is drawn at `screen height − border + 1`, so it slides with the bottom bar.
- Wrapping: the text is split into words, runs of spaces, and line breaks. Words go on a line
  until the next one would make the line wider than the screen; then a new line starts at the
  left edge. A run of spaces at the start of a wrapped line is dropped. Line height is the
  tallest letter on it.

Letters: a character's rectangle is found by walking the font's pages (character number minus
the earlier pages' counts). A character with no width there uses its capital letter, and failing
that a space. Letters sit side by side with no extra spacing.

## Countdown and lesson popups
The countdown bar and the spell lesson's grade and final-score popups are described with the
lesson (`specs/spells/SpellLesson.md`, Popups and HUD). The countdown is drawn with the health
bar, outside cutscene mode; popups are drawn last, over everything, also in cutscene mode.

## Screen size
The original drew in screen pixels. Here the HUD works in the window's size in points, so a
high-density (Retina) screen shows it at the same size as an ordinary one of that size.

## Uses from the converted data
Fonts (`fonts/HPBase.json`) and their page textures, `HPBase.Icons.leftPanel`, the language
(`default.ini` `[Engine.Engine] Language`).

## Changes from the original
- Screen size in points instead of pixels (see above).
