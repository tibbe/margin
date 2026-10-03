# macOS architecture

The macOS editor's architecturally significant decisions: the ones costly to
change later, e.g. its language and frameworks. Those every platform shares
are in `docs/architecture.md`.

- The app is an Xcode project.
- The UI is built with AppKit.
- The document is a view of Margin's own, not an `NSTextView`. Its model
  is the file's text; it lays out the core's shown paragraphs with Core
  Text, draws them, takes input as an `NSTextInputClient`, and edits
  only through the core. `NSTextView`, on either TextKit, takes paragraphs
  from its stored text's newlines, so a reflowed paragraph, joined from
  several lines of the file, would be two models of the document to keep
  in agreement; TextKit 2 can't join them at all. Holding the projection
  in the text view's storage instead has no precedent among editors that
  keep files byte for byte. The view's own costs: no Writing Tools, no
  dragging text, and accessibility only as plain text.
- Visual components (code boxes, tables, images, diagrams) are drawn by
  that view, in space its layout leaves for them.
- Tests use `XCTest` for unit tests and `XCUITest` for UI tests.
- Distributed using Homebrew.
