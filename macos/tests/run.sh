#!/bin/sh
# Runs the editor's UI scripts (see Sources/ScriptDriver.swift) and compares
# what they print with the expected output next to them.
#   macos/tests/run.sh            all tests
#   macos/tests/run.sh editing    some tests
#   UPDATE=1 macos/tests/run.sh   accept the current output
# Needs a test build (macos/build.sh debug). The app briefly opens a window.
set -u
cd "$(dirname "$0")"
app=../build/Margin.app/Contents/MacOS/Margin
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
[ $# -eq 0 ] && set -- *.txt
for script in "$@"; do
    script=${script%.txt}.txt
    [ -f "$script" ] || continue
    name=${script%.txt}
    : > "$work/doc.md"
    MARGIN_DATA_DIR="$work/$name.data" MARGIN_SCRIPT="$PWD/$script" "$app" "$work/doc.md" > "$work/$name.out" 2>&1
    if [ "${UPDATE:-}" = 1 ]; then
        cp "$work/$name.out" "$name.out"
        echo "updated $name"
    elif diff -u "$name.out" "$work/$name.out" > "$work/$name.diff"; then
        echo "ok   $name"
    else
        echo "FAIL $name"
        cat "$work/$name.diff"
        fail=1
    fi
done
exit $fail
