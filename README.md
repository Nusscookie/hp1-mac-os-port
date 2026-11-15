# Harry Potter 1 (PC, 2001) — native macOS port

Private, unofficial. Runs the original game data from the CD on Apple Silicon through a fork of
[SurrealEngine](https://github.com/dpjudas/SurrealEngine) (`engine/`, branch `hp1`). No game data is stored in git.
Changes in `engine/` stay in this fork; upstream asks that LLM-assisted changes are not sent to them.

## Setup

`engine/` is its own git repository (a clone of SurrealEngine with the port's changes on branch `hp1`)
and is not tracked by this one.

    python3 tools/install_from_cd.py --lang eng      # or ger / fre; copies the CD into gamedata/
    cmake -S engine -B engine/build -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_OSX_ARCHITECTURES=arm64
    cmake --build engine/build --target SurrealEngine -j 10

## Play

    tools/make_app.sh            # builds build/HP1.app (icon from the game, data stays in gamedata/)
    open build/HP1.app           # or drag it to /Applications or the Dock

The app remembers where `gamedata/` is; rebuild it with `tools/make_app.sh <path>` if the data moves.
Saves go to `gamedata/save/` like the original (`SaveN.usa` plus `GameSaveInfoN.txt`).

## Run from the terminal

    engine/build/SurrealEngine.app/Contents/MacOS/SurrealEngine --autostart "$PWD/gamedata"

Options: `--url=<map>` boots straight into a map, `--maxticks=N` quits after N frames,
`--screenshot=<file.bmp>` saves the last frame, `--loadall` runs the package load test,
`--dumpmesh=Package.Mesh [--dumpseq=list|<name>]` prints a skeletal mesh's data,
`--dumpdefaults=Package.Class` prints a class's default properties, `--nolauncher` skips the game picker.

Scripted input for tests: `--input="tick:action:arg;..."` with actions `press`, `release`, `tap` (key names),
`click`/`mouse` (window fractions), `rawmouse dx,dy`, `uimouse x,y` (menu cursor in 640x480 menu space),
`shot <file.bmp>` and `cmd <console command>`. Set `SURREAL_NO_CRASH_WINDOW=1` to get a backtrace on
stderr instead of the crash window.

## Tools and tests

    python3 tools/ue1pkg.py info|natives|exports|scripts ...   # package inspector, script source dump
    tests/smoke.sh [map ...]                                   # load-all test, optional map boots
