# macOS editor

See `docs/macos/architecture.md`. Run these from the repository root; `$X`
is `xcodebuild -project macos/Margin.xcodeproj -scheme Margin -derivedDataPath macos/build/DerivedData`.

- Build: `$X build` (Debug) or `$X -configuration Release build`.
- Run: `open -g -n -a "$PWD/macos/build/DerivedData/Build/Products/Debug/Margin.app" FILE…`
- Quit: `pkill -f "$PWD/macos/build/DerivedData/Build/Products/Debug/Margin.app/"`
- Test: add an XCTest to `macos/MarginTests` for new behavior. Add an XCUITest
  to `macos/MarginUITests` only for what needs the real keyboard or mouse:
  UI tests take them over while they run, so say so before running them.

## Checks

- `swift format -i -p -r macos/Sources macos/App macos/MarginTests macos/MarginUITests macos/tools`
- `swift format lint --strict -p -r macos/Sources macos/App macos/MarginTests macos/MarginUITests macos/tools`
- `$X test -only-testing:MarginTests` (builds the app too; the unit tests
  don't launch it)
