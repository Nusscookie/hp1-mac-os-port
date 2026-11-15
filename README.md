# Harry Potter and the Philosopher's Stone (PC, 2001) for macOS

A native macOS port of the 2001 PC game *Harry Potter and the Philosopher's Stone* (US title: *Sorcerer's
Stone*), for people who still own the original CD. It runs the game's own data on a Mac, natively on Apple
Silicon, with no Wine, virtual machine or emulator.

**This repository contains no game files.** You need your own original PC CD. On first launch the app asks you to
insert it and copies the game data from it.

> Unofficial fan project. Not affiliated with or endorsed by Warner Bros., Electronic Arts, KnowWonder/Amaze
> Entertainment or J.K. Rowling. Harry Potter names and characters are trademarks of Warner Bros. Entertainment Inc.

> **Branch `rust-rewrite`:** work in progress on a from-scratch Rust engine that will also run on Windows,
> Linux and mobile, with every behaviour written by hand. See [`rewrite/README.md`](rewrite/README.md). The
> playable version is the SurrealEngine port on `main`.

## How it works

The original game is built on Unreal Engine 1, and its gameplay is written in UnrealScript, stored inside the
game's data files. This port uses a modified version of [SurrealEngine](https://github.com/dpjudas/SurrealEngine),
an open-source re-implementation of the Unreal Engine 1 runtime (in `engine/`), and adds what Harry Potter needs on
top: skeletal animation, particle effects, the save system, game-specific functions and Mac app packaging.

## What you need

- A Mac with Apple Silicon (M1 or newer) and macOS 15 or newer.
- The original PC CD. Tested with the European English/French/German edition; other editions should work.
  If your Mac has no disc drive, use a USB CD/DVD drive, or copy the CD's files onto your Mac first.

## Download and play

1. Download `HP1-macOS-AppleSilicon.zip` from the [latest release](https://github.com/Nusscookie/hp1-mac-os-port/releases/latest)
   and unzip it.
2. Move `HP1.app` to your Applications folder.
3. Open it. The app is not signed by an identified Apple developer, so macOS blocks the first launch. Click
   **Done**, then open **System Settings → Privacy & Security**, scroll down and click **Open Anyway**.
   (Or run `xattr -dr com.apple.quarantine /Applications/HP1.app` in Terminal.)
4. Insert your CD and click **Install** (see below).

The download contains no game files, only the engine.

## Build it yourself

You need the Xcode command line tools (`xcode-select --install`), [CMake](https://cmake.org) (`brew install cmake`)
and an internet connection for the first build (it downloads MoltenVK, the Vulkan-to-Metal layer).

    git clone https://github.com/Nusscookie/hp1-mac-os-port.git
    cd hp1-mac-os-port
    ./build.sh
    open build/HP1.app

Either way, on first launch the app shows an installer window:

1. Insert your Harry Potter CD. The app detects it on its own.
2. Pick the language, if your disc has several.
3. Click **Install**. About 520 MB is copied to `~/Library/Application Support/HP1 Port`, and then the game starts.

You can also click **Choose Folder…** to install from a folder holding a copy of the CD, or from an existing
Windows installation of the game. After that, the CD is no longer needed. Saved games are stored in the `save`
folder inside the game folder, as in the original.

You can move `HP1.app` to your Applications folder or the Dock.

### Mac extras

- **Skipping:** press Space, Enter, Esc or click to skip the startup logos and the storybook pages, and Space, Enter
  or Esc to skip in-game cutscenes (the original only allowed this in its hidden debug mode).
- **Widescreen:** the 3D view and the HUD fill widescreen displays, while the menus keep the original 4:3 layout.
- **Controls:** W/A/S/D work like the arrow keys (walk, turn, and steer the broom), Space jumps, Shift is the broom
  boost. Your own key changes in the game's options are kept.
- **Mac shortcuts:** Cmd+Q quits (and saves your settings), Cmd+F or Ctrl+Cmd+F switches between full screen and a
  window, Cmd+M minimizes. Switching to another app (Cmd+Tab) pauses the game and its sound and frees the mouse.
  While the game's menu is open (Esc), the mouse is free, so in full screen you can move it to the top of the screen
  to reach the macOS menu bar.

## Is this legal?

This project is set up the way other engine re-implementations (OpenMW, ScummVM, OpenRCT2) do it:

- No game content is distributed. The code here is written from scratch or comes from open-source projects. The
  game's data, music, art and scripts come only from your own disc, at install time, on your own Mac.
- The installer copies only data files (packages, maps, textures, sounds, music, text and settings). The Windows
  program on the disc (`HP.exe`), its copy-protection driver and the setup programs are never read, run or changed.
  Nothing is cracked or bypassed.
- Even the app's Dock icon is read from your installed game data. Until then it shows a plain "HP1" placeholder.

This is not legal advice. Please only use this with a copy of the game you own.

## Status

Work in progress. All story levels load and the game is playable from the main menu, including new games, saving
and loading. Expect bugs: please report them as [issues](https://github.com/Nusscookie/hp1-mac-os-port/issues), with
the level and what happened.

## For developers

    python3 tools/install_from_cd.py --lang eng      # copy the CD into gamedata/ (git-ignored) instead
    tools/make_app.sh [game folder]                  # app that plays that folder (default: gamedata/ if present)
    tools/make_release.sh                            # release zip: macOS 15+, no game data, no local paths
    engine/build/SurrealEngine.app/Contents/MacOS/SurrealEngine --autostart "$PWD/gamedata"

Engine options: `--url=<map>` boots straight into a map, `--maxticks=N` quits after N frames,
`--screenshot=<file.bmp>` saves the last frame, `--loadall` runs the package load test,
`--dumpmesh=Package.Mesh [--dumpseq=list|<name>]` prints a skeletal mesh's data,
`--dumpdefaults=Package.Class` prints a class's default properties, `--nolauncher` skips the game picker,
`--install-from=<CD or folder> [--language=eng|ger|fre] <game folder>` installs without a window.

Scripted input for tests: `--input="tick:action:arg;..."` with actions `press`, `release`, `tap` (key names),
`click`/`mouse` (window fractions), `rawmouse dx,dy`, `uimouse x,y` (menu cursor in 640x480 menu space),
`shot <file.bmp>` and `cmd <console command>`. Set `SURREAL_NO_CRASH_WINDOW=1` to get a backtrace on stderr instead
of the crash window, and `SURREAL_INSTALLER_AUTOINSTALL=1` to have the installer start as soon as it finds a disc.

    python3 tools/ue1pkg.py info|natives|exports|scripts ...   # package inspector, script source dump
    tests/smoke.sh [map ...]                                   # load-all test, optional map boots

`gamedata/` (installed game files) and `reference/` (script source dumped from your own copy) are git-ignored and
must never be committed.

### The engine

`engine/` is a modified copy of SurrealEngine, merged in with its full history (git subtree), so you can see
upstream's commits and this port's changes on top of them. To pull in newer upstream work:

    git subtree pull --prefix=engine https://github.com/dpjudas/SurrealEngine master

SurrealEngine's authors ask that changes made mainly with AI tools are not sent to them as pull requests. Much of
this port was written with Claude (an AI assistant), so please keep these changes here and don't open pull
requests or issues for them upstream.

## Credits and license

- [SurrealEngine](https://github.com/dpjudas/SurrealEngine) by Magnus Norddahl, Lupert Everett and contributors,
  zlib license (see `engine/LICENSE.md`, which also lists its third-party components). `engine/` is an altered
  version; see `engine/MODIFIED.md`.
- Everything outside `engine/` is under the zlib license in `LICENSE`.
