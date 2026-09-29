# macOS editor

- Build: `macos/build.sh` (release) or `macos/build.sh debug` (with the
  script driver, for tests). Either writes `macos/build/Margin.app`.
- Test: `macos/tests/run.sh` runs the UI scripts in `macos/tests/` against a
  debug build; add a script there for new behavior.
