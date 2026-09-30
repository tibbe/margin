import AppKit
import XCTest

@testable import MarginKit

/// Comment cards: where they appear, clicks on them, long messages.
final class CardTests: XCTestCase {
    @MainActor
    private func comment(_ h: Harness, on text: String, _ body: String) {
        h.select(text)
        h.key("cmd-opt-m")
        h.compose(body)
        h.key("cmd-enter")
        h.wait(0.2)
    }

    /// A new comment's card is beside its text as soon as it is posted,
    /// never first at the top of the gutter.
    @MainActor
    func testANewCardIsBesideItsText() throws {
        let h = try Harness("First line.\n\nSecond line.\n\nThird line to comment on.\n")
        defer { h.close() }
        h.size(1300, 700)
        h.select("comment")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        XCTAssertEqual(h.cardOffsets, [1: 0])
    }

    /// A click on a card's message text focuses the thread, as a click on
    /// the card's padding does; a drag across it selects the text instead.
    @MainActor
    func testClickingAndDraggingMessageText() throws {
        let h = try Harness("First line.\n")
        defer { h.close() }
        comment(h, on: "First", "Why is this here?")
        h.key("escape")
        h.wait(0.1)
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["Why is this here?"]"# + "\nactive none")
        try h.click(cardText: "Why", onCard: 1)
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["Why is this here?"]"# + "\nactive 1")
        XCTAssertEqual(h.focus, "ComposerTextView")
        h.key("escape")
        h.wait(0.1)
        try h.click(cardText: "Why", onCard: 1, drag: true)
        XCTAssertEqual(h.focus, "NSTextView")
    }

    /// A message longer than seven lines is cut off at three, with Show
    /// More; Show Less cuts it off again. Seven lines are shown in full.
    @MainActor
    func testLongMessagesAreCutOff() throws {
        let h = try Harness("First line.\n")
        defer { h.close() }
        comment(h, on: "First", "1\n2\n3\n4\n5\n6\n7\n8")
        h.key("escape")
        h.wait(0.1)
        XCTAssertEqual(try h.messageStates(onCard: 1), ["collapsed"])

        // Show More focuses the thread, as any click on the card does.
        try h.click(button: "Show More", onCard: 1)
        h.wait(0.2)
        XCTAssertEqual(try h.messageStates(onCard: 1), ["expanded"])
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["1\n2\n3\n4\n5\n6\n7\n8"]"# + "\nactive 1")

        // Rebuilding the card (here, for a reply) keeps the message shown in
        // full.
        h.compose("1\n2\n3\n4\n5\n6\n7")
        h.key("cmd-enter")
        h.wait(0.2)
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["1\n2\n3\n4\n5\n6\n7\n8", "1\n2\n3\n4\n5\n6\n7"]"# + "\nactive 1")
        XCTAssertEqual(try h.messageStates(onCard: 1), ["expanded", "short"])
        try h.click(button: "Show Less", onCard: 1)
        h.wait(0.2)
        XCTAssertEqual(try h.messageStates(onCard: 1), ["collapsed", "short"])
    }

    /// The text's right-click menu leads with Comment on Selection, which
    /// starts a draft on the selection, even on a line that has a thread.
    @MainActor
    func testCommentingFromTheContextMenu() throws {
        let h = try Harness("First line.\nSecond line.\n")
        defer { h.close() }
        comment(h, on: "First", "Why?")
        h.select("First")
        XCTAssertEqual(h.contextMenu(choose: "Comment on Selection"), ["Comment on Selection", "—"])
        h.wait(0.1)
        XCTAssertEqual(h.focus, "ComposerTextView")
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["Why?"]"# + "\ndraft [0,5)\nactive none")
    }

    /// Clicking empty gutter space leaves the focused thread, and the
    /// keyboard goes back to the text; so does Cancel in the reply box.
    @MainActor
    func testLeavingAThread() throws {
        let h = try Harness("First line.\nSecond line.\n")
        defer { h.close() }
        comment(h, on: "First", "Why?")
        h.key("escape")
        h.size(1200, 900)
        try h.click(card: 1)
        h.wait(0.2)
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["Why?"]"# + "\nactive 1")
        h.click(x: 1100, y: 700)
        h.wait(0.1)
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["Why?"]"# + "\nactive none")
        XCTAssertEqual(h.focus, "DocTextView")

        try h.click(card: 1)
        h.wait(0.2)
        XCTAssertEqual(h.focus, "ComposerTextView")
        try h.click(button: "Cancel", onCard: 1)
        h.wait(0.2)
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["Why?"]"# + "\nactive none")
        XCTAssertEqual(h.focus, "DocTextView")
        XCTAssertNil(try h.button("Cancel", onCard: 1))
    }
}
