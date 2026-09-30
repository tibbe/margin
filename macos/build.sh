#!/bin/sh
# Builds Margin.app into macos/build/: the Rust core as a static library,
# its Swift bindings, the AppKit app, the icon's asset catalog, and the
# `margin` CLI inside the bundle.
#   macos/build.sh           release: universal (Apple silicon and Intel),
#                            with a dSYM, signed ad hoc with the hardened runtime
#   macos/build.sh debug     this Mac's architecture, scriptable for tests
# SCRIPTING=1 adds the test driver to a release build (for benchmarks).
set -eu
cd "$(dirname "$0")"
root=$(cd .. && pwd)
profile=${1:-release}
build="$PWD/build"
gen="$build/gen"
app="$build/Margin.app"
min=26.0

if [ "$profile" = release ]; then
    cargo_flag=--release
    swift_opt="-O"
    archs="arm64 x86_64"
else
    cargo_flag=
    swift_opt="-Onone -D SCRIPTING"
    archs=$(uname -m)
fi
if [ "${SCRIPTING:-}" = 1 ]; then swift_opt="$swift_opt -D SCRIPTING"; fi
cargo_dir=$([ "$profile" = release ] && echo release || echo debug)
rust_target() { [ "$1" = arm64 ] && echo aarch64-apple-darwin || echo x86_64-apple-darwin; }

export MACOSX_DEPLOYMENT_TARGET=$min
for arch in $archs; do
    cargo build $cargo_flag --target "$(rust_target "$arch")" -p margin-ffi -p margin --manifest-path "$root/Cargo.toml"
done
native="$root/target/$(rust_target "$(uname -m)")/$cargo_dir"

# Bindings come from the library's metadata; any architecture will do.
rm -rf "$gen" && mkdir -p "$gen"
cargo run -q $cargo_flag -p margin-ffi --bin uniffi-bindgen --manifest-path "$root/Cargo.toml" -- \
    generate --library "$native/libmargin_ffi.dylib" --language swift --out-dir "$gen"

rm -rf "$app" "$build/Margin.app.dSYM"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Helpers" "$build/obj"
apps=""
clis=""
for arch in $archs; do
    lib="$root/target/$(rust_target "$arch")/$cargo_dir"
    mod="$build/obj/$arch"
    mkdir -p "$mod"
    # Compiled and linked separately, so the object files (which the dSYM
    # is made from) stay.
    # The bindings are a module of their own, so the app's default main
    # actor isolation doesn't reach them: an imported module keeps its own.
    (cd "$mod" && swiftc $swift_opt -wmo -g -swift-version 6 -target "$arch-apple-macos$min" \
        -module-name margin_ffi -parse-as-library \
        -Xcc -fmodule-map-file="$gen/margin_ffiFFI.modulemap" \
        -emit-module -emit-module-path "$mod/margin_ffi.swiftmodule" \
        "$gen/margin_ffi.swift" -c -o "$mod/margin_ffi.o")
    # Swift 6 on the main actor, as Xcode sets up new apps. Warnings fail
    # the build, except deprecations, so a new SDK doesn't break it.
    # Run from build/obj, where swiftc leaves its module files.
    (cd "$build/obj" && swiftc $swift_opt -wmo -g -swift-version 6 -target "$arch-apple-macos$min" \
        -default-isolation MainActor \
        -enable-upcoming-feature NonisolatedNonsendingByDefault \
        -enable-upcoming-feature InferIsolatedConformances \
        -warnings-as-errors -Wwarning DeprecatedDeclaration \
        -module-name Margin -parse-as-library \
        -I "$mod" -Xcc -fmodule-map-file="$gen/margin_ffiFFI.modulemap" \
        "$OLDPWD"/Sources/*.swift \
        -c -o "$build/obj/Margin-$arch.o")
    swiftc -g -target "$arch-apple-macos$min" "$build/obj/Margin-$arch.o" "$mod/margin_ffi.o" "$lib/libmargin_ffi.a" \
        -framework AppKit -framework UniformTypeIdentifiers -framework UserNotifications \
        -o "$build/obj/Margin-$arch"
    apps="$apps $build/obj/Margin-$arch"
    clis="$clis $lib/margin"
done
lipo -create $apps -output "$app/Contents/MacOS/Margin"
# The CLI agents use; `margin open` hands documents to this app.
lipo -create $clis -output "$app/Contents/Helpers/margin"

# The icon, compiled from the asset catalog.
xcrun actool Resources/Assets.xcassets --compile "$app/Contents/Resources" \
    --platform macosx --minimum-deployment-target "$min" --app-icon AppIcon \
    --output-partial-info-plist "$build/obj/assets.plist" > /dev/null
cp Resources/Info.plist "$app/Contents/Info.plist"

if [ "$profile" = release ]; then
    dsymutil "$app/Contents/MacOS/Margin" -o "$build/Margin.app.dSYM"
fi
# Ad hoc: fine for this Mac. Distribution needs a Developer ID signature
# and notarization.
codesign --force --options runtime --sign - "$app/Contents/Helpers/margin"
codesign --force --options runtime --sign - "$app"
echo "Built $app ($archs)"
