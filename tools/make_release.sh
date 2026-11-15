#!/bin/sh
# Build the downloadable app for a GitHub release: build/release/HP1-macOS-AppleSilicon.zip
#
# Differences from ./build.sh:
# - its own build folder (engine/build-release), so the normal development build is left alone;
# - runs on macOS 15 and newer, not only on the macOS version of the building Mac;
# - contains no paths from the building Mac (no user or folder names);
# - never contains game data: on first launch it asks for the CD, like a fresh build from source.
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
build="$root/engine/build-release"
out="$root/build/release"
app="$out/HP1.app"
zip="$out/HP1-macOS-AppleSilicon.zip"

# Source paths end up in the binary through __FILE__ (asserts, logs); make them relative.
prefixmap="-ffile-prefix-map=$root/="
cmake -S engine -B "$build" -DCMAKE_BUILD_TYPE=Release \
	-DCMAKE_OSX_ARCHITECTURES=arm64 \
	-DCMAKE_OSX_DEPLOYMENT_TARGET=15.0 \
	-DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
	-DCMAKE_C_FLAGS="$prefixmap" -DCMAKE_CXX_FLAGS="$prefixmap" -DCMAKE_OBJC_FLAGS="$prefixmap" -DCMAKE_OBJCXX_FLAGS="$prefixmap"
cmake --build "$build" --target SurrealEngine -j "$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"

tools/make_app.sh --no-data --engine-build "$build" --out "$app"

# Only the bundle's own Frameworks folder may be searched for libraries.
for bin in "$app/Contents/MacOS/HP1" "$app/Contents/Frameworks/"*.dylib; do
	otool -l "$bin" | awk '/LC_RPATH/ { getline; getline; print $2 }' | while read -r rpath; do
		[ "$rpath" = "@executable_path/../Frameworks" ] || [ "$rpath" = "@loader_path" ] || install_name_tool -delete_rpath "$rpath" "$bin"
	done
done
codesign --force --deep --sign - "$app" >/dev/null

# Refuse to publish anything that still names the building Mac's folders or user.
if grep -rl -F "$root" "$app" >/dev/null 2>&1 || grep -rl -F "$HOME" "$app" >/dev/null 2>&1; then
	echo "error: the app still contains local paths:"
	grep -rl -F "$HOME" "$app"
	exit 1
fi
if /usr/libexec/PlistBuddy -c "Print :SEGameFolder" "$app/Contents/Info.plist" >/dev/null 2>&1; then
	echo "error: the release app points at a local game folder"
	exit 1
fi

rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
echo "built $zip ($(du -h "$zip" | cut -f1))"
