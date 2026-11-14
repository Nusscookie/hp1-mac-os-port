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

## Run

    engine/build/SurrealEngine.app/Contents/MacOS/SurrealEngine --autostart "$PWD/gamedata"

Options: `--url=<map>` boots straight into a map, `--maxticks=N` quits after N frames,
`--screenshot=<file.bmp>` saves the last frame, `--loadall` runs the package load test,
`--dumpmesh=Package.Mesh [--dumpseq=list|<name>]` prints a skeletal mesh's data.

## Tools and tests

    python3 tools/ue1pkg.py info|natives|exports|scripts ...   # package inspector, script source dump
    tests/smoke.sh [map ...]                                   # load-all test, optional map boots
