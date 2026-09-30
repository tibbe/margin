import AppKit
import XCTest

@testable import MarginKit

/// An agent changes the file while the document is open. Theirs alone
/// loads; edits on other lines merge; overlapping edits ask, and nothing is
/// written until the person answers.
final class OutsideEditTests: XCTestCase {
    @MainActor
    func testTheirsAloneLoadsAndOtherLinesMerge() throws {
        let h = try Harness("One.\n\nTwo.\n\nThree.\n")
        defer { h.close() }
        h.external("One.\n\nTwo, by an agent.\n\nThree.\n")
        h.wait(forBanner: "Updated from disk")
        XCTAssertEqual(h.text, "One.\n\nTwo, by an agent.\n\nThree.\n")

        // Ours on one line, theirs on another: merged, then saved.
        h.find("One")
        h.type(" more")
        h.external("One.\n\nTwo, by an agent.\n\nThree, again.\n")
        h.wait(forBanner: "Merged changes from disk")
        h.save()
        XCTAssertEqual(h.fileText, "One more.\n\nTwo, by an agent.\n\nThree, again.\n")
    }

    @MainActor
    func testOverlappingEditsAsk() throws {
        let h = try Harness("One more.\n\nTwo, by an agent.\n\nThree, again.\n")
        defer { h.close() }

        // Both on the same line: a conflict, and saving waits for the answer.
        h.find("Three, again")
        h.type("!")
        h.external("One more.\n\nTwo, by an agent.\n\nThree, again?\n")
        h.wait(until: { h.sheet != nil }, "the conflict")
        XCTAssertEqual(h.sheet, ["Keep My Version", "Load Disk Version"])
        h.save()
        XCTAssertEqual(h.fileText, "One more.\n\nTwo, by an agent.\n\nThree, again?\n")
        XCTAssertEqual(h.title, "doc.md | subtitle | edited false")
        try h.press(sheetButton: "Keep My Version")
        h.wait(0.1)
        XCTAssertEqual(h.fileText, "One more.\n\nTwo, by an agent.\n\nThree, again!.\n")

        // Again, loading theirs this time.
        h.find("Two, by an agent")
        h.type("!")
        h.external("One more.\n\nTwo, by an agent?\n\nThree, again!\n")
        h.wait(until: { h.sheet != nil }, "the conflict")
        try h.press(sheetButton: "Load Disk Version")
        h.wait(0.1)
        XCTAssertEqual(h.text, "One more.\n\nTwo, by an agent?\n\nThree, again!\n")
        XCTAssertEqual(h.fileText, "One more.\n\nTwo, by an agent?\n\nThree, again!\n")
    }

    /// A conflict while another sheet shows is asked once that sheet is gone.
    @MainActor
    func testAConflictWaitsForAnotherSheet() throws {
        let h = try Harness("One more.\n\nTwo, by an agent?\n\nThree, again!\n")
        defer { h.close() }
        h.find("One more")
        h.type("!")
        h.action(#selector(DocumentWindow.marginRename(_:)))
        h.external("One more?\n\nTwo, by an agent?\n\nThree, again!\n")
        h.wait(0.4)
        XCTAssertEqual(h.sheet, ["Cancel", "Rename"])
        try h.press(sheetButton: "Cancel")
        h.wait(until: { h.sheet == ["Keep My Version", "Load Disk Version"] }, "the conflict")
        try h.press(sheetButton: "Keep My Version")
        h.wait(0.1)
        XCTAssertEqual(h.fileText, "One more!.\n\nTwo, by an agent?\n\nThree, again!\n")
    }
}
