#!/bin/sh
# Build the Harry Potter 1 Mac port: compiles the engine and makes build/HP1.app.
# Needs the Xcode command line tools and CMake. No game files are needed to build;
# the app asks for your CD on first launch.
set -e
root="$(cd "$(dirname "$0")" && pwd)"
cd "$root"

if ! xcode-select -p >/dev/null 2>&1; then
	echo "The Xcode command line tools are missing. Install them with:"
	echo "    xcode-select --install"
	exit 1
fi
if ! command -v cmake >/dev/null 2>&1; then
	echo "CMake is missing. Install it with Homebrew (https://brew.sh):"
	echo "    brew install cmake"
	exit 1
fi

jobs="$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"
if [ ! -f engine/build/CMakeCache.txt ]; then
	cmake -S engine -B engine/build -DCMAKE_BUILD_TYPE=Release
fi
cmake --build engine/build --target SurrealEngine -j "$jobs"
tools/make_app.sh

echo
echo "Done. Start the game with:"
echo "    open build/HP1.app"
echo "(or drag build/HP1.app into your Applications folder)"
