#!/bin/sh
# Build build/HP1.app from the engine build.
#
# Usage: tools/make_app.sh [--no-data] [--engine-build <dir>] [--out <path/HP1.app>] [path to game data]
#
# Without game data (the normal case for a fresh clone) the app asks for the CD on first launch and
# installs the data into ~/Library/Application Support/HP1 Port. Given a folder with installed game
# data (or if gamedata/ holds it), the app plays that folder instead and uses the game's own icon.
# --no-data always makes the CD-installing app (used for releases, see tools/make_release.sh).
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
build="$root/engine/build"
app="$root/build/HP1.app"
nodata=0
while [ $# -gt 0 ]; do
	case "$1" in
		--no-data) nodata=1; shift ;;
		--engine-build) build="$2"; shift 2 ;;
		--out) app="$2"; shift 2 ;;
		*) break ;;
	esac
done
engine="$build/SurrealEngine.app"

data=""
if [ -n "$1" ]; then
	data="$(cd "$1" && pwd)"
	[ -f "$data/System/HPBase.u" ] || { echo "no Harry Potter 1 data in $data"; exit 1; }
elif [ "$nodata" = 0 ] && [ -f "$root/gamedata/System/HPBase.u" ]; then
	data="$root/gamedata"
fi
[ -x "$engine/Contents/MacOS/SurrealEngine" ] || { echo "engine not built: $engine (run ./build.sh)"; exit 1; }

rm -rf "$app"
mkdir -p "$(dirname "$app")"
cp -R "$engine" "$app"
# Make the bundle self-contained: the engine binary also looks in the build tree for these.
cp -L "$build/libSurrealVideo.dylib" "$build/Thirdparty/openal-soft/libopenal.1.dylib" "$app/Contents/Frameworks/"

# The engine binary itself is the app (so the Dock and Activity Monitor show "HP1", not a shell).
# Started without arguments it reads its game folder from Info.plist (see GameApp.cpp).
mv "$app/Contents/MacOS/SurrealEngine" "$app/Contents/MacOS/HP1"

plist="$app/Contents/Info.plist"
set_key() { /usr/libexec/PlistBuddy -c "Set :$1 \"$2\"" "$plist" 2>/dev/null || /usr/libexec/PlistBuddy -c "Add :$1 string \"$2\"" "$plist"; }
set_bool() { /usr/libexec/PlistBuddy -c "Delete :$1" "$plist" 2>/dev/null || true; /usr/libexec/PlistBuddy -c "Add :$1 bool $2" "$plist"; }
set_key CFBundleExecutable HP1
set_key CFBundleName "Harry Potter"
set_key CFBundleDisplayName "Harry Potter"
set_key CFBundleIdentifier io.github.nusscookie.hp1port
set_key CFBundleIconFile hp1
set_key CFBundleShortVersionString "${HP1_VERSION:-0.1.0}"
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
