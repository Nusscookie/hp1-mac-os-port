#!/bin/sh
# Build build/HP1.app from the engine build.
#
# Usage: tools/make_app.sh [path to game data]
#
# Without game data (the normal case for a fresh clone) the app asks for the CD on first launch and
# installs the data into ~/Library/Application Support/HP1 Port. Given a folder with installed game
# data (or if gamedata/ holds it), the app plays that folder instead and uses the game's own icon.
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
engine="$root/engine/build/SurrealEngine.app"
app="$root/build/HP1.app"

data=""
if [ -n "$1" ]; then
	data="$(cd "$1" && pwd)"
	[ -f "$data/System/HPBase.u" ] || { echo "no Harry Potter 1 data in $data"; exit 1; }
elif [ -f "$root/gamedata/System/HPBase.u" ]; then
	data="$root/gamedata"
fi
[ -x "$engine/Contents/MacOS/SurrealEngine" ] || { echo "engine not built: $engine (run ./build.sh)"; exit 1; }

rm -rf "$app"
mkdir -p "$root/build"
cp -R "$engine" "$app"
# Make the bundle self-contained: the engine binary also looks in the build tree for these.
cp -L "$root/engine/build/libSurrealVideo.dylib" "$root/engine/build/Thirdparty/openal-soft/libopenal.1.dylib" "$app/Contents/Frameworks/"

# The engine binary itself is the app (so the Dock and Activity Monitor show "HP1", not a shell).
# Started without arguments it reads its game folder from Info.plist (see GameApp.cpp).
mv "$app/Contents/MacOS/SurrealEngine" "$app/Contents/MacOS/HP1"

plist="$app/Contents/Info.plist"
set_key() { /usr/libexec/PlistBuddy -c "Set :$1 \"$2\"" "$plist" 2>/dev/null || /usr/libexec/PlistBuddy -c "Add :$1 string \"$2\"" "$plist"; }
set_bool() { /usr/libexec/PlistBuddy -c "Delete :$1" "$plist" 2>/dev/null || true; /usr/libexec/PlistBuddy -c "Add :$1 bool $2" "$plist"; }
set_key CFBundleExecutable HP1
set_key CFBundleName "Harry Potter"
set_key CFBundleDisplayName "Harry Potter"
set_key CFBundleIdentifier local.hp1port
set_key CFBundleIconFile hp1
set_key CFBundleShortVersionString 1.0
set_key LSApplicationCategoryType public.app-category.adventure-games
set_key SEDataFolderName "HP1 Port"
set_bool NSHighResolutionCapable true

if [ -n "$data" ]; then
	# The game's own icon. The repository contains no game art, so it is made from the data at build time.
	python3 "$root/tools/ico_to_icns.py" "$data/System/hp.ico" "$app/Contents/Resources/hp1.icns"
	set_key SEGameFolder "$data"
	set_bool SEKeepBundleIcon true
else
	# Neutral icon until the game is installed; while running, the app shows the game's icon from the data.
	python3 "$root/tools/ico_to_icns.py" --placeholder "$app/Contents/Resources/hp1.icns"
fi

# Ad-hoc signature so macOS runs it locally (not for distribution).
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || echo "warning: ad-hoc signing failed"

if [ -n "$data" ]; then
	echo "built $app (game data: $data)"
else
	echo "built $app (asks for the CD on first launch)"
fi
