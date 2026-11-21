# Pickups: Every Flavour Beans and Chocolate Frogs

Used in: everywhere (Tut1 has 29 beans and one frog)
Placed objects: HProps.Jellybean and its colours (GreenJellyBean, BlueJellyBean, …),
HarryPotter.ChocolateFrog
Sources: the original prop scripts (Jellybean, ChocolateFrog, baseProps), Harry's counters
(baseHarry), the HUD items (baseHudItem, BeansHudItem, baseHud's health), and PlayerPawn's
`CameraToWorld` for where collected items fly.

## Beans
A placed bean spins on the spot, a full turn about every 2.2 s (30000 rotation units a second).
When Harry comes within 60 units of its centre, or touches it, it is collected:
1. It plays `HPSounds.magic_sfx.pickup11`, stops colliding with the world, and Harry's bean
   count goes up by one; the HUD's bean counter shows (see below).
2. For 0.25 s it flies towards a point in front of the player's view: 150 units ahead, and
   to the right and up by three quarters of the half-width the view shows at that distance
   (with the 90° view, 112.5 units each way): towards the bean counter in the top right. Each
   tick it covers the remaining distance divided by the number of ticks left. (The original's
   comment says the second number counts from the top, but its maths puts +0.75 above the
   centre, where the counter is.)
3. Then it is gone. The original checks every 0.1 s, so it is gone at the first check after
   the 0.25 s (at 0.3 s); in the ticks after 0.25 s it stays on the point.

Beans dropped by others (gnomes, chests) bounce on the floor; that comes with those objects.

## Chocolate Frogs
A frog sits in place playing, over and over: `CROAK` once, a ribbit
(`HPSounds.critters_sfx.frog_ribbit`), `hop` once, `BREATH` looping at 0.6 speed for a random
0–3 s and then on to the last frame of the loop it is in (at once if it is already past that
frame), another ribbit and `hop`. "Once" means from the first frame to the last, as the engine's
finish-animation rule has it (SurrealEngine). When Harry touches it, it plays its `pickup` sound,
gives Harry 10 health (up to his maximum) and flies like a bean, but to the top left (the health
bar).
Each frog puts a glow (`HPParticle.Jellyglow`, see `specs/engine/Particles.md`) 96 units above
itself when the level starts. The glow stays after the frog is collected.

## Harry's counters
Beans start at 0. Health is `lifePotions` out of `MaxLifePotions` (50 and 50 for Harry).

## HUD: health and the bean counter
Drawn each frame after the cutscene bars and the spoken line (`specs/hud/Hud.md`):
- **Bean counter** (always, also in cutscenes): it slides down from above the screen while
  shown and back up when its time is over. Showing it lasts 5 s. Each game tick while shown, its
  top moves a fifth of the way to 4 pixels; afterwards a fifth of the way to −128. At −128 or
  above the screen it is not drawn. It sits 160 pixels in from the right edge. It draws
  `HPMenu.Icons.beancounter`, the count in black in the HUD font (at the icon's centre, moved
  5 left for numbers below 10, else 10 left, and 5 down), then one bean pile picture per three
  beans above the first three (`beans1` … `beans4`, up to 15 beans), each at the counter's
  corner.
- **Health** (only outside cutscene mode): `HarryBarfull` at the top left, with the top part of
  `HarryBarEmpty` over it: `(height − 50) × (1 − health) + 25` pixels of it.

Textures that are masked keep their see-through parts.

## Uses from the converted data
On beans and frogs: their mesh and animations, the frog's `pickup` sound. On Harry's class:
`lifePotions`, `MaxLifePotions`. HUD textures `HPMenu.Icons.*` and `HPBase.Icons.HarryBar*`, and
BeansHudItem's `xOffset`, `yFinalOffset`, `Image`, `BeanTexture`.

## Open points
- The original frog has falling physics; placed frogs already sit on the floor, so it is not
  done here yet.
- Beans that touch a Tut1 gnome vanish with the pickup sound; comes with the gnomes.

## Changes from the original
- The original's last flying ticks divide by less than one tick left (or a negative number)
  and push the item past or away from the point for a frame; here it lands on the point and
  stays.
- A frog touched by Harry starts its flight in the same tick. In the original its state code
  starts a tick later, so its first flying tick may run before the flight time is set (and
  push it away for a frame), depending on the order the engine runs things in.
