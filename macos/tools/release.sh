#!/bin/sh
# Builds Margin.app for release, for Apple silicon and Intel, and zips it
# into macos/build/release/Margin.zip for GitHub Releases. Pass the version
# being released; it must match the one in the sources.
#
# The app is signed ad hoc, not with a Developer ID, so it isn't notarized
# and Gatekeeper blocks it once downloaded until its quarantine is removed.
set -eu
cd "$(dirname "$0")/../.."
version=${1:?usage: macos/tools/release.sh VERSION}

# The app and the CLI each carry a version; both must be the one released.
app_version=$(sed -n 's/^MARKETING_VERSION = //p' macos/Configurations/Common.xcconfig)
cli_version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
if [ "$app_version" != "$version" ] || [ "$cli_version" != "$version" ]; then
    echo "error: releasing $version, but MARKETING_VERSION is $app_version and Cargo.toml's version is $cli_version" >&2
    exit 1
fi

xcodebuild -project macos/Margin.xcodeproj -scheme Margin -configuration Release \
    -derivedDataPath macos/build/DerivedData build
app=macos/build/DerivedData/Build/Products/Release/Margin.app
codesign --verify --deep --strict "$app"
for binary in "$app/Contents/MacOS/Margin" "$app/Contents/Helpers/margin"; do
    archs=$(lipo -archs "$binary")
    case $archs in
        *arm64*x86_64* | *x86_64*arm64*) ;;
        *) echo "error: $binary is built for $archs only" >&2; exit 1 ;;
    esac
done

out=macos/build/release
rm -rf "$out"
mkdir -p "$out"
ditto -c -k --sequesterRsrc --keepParent "$app" "$out/Margin.zip"
shasum -a 256 "$out/Margin.zip"
