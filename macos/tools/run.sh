#!/bin/sh
# Builds this checkout's Margin.app and opens FILEs in it. Quits the
# checkout's running Margin first, so the new build is the one that runs.
# Pass -g to open it in the background, as tests do, so it doesn't take focus.
#
#   macos/tools/run.sh [-g] [FILE…]
set -eu
background=
case ${1:-} in
    -g) background=-g; shift ;;
    -*) echo "usage: macos/tools/run.sh [-g] [FILE…]" >&2; exit 2 ;;
esac
# open refuses missing files, so refuse them before quitting the running app.
for file in "$@"; do
    if [ ! -e "$file" ]; then
        echo "error: $file does not exist" >&2
        exit 1
    fi
done

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
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
open ${background:+"$background"} -n -a "$app" "$@"
