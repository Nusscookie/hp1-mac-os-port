#!/bin/sh
# Build build/HP1.app: a double-clickable Harry Potter 1 for this Mac.
# The game data stays where it is (gamedata/ by default); the app only remembers its path.
#
# Usage: tools/make_app.sh [path to game data]
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
data="$(cd "${1:-$root/gamedata}" && pwd)"
engine="$root/engine/build/SurrealEngine.app"
app="$root/build/HP1.app"

[ -f "$data/System/HPBase.u" ] || { echo "no Harry Potter 1 data in $data (run tools/install_from_cd.py first)"; exit 1; }
[ -x "$engine/Contents/MacOS/SurrealEngine" ] || { echo "engine not built: $engine"; exit 1; }

rm -rf "$app"
mkdir -p "$root/build"
cp -R "$engine" "$app"
# Make the bundle self-contained: the engine binary also looks in the build tree for these.
cp -L "$root/engine/build/libSurrealVideo.dylib" "$root/engine/build/Thirdparty/openal-soft/libopenal.1.dylib" "$app/Contents/Frameworks/"
python3 "$root/tools/ico_to_icns.py" "$data/System/hp.ico" "$app/Contents/Resources/hp1.icns"

# The engine binary itself is the app (so the Dock and Activity Monitor show "HP1", not a shell).
# Started without arguments it reads SEGameFolder from Info.plist and boots that game.
mv "$app/Contents/MacOS/SurrealEngine" "$app/Contents/MacOS/HP1"

plist="$app/Contents/Info.plist"
set_key() { /usr/libexec/PlistBuddy -c "Set :$1 \"$2\"" "$plist" 2>/dev/null || /usr/libexec/PlistBuddy -c "Add :$1 string \"$2\"" "$plist"; }
set_key CFBundleExecutable HP1
set_key CFBundleName "Harry Potter"
set_key CFBundleDisplayName "Harry Potter"
set_key CFBundleIdentifier local.hp1port
set_key CFBundleIconFile hp1
set_key CFBundleShortVersionString 1.0
set_key SEGameFolder "$data"
set_key LSApplicationCategoryType public.app-category.adventure-games
/usr/libexec/PlistBuddy -c "Add :NSHighResolutionCapable bool true" "$plist" 2>/dev/null || true

# Ad-hoc signature so macOS runs it locally (not for distribution).
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || echo "warning: ad-hoc signing failed"

echo "built $app (game data: $data)"
