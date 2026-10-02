# macOS editor

See `docs/macos/architecture.md`. Run these from the repository root.

Several worktrees may run their builds at once, under one bundle ID, so
address the app by its path, as below.

- Build: `xcodebuild -project macos/Margin.xcodeproj -scheme Margin -derivedDataPath macos/build/DerivedData build`
- Run in the background: `open -g -n -a "$PWD/macos/build/DerivedData/Build/Products/Debug/Margin.app" [FILE…]`
  starts another copy without taking focus, e.g. for automated checks. It
  returns before the window is shown.
- Quit: `pkill -f "$PWD/macos/build/DerivedData/Build/Products/Debug/Margin.app/Contents/MacOS/Margin"`
- CLI: `Contents/Helpers/margin` in the app.

## Release

1. Set the version in `MARKETING_VERSION` in
   `macos/Configurations/Common.xcconfig` and in `[workspace.package]` in
   `Cargo.toml`, run `cargo check` to update `Cargo.lock`, and commit.
2. Push a tag for it, such as `v0.2.0`.

`.github/workflows/release.yml` then builds `Margin.zip` with
`macos/tools/release.sh`, publishes it on GitHub Releases, and commits the
new version and checksum to `Casks/margin.rb` on `main`.

## Checks

- `swift format -i -p -r macos/Sources macos/App macos/MarginTests macos/MarginUITests macos/tools`
- `swift format lint --strict -p -r macos/Sources macos/App macos/MarginTests macos/MarginUITests macos/tools`
- `xcodebuild -project macos/Margin.xcodeproj -scheme Margin -derivedDataPath macos/build/DerivedData test -only-testing:MarginTests SWIFT_TREAT_WARNINGS_AS_ERRORS=YES`
  (builds the app too; the unit tests don't launch it)
