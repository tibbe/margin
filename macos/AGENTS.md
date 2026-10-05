# macOS editor

See `docs/macos/architecture.md`. Run these from the repository root.

Xcode builds each checkout in a folder of its own. Several worktrees may
run their builds at once, under one bundle ID, so address the app by its
path, `$APP` below, which `macos/tools/app.sh` prints:
`APP=$(macos/tools/app.sh)`.

- Build: `xcodebuild -project macos/Margin.xcodeproj -scheme Margin build`
- Run in the background: `open -g -n -a "$APP" [FILE…]` starts another copy
  without taking focus, e.g. for automated checks. It returns before the
  window is shown.
- Quit: `pkill -f "$APP/Contents/MacOS/Margin"`
- CLI: `$APP/Contents/Helpers/margin`.

## Tests

- `MarginTests/EndToEndTests.swift` drives a document as a writer does,
  through `Support/Editor.swift`; only its "How the editor draws" section
  knows the view, so a test of what a writer sees or does goes there.
- Other tests use `Support/Harness.swift`: key presses, clicks, menus,
  outside edits (`h.external`), the CLI (`h.margin`). Open menus through
  it (`h.menu`), never by clicking: menus show on screen, though the
  windows don't.

## Release

1. Set the version in `MARKETING_VERSION` in
   `macos/Configurations/Common.xcconfig` and in `[workspace.package]` in
   `Cargo.toml`, run `cargo check` to update `Cargo.lock`, and commit.
2. Push a tag for it, such as `v0.2.0`.

`.github/workflows/release.yml` then builds `Margin.zip` with
`macos/tools/release.sh`, publishes it on GitHub Releases, and commits the
new version and checksum to `Casks/margin.rb` on `main`.

## Checks

`tools/check.sh` runs the format lint and the unit tests, for changes here
and in `crates/core` and `crates/ffi`, which the app is built on. While
working:

- Format: `swift format -i -p -r macos/Sources macos/App macos/MarginTests macos/MarginUITests macos/tools`
- Test: `macos/tools/test.sh MarginTests/CLASS[/TEST]…` builds the app and
  runs the unit tests named, printing only failures, with file and line.
  Name the tests your change bears on: with none, it runs every test, as
  `tools/check.sh` does at commit.
- See what the window draws: `h.snapshot("NAME")` in a test writes
  `macos/build/snapshots/NAME.png`.

The UI tests are in their own test plan, which `tools/check.sh` doesn't run:
`xcodebuild -project macos/Margin.xcodeproj -scheme Margin test -testPlan UI`.
They wait for a person to approve UI automation with a password, so ask
the user to run them; started by an agent, they fail with "Authentication
cancelled".
