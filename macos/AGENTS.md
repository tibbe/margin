# macOS editor

- Build: `macos/build.sh` (release) or `macos/build.sh debug` (with the
  script driver, for tests). Either writes `macos/build/Margin.app`.
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
