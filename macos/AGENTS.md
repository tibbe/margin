# macOS editor

- Build: `macos/build.sh` (release) or `macos/build.sh debug` (with the
  script driver, for tests). Either writes `macos/build/Margin.app`.
  Warnings fail the build.
- Swift 6, on the main actor unless declared otherwise. The core's
  bindings are the module `margin_ffi`; files that use them import it.
- Format: `swift format -i -p -r macos/Sources macos/tools`; check with
  `swift format lint --strict -p -r macos/Sources macos/tools`. Name those
  directories: `macos/build/gen` holds the generated bindings.
- Test: `macos/tests/run.sh` runs the UI scripts in `macos/tests/` against a
  debug build; add a script there for new behavior.
- Run: `open -g -n -a "$PWD/macos/build/Margin.app" FILE…` from the
  repository root. `-g` keeps it in the background, so it doesn't take the
  keystrokes of a user typing elsewhere. Other worktrees may have builds of
  their own, and `margin FILE…` or `open -b` launch whichever build macOS
  picks by bundle ID. Quit only your
  own build (`pkill -f "$PWD/macos/build/Margin.app/"`), not by bundle ID,
  which would quit the user's other copies. All builds share settings and
  comments.
