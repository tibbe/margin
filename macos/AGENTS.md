# macOS editor

Run these from the repository root.

- Build: `macos/build.sh debug` (for tests) or `macos/build.sh` (release).
- Run: `open -g -n -a "$PWD/macos/build/Margin.app" FILE…`
- Quit: `pkill -f "$PWD/macos/build/Margin.app/"`
- Test: add a UI script to `macos/tests/` for new behavior. `caret` and
  `links` fail unless Margin is the active app.

## Checks

- `swift format -i -p -r macos/Sources macos/tools`
- `swift format lint --strict -p -r macos/Sources macos/tools`
- `macos/build.sh debug`
- `macos/tests/run.sh`
