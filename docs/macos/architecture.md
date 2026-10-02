# macOS architecture

The macOS editor's architecturally significant decisions: the ones costly to
change later, e.g. its language and frameworks. Those every platform shares
are in `docs/architecture.md`.

- The app is an Xcode project.
- The UI is built with AppKit.
- The document is one `NSTextView` on TextKit 1 (`NSLayoutManager`), whose
  glyph generation hides characters while the text storage keeps them.
  TextKit 2 can't hide characters inside a paragraph, and the positions it
  gives for text not yet laid out are estimates, which the comment cards
  and change bars can't use. Revisit if Apple deprecates `NSLayoutManager`.
- Visual components (code boxes, tables, images, diagrams) are drawn inside
  that text view, in space its layout leaves for them, not as views beside
  it.
- Tests use `XCTest` for unit tests and `XCUITest` for UI tests.
- Distributed using Homebrew.
