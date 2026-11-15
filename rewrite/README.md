# HP1 rewrite (Rust)

A from-scratch Rust engine for *Harry Potter and the Philosopher's Stone* (PC, 2001), meant to run on macOS,
Windows, Linux and later mobile. It still needs the original CD: all maps, textures, models, animations,
sounds, music and text come from your own disc. No Unreal code is run or interpreted: every behaviour
(Harry, spells, characters, cutscenes, menus, physics) is written by hand in Rust.

This lives on the `rust-rewrite` branch. The playable Mac port on `main` (SurrealEngine based) stays as
it is until the rewrite can play the whole game.

## How it fits together

1. **Converter** (planned: `hp1-convert`): runs once at install time on the player's computer. It reads
   the CD's Unreal packages and writes the game folder in our own simple formats. It is the only code that
   knows Unreal's file formats. Script code in the packages is skipped.
2. **Game** (planned: `hp1`): reads only our formats. All behaviour is our Rust code.

| Crate | What it does | Status |
|---|---|---|
| `crates/ue1-read` | Reads Unreal packages: header, names, imports, exports, property values | Working |
| `apps/hp1-tools` | `info`, `exports`, `props`, `check-data` | Working |
| `hp1-data`, `hp1-convert` | Our formats and the CD converter | Next |
| `hp1-engine`, `hp1-game`, `hp1-render`, `hp1-audio`, `hp1-platform`, `apps/hp1` | The game | Later |

Planned libraries: SDL3 (window, input, gamepads, touch, audio output), wgpu (graphics), symphonia (audio
decoding), glam (math), serde (our data formats).

## Build and try

You need Rust (`brew install rustup && rustup default stable`, or [rustup.rs](https://rustup.rs)).

    cd rewrite
    cargo test
    cargo run --release -p hp1-tools -- check-data ../gamedata
    cargo run --release -p hp1-tools -- props ../gamedata/Maps/Lev_Tut1.unr Tut1Dumbledore1

`check-data` opens every package of an installed game (from the Mac port's installer or
`tools/install_from_cd.py`) and reads every object's property list.

## Rules

- Never commit game data: no `gamedata/`, no converted files, no `reference/`.
- Behaviour is written from our own descriptions in `specs/`, never translated from the original scripts.
  See `specs/README.md`.
- The format knowledge comes from SurrealEngine (zlib license) and this repository's `tools/ue1pkg.py`.
