import AppKit
import XCTest

@testable import MarginKit

/// The page: layout, highlights, list markers and tables.
final class PageTests: XCTestCase {
    /// The text alone is centered; with cards, the text and the cards are
    /// centered together, and the cards take some of the spare width.
    /// Resolving the last comment centers the text alone again.
    @MainActor
    func testTheTextAndCardsAreCentered() throws {
        let h = try Harness("Some text to comment on.\n")
        defer { h.close() }
        h.size(1300, 700)
        XCTAssertEqual(h.page, "left 294 text 713 cards none")
        h.select("comment")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.2)
        XCTAssertEqual(h.page, "left 76 text 713 cards 396")
        h.size(1700, 700)
        XCTAssertEqual(h.page, "left 274 text 713 cards 400")
        h.size(900, 700)
        XCTAssertEqual(h.page, "left 28 text 504 cards 300")
        h.size(1300, 700)
        try h.clickResolve(card: 1)
        h.wait(0.2)
        XCTAssertEqual(h.page, "left 294 text 713 cards none")
    }

    /// Commented text is filled once throughout. Inline code keeps its own
    /// tint, and the character before its hidden closing backtick isn't
    /// filled twice, which the see-through dark highlight would show
    /// brighter.
    @MainActor
    func testHighlightsFillEachCharacterOnce() throws {
        let h = try Harness("Use `oov` now.\n")
        defer { h.close() }
        NSApp.appearance = NSAppearance(named: .darkAqua)
        h.wait(0.3)
        let once = #""oov":a " now":b"#
        h.select("oov` now")
        h.key("cmd-opt-m")
        h.wait(0.1)
        XCTAssertEqual(h.fills("oov` now"), once, "draft")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        XCTAssertEqual(h.fills("oov` now"), once, "focused")
        h.key("escape")
        h.wait(0.1)
        XCTAssertEqual(h.fills("oov` now"), once, "unfocused")
        NSApp.appearance = NSAppearance(named: .aqua)
        h.wait(0.3)
        XCTAssertEqual(h.fills("oov` now"), once, "light")
    }

    /// A list item reflowed onto one line with the line below it: the
    /// commented text after the joined break is filled once, like the text
    /// before it, also where it wraps, and code there keeps its tint.
    @MainActor
    func testHighlightsFillTextAfterAReflowedBreak() throws {
        let words = "en mon `xx` ov nou som "
        let h = try Harness(
            "- **Cause:** we see users \(words)ever\n  more `xx` rows soon \(String(repeating: words, count: 4))once.\n"
        )
        defer { h.close() }
        NSApp.appearance = NSAppearance(named: .darkAqua)
        h.view.reflowsParagraphs = true
        h.size(1240, 944)
        h.wait(0.3)
        let quote = "use:** we see users \(words)ever\n  more `xx` rows soon \(String(repeating: words, count: 4))once."
        _ = h.margin("add", h.path, "--quote", quote, "Why?")
        h.wait(0.3)
        // Runs alternate between text and code, filled `a` and `b`.
        let fills = h.fills(quote)
        let runs = fills.matches(of: /"(.*?)":([a-z])/).map { (String($0.1), String($0.2)) }
        XCTAssertEqual(runs.count, 13, fills)
        for (text, fill) in runs {
            XCTAssertEqual(fill, text == "xx" ? "b" : "a", "\(text.debugDescription) in \(fills)")
        }
    }

    /// An item starting with hidden syntax (bold) after a blank line, a
    /// heading or another item: its hidden prefix is laid out on the line
    /// before, and its marker must not follow it there.
    @MainActor
    func testMarkersAreDrawnOnTheirText() throws {
        let h = try Harness()
        defer { h.close() }
        h.reset(
            "## Menu bar\n\nThe standard menus:\n\n- **Margin**: About.\n- **File**: New.\n\n## Text input\n\n"
                + "- **No substitutions:** off.\n- plain\n1. **Numbered** bold\n- [ ] **Task** bold\n")
        h.wait(0.2)
        XCTAssertEqual(
            h.markers,
            [
                #""Margin: About.": on its text"#, #""File: New.": on its text"#,
                #""No substitutions: off.": on its text"#, #""plain": on its text"#,
                #""Numbered bold": on its text"#, #""Task bold": on its text"#,
            ])
        h.reset("> **Quoted** bold\n> more\n\n- **A**\n")
        h.wait(0.2)
        XCTAssertEqual(h.markers, [#""A": on its text"#])
    }

    /// Tables show as grids: the cursor moves through cells' text, never
    /// resting in the hidden padding and `|` between them.
    @MainActor
    func testTheCursorMovesThroughTableCells() throws {
        let h = try Harness()
        defer { h.close() }
        h.reset("| Name | Role |\n|:-----|:----:|\n| Grace Hopper |  |\n| Alan | Engineer |\n")
        // Right from the end of a cell goes to the start of the next.
        h.find("Hopper")
        h.key("right")
        XCTAssertEqual(h.selection, "49 0")
        // Into an empty cell, and typing there.
        h.type("X")
        XCTAssertEqual(h.text, "| Name | Role |\n|:-----|:----:|\n| Grace Hopper | X |\n| Alan | Engineer |\n")
        // Left from a row's first cell goes to the end of the row above.
        h.findBefore("Grace")
        h.key("left")
        XCTAssertEqual(h.selection, "13 0")
        h.key("right")
        XCTAssertEqual(h.selection, "34 0")
        // Extending a selection across cells.
        for _ in 0..<13 { h.key("shift-right") }
        XCTAssertEqual(h.selection, "34 15")
        h.key("down")
        XCTAssertEqual(h.selection, "55 0")
    }
}
