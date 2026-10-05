#!/bin/sh
# Prints the path of this checkout's Margin.app, as built for running here:
# Xcode builds each checkout in a folder of its own, which it names.
#
#   macos/tools/app.sh
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
xcodebuild -project "$root/macos/Margin.xcodeproj" -scheme Margin -destination "platform=macOS,arch=$(uname -m)" \
    -showBuildSettings -json 2>/dev/null |
    jq -r '.[] | select(.target == "Margin") | .buildSettings | .TARGET_BUILD_DIR + "/" + .WRAPPER_NAME'
