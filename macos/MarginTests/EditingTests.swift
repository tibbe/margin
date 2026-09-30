import AppKit
import XCTest

@testable import MarginKit

/// Typing through real key events. Each result must match the core's
/// keystroke tests in crates/core/src/md/edit.rs.
final class EditingTests: XCTestCase {
    @MainActor
    func testTyping() throws {
        let h = try Harness()
        defer { h.close() }

        h.reset("")
        h.type("# New section\nBody")
        XCTAssertEqual(h.text, "# New section\n\nBody")

        h.reset("")
        h.type("- first\nsecond")
        h.key("tab")
        h.type("\nnested\n\n\nAfter")
        XCTAssertEqual(h.text, "- first\n  - second\n  - nested\n\nAfter")

        h.reset("x")
        h.type(" **bold**")
        h.key("backspace", "backspace", "backspace", "backspace")
        XCTAssertEqual(h.text, "x ")

        h.reset("a word here")
        h.find("wor")
        h.key("cmd-b")
        XCTAssertEqual(h.text, "a **word** here")

        h.reset("")
        h.type("a")
        h.key("shift-enter")
        h.type("b")
        XCTAssertEqual(h.text, "a\\\nb")

        h.reset("")
        h.type("```rust\nlet x = 1;")
        XCTAssertEqual(h.text, "```rust\nlet x = 1;\n```")

        h.reset("")
        h.type("> q\nr")
        XCTAssertEqual(h.text, "> q\n>\n> r")

        h.reset("")
        h.type("1. one\ntwo\n\nafter")
        XCTAssertEqual(h.text, "1. one\n2. two\n\nafter")
    }

    @MainActor
    func testEditingExistingText() throws {
        let h = try Harness()
        defer { h.close() }

        h.reset("one two\n")
        h.find("one")
        h.key("enter", "backspace")
        XCTAssertEqual(h.text, "onetwo\n")

        h.reset("- [ ] buy milk")
        h.type("\neggs")
        XCTAssertEqual(h.text, "- [ ] buy milk\n- [ ] eggs")

        h.reset("1. a\n2. b\n3. c\n")
        h.find("1. a")
        h.key("enter")
        XCTAssertEqual(h.text, "1. a\n2. \n3. b\n4. c\n")

        h.reset("# Title\n")
        h.find("Title")
        h.key("left", "left", "left", "left", "left", "left")
        h.type("x")
        XCTAssertEqual(h.text, "# xTitle\n")
    }

    @MainActor
    func testTypingOverASelection() throws {
        let h = try Harness()
        defer { h.close() }

        h.reset("one two three\n")
        h.select("one")
        h.type("X")
        XCTAssertEqual(h.text, "X two three\n")

        h.reset("a **bold** c\n")
        h.select("bold")
        h.type("Z")
        XCTAssertEqual(h.text, "a **Z** c\n")

        h.reset("one two\n")
        h.select("one")
        h.key("backspace")
        XCTAssertEqual(h.text, "two\n")
    }
}
