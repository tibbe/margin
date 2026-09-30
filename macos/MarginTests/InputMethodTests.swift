import AppKit
import XCTest

@testable import MarginKit

/// Input method calls (NSTextInputClient), as Japanese or Chinese input
/// methods make them, and press-and-hold accents.
final class InputMethodTests: XCTestCase {
    @MainActor
    private func mark(_ h: Harness, _ text: String) {
        h.view.setMarkedText(
            text, selectedRange: NSRange(location: (text as NSString).length, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0))
    }

    @MainActor
    private func commit(_ h: Harness, _ text: String) {
        h.view.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
    }

    @MainActor
    func testComposingText() throws {
        let h = try Harness()
        defer { h.close() }

        h.reset("# Title\n")
        h.findBefore("Title")
        mark(h, "に")
        mark(h, "にほん")
        commit(h, "日本")
        XCTAssertEqual(h.text, "# 日本Title\n")
        h.key("cmd-z")
        XCTAssertEqual(h.text, "# Title\n")

        h.reset("[docs](u) z\n")
        h.find("docs")
        mark(h, "ご")
        commit(h, "語")
        XCTAssertEqual(h.text, "[docs](u)語 z\n")

        h.reset("a **bold** c\n")
        h.find("bold")
        mark(h, "に")
        commit(h, "日")
        XCTAssertEqual(h.text, "a **bold日** c\n")

        h.reset("a **word** b\n")
        h.select("word")
        mark(h, "に")
        commit(h, "日")
        XCTAssertEqual(h.text, "a **日** b\n")

        h.reset("keep\n")
        h.find("keep")
        mark(h, "に")
        mark(h, "")
        commit(h, "")
        XCTAssertEqual(h.text, "keep\n")
    }

    @MainActor
    func testPressAndHoldReplacesTheLetterTyped() throws {
        let h = try Harness()
        defer { h.close() }
        h.reset("abc\n")
        h.find("abc")
        h.type("e")
        let c = h.view.selectedRange().location
        h.view.insertText("é", replacementRange: NSRange(location: max(0, c - 1), length: min(1, c)))
        XCTAssertEqual(h.text, "abcé\n")
    }
}
