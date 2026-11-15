# Modified version of SurrealEngine

This copy of SurrealEngine is altered for the Harry Potter 1 macOS port
(https://github.com/Nusscookie/hp1-mac-os-port). It is not the original software.

The changes sit on top of upstream commit `92d00ca9` ("Use more traces for light/actor visibility check").
They include Harry Potter 1 support (skeletal animation, particle effects, save games and game-specific natives),
the CD installer window (`SurrealEngine/UI/Installer`), macOS app-bundle handling and audio panning changes,
skippable intro and cutscenes, widescreen rendering for HP1, a macOS menu bar with pause-on-app-switch, HP1's
oriented box collision (`CT_Box`), movers that use HP1's own `PhysAlpha`/`PhysRate`, NPCs that stop when a
scripted move ends, and VM fixes for empty states and state code that ends without a stop.
`git log -- engine` in the port's repository lists every change.

Upstream asks that changes made mainly with AI tools are not sent to them as pull requests
(see `NO-AI Code Rule.md`). Please keep these changes in this fork.
