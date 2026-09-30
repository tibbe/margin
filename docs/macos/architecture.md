# macOS architecture

The macOS editor's architecturally significant decisions: the ones costly to
change later, such as its language, frameworks, and key libraries.

- The app is an Xcode project.
- The UI is built with AppKit.
- Tests use `XCTest` for unit tests and `XCUITest` for UI tests.
