#!/bin/sh
# Builds the app and runs the unit tests named, or all of them, then prints
# only what failed: build errors, and each failing test with its file, line
# and message. Pass tests as -only-testing takes them: MarginTests/CLASS or
# MarginTests/CLASS/TEST.
#
#   macos/tools/test.sh [TEST…]
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
build=$root/macos/build
result=$build/Test.xcresult
log=$build/test.log

only='-only-testing:MarginTests'
if [ $# -gt 0 ]; then
    only=''
    for t in "$@"; do only="$only -only-testing:$t"; done
fi

mkdir -p "$build"
# xcodebuild won't write over an earlier run's results.
rm -rf "$result"
status=0
# xcodebuild picks this destination itself, but warns that several match.
# shellcheck disable=SC2086 # one -only-testing per word
xcodebuild -quiet -project "$root/macos/Margin.xcodeproj" -scheme Margin \
    -destination "platform=macOS,arch=$(uname -m)" -resultBundlePath "$result" \
    test $only >"$log" 2>&1 || status=$?

# An interrupted run leaves no readable results.
if ! xcrun xcresulttool get test-results summary --path "$result" >"$build/test-summary.json" 2>/dev/null; then
    tail -n 40 "$log"
    exit "$status"
fi
# A source location is a URL whose fragment holds 0-based numbers.
xcrun xcresulttool get build-results --path "$result" |
    jq -r '.errors[] | if .sourceURL then
            (.sourceURL | sub("^file://"; "") | split("#")) as [$path, $at]
            | ($at | capture("StartingLineNumber=(?<n>[0-9]+)").n | tonumber + 1) as $line
            | ($at | capture("StartingColumnNumber=(?<n>[0-9]+)").n | tonumber + 1) as $col
            | "\($path):\($line):\($col): error: \(.message)"
        else "error: \(.message)" end'
xcrun xcresulttool get test-results tests --path "$result" |
    jq -r '.. | objects | select(.nodeType == "Test Case" and .result == "Failed")
        | .nodeIdentifier as $t | .. | objects | select(.nodeType == "Failure Message")
        | "\($t): \(.name)"'
jq -r 'select(.totalTestCount > 0) | "\(.passedTests) passed, \(.failedTests) failed, \(.skippedTests) skipped"' "$build/test-summary.json"
if [ "$status" -ne 0 ]; then
    echo "xcodebuild exited with $status; its output is in $log"
fi
exit "$status"
