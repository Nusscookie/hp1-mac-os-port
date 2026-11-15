#!/bin/sh
# Smoke tests for the HP1 port. Usage: tests/smoke.sh [map ...]
#   no arguments: load-all test only (headless)
#   with maps:    also boots each map for 600 frames (opens the game window) and fails on a fatal error
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/engine/build/SurrealEngine.app/Contents/MacOS/SurrealEngine"
data="$root/gamedata"
out="${TMPDIR:-/tmp}/hp1-smoke"
mkdir -p "$out"
status=0
export SURREAL_NO_CRASH_WINDOW=1  # crashes print a backtrace instead of opening a window

"$bin" --autostart --loadall "$data" > "$out/loadall.log" 2>&1
grep -E "^(PACKAGE FAILED|LOAD FAILED|UNREAD|NATIVE MISSING|natives without|loadall)" "$out/loadall.log"

for map in "$@"; do
	# perl alarm: kill a map that hangs (exit code 142)
	perl -e 'alarm 90; exec @ARGV' "$bin" --autostart --url="$map" --maxticks=400 --screenshot="$out/$map.bmp" "$data" > "$out/$map.log" 2>&1
	code=$?
	unimplemented=$(grep -o "Unimplemented: .*" "$out/$map.log" | sort -u | wc -l | tr -d ' ')
	if [ $code -ne 0 ] || grep -q "^FATAL" "$out/$map.log"; then
		echo "FAIL $map (exit $code): $(grep '^FATAL' "$out/$map.log" | head -1)"
		status=1
	else
		errors=$(grep -c "Script error" "$out/$map.log")
		echo "ok   $map ($unimplemented unimplemented natives hit, $errors script errors)"
	fi
done
echo "logs and screenshots: $out"
exit $status
