import AppKit
import XCTest

@testable import MarginKit

/// Documents: outside changes, comments' undo, Rename, Duplicate, Revert,
/// checkboxes and Replace All.
final class DocumentTests: XCTestCase {
    @MainActor
    func testAnOutsideChangeIsUndoable() throws {
        let h = try Harness("First line.\n")
        defer { h.close() }
        XCTAssertEqual(h.title, "doc.md | subtitle | edited false")
        h.external("First line.\nAdded by an agent.\n")
        h.wait(forBanner: "Updated from disk")
        XCTAssertEqual(h.text, "First line.\nAdded by an agent.\n")
        h.key("cmd-z")
        h.wait(0.1)
        XCTAssertEqual(h.text, "First line.\n")
        h.key("cmd-shift-z")
        h.wait(0.1)
        XCTAssertEqual(h.text, "First line.\nAdded by an agent.\n")
    }

    @MainActor
    func testResolveAllIsUndoable() throws {
        let h = try Harness("First line.\n")
        defer { h.close() }
        h.select("First")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        h.action(#selector(DocumentWindow.marginResolveAll(_:)))
        XCTAssertEqual(h.comments, #"#1 resolved [0,5) ["Why?"]"# + "\nactive none")
        h.key("cmd-z")
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["Why?"]"# + "\nactive none")
        h.key("cmd-shift-z")
        XCTAssertEqual(h.comments, #"#1 resolved [0,5) ["Why?"]"# + "\nactive none")
    }

    @MainActor
    func testRenameDuplicateAndRevert() throws {
        let h = try Harness("")
        defer { h.close() }
        h.reset("First line.\n")
        h.select("First")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        try h.doc.relocate(to: h.dir.appendingPathComponent("renamed.md").path, moving: true)
        XCTAssertEqual((h.path as NSString).lastPathComponent, "renamed.md")
        XCTAssertTrue(FileManager.default.fileExists(atPath: h.path))
        XCTAssertEqual(h.storedThreads, 1)
        h.action(#selector(DocumentWindow.marginDuplicate(_:)))
        h.wait(0.3)
        XCTAssertEqual(h.windows, ["renamed.md", "Untitled"])
        h.action(#selector(DocumentWindow.marginRevertToLastOpened(_:)))
        XCTAssertEqual(h.text, "")
    }

    @MainActor
    func testCheckboxesAndReplaceAll() throws {
        let h = try Harness()
        defer { h.close() }
        h.reset("- [ ] task\n")
        h.view.display()
        let task = try XCTUnwrap(h.view.items.first { $0.task != nil })
        let box = h.view.checkboxBox(task)
        h.click(h.view, at: NSPoint(x: box.midX, y: box.midY))
        XCTAssertEqual(h.text, "- [x] task\n")

        h.reset("a **bold** b bold\n")
        h.doc.findBar.open(showingReplaceField: true)
        h.doc.findBar.search.stringValue = "bold"
        h.doc.findBar.replaceField.stringValue = "strong"
        h.doc.findBar.refresh(goingToMatchAtOrAfterCursor: true)
        h.doc.findBar.replaceAll()
        XCTAssertEqual(h.text, "a **strong** b strong\n")
        h.key("cmd-z")
        XCTAssertEqual(h.text, "a **bold** b bold\n")
    }
}
