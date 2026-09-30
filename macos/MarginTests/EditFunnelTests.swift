import AppKit
import XCTest

@testable import MarginKit

/// Edits AppKit makes itself go through the editing rules too.
final class EditFunnelTests: XCTestCase {
    @MainActor
    func testAppKitsOwnEditsFollowTheRules() throws {
        let h = try Harness()
        defer { h.close() }

        h.reset("x **bold**\n")
        h.find("bold**")
        h.key("opt-backspace")
        XCTAssertEqual(h.text, "x \n")

        h.reset("ab cd\n")
        h.find("ab")
        h.key("ctrl-t")
        XCTAssertEqual(h.text, "a bcd\n")

        // What the Spelling panel's Change button sends.
        h.reset("some **teh** text\n")
        h.select("teh")
        let change = NSMatrix(
            frame: .zero, mode: .radioModeMatrix, cellClass: NSCell.self, numberOfRows: 1, numberOfColumns: 1)
        change.cells.first?.stringValue = "the"
        change.selectCell(atRow: 0, column: 0)
        NSApp.sendAction(#selector(NSTextView.changeSpelling(_:)), to: h.view, from: change)
        XCTAssertEqual(h.text, "some **the** text\n")

        h.reset("one two three four\n")
        h.select(["two", "four"])
        XCTAssertEqual(h.selection, "4 3")
        h.type("X")
        XCTAssertEqual(h.text, "one X three four\n")
    }
}
