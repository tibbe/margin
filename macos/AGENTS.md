# macOS editor

See `docs/macos/architecture.md`. Run these from the repository root.

Several worktrees may run their builds at once, under one bundle ID, so
address the app by its path, as below.

- Run: `open -g -n -a "$PWD/macos/build/DerivedData/Build/Products/Debug/Margin.app" FILE…`
- Quit: `pkill -f "$PWD/macos/build/DerivedData/Build/Products/Debug/Margin.app/"`

## Checks

- `swift format -i -p -r macos/Sources macos/App macos/MarginTests macos/MarginUITests macos/tools`
- `swift format lint --strict -p -r macos/Sources macos/App macos/MarginTests macos/MarginUITests macos/tools`
- `xcodebuild -project macos/Margin.xcodeproj -scheme Margin -derivedDataPath macos/build/DerivedData test -only-testing:MarginTests SWIFT_TREAT_WARNINGS_AS_ERRORS=YES`
  (builds the app too; the unit tests don't launch it)
