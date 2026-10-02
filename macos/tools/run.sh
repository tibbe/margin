#!/bin/sh
# Builds this checkout's Margin.app and opens FILEs in it, as `margin FILE…`
# does. Quits the checkout's running Margin first: files opened in it would
# go to the running process, which still runs the build before this one.
#
#   macos/tools/run.sh [FILE…]
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
app=$root/macos/build/DerivedData/Build/Products/Debug/Margin.app
# xcodebuild picks this destination itself, but warns that several match.
xcodebuild -quiet -project "$root/macos/Margin.xcodeproj" -scheme Margin \
    -destination "platform=macOS,arch=$(uname -m)" \
    -derivedDataPath "$root/macos/build/DerivedData" build

# Several checkouts share the bundle ID, so AppKit finds the running app by
# its bundle path. It quits as with Command-Q, saving its documents.
pids=$(osascript -l JavaScript - "$app" <<'EOF'
ObjC.import("AppKit");
function run(argv) {
    const app = $.NSURL.fileURLWithPath(argv[0]).URLByResolvingSymlinksInPath.path.js;
    return ObjC.unwrap($.NSWorkspace.sharedWorkspace.runningApplications)
        .filter((a) => a.bundleURL.js && a.bundleURL.URLByResolvingSymlinksInPath.path.js === app)
        .map((a) => {
            a.terminate;
            return a.processIdentifier;
        })
        .join(" ");
}
EOF
)
tries=0
for pid in $pids; do
    while kill -0 "$pid" 2>/dev/null; do
        tries=$((tries + 1))
        if [ "$tries" -gt 50 ]; then
            echo "error: the running Margin didn't quit within 5 seconds" >&2
            exit 1
        fi
        sleep 0.1
    done
done
exec "$app/Contents/Helpers/margin" "$@"
