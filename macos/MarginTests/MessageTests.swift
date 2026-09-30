import AppKit
import XCTest

@testable import MarginKit

/// Each message's menu edits or deletes it, deleting the comment deletes the
/// thread, and undo puts things back.
final class MessageTests: XCTestCase {
    @MainActor
    private func thread(_ h: Harness) {
        h.select("First")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        for reply in ["Because.", "Fixed."] {
            h.action(#selector(DocumentWindow.marginReply(_:)))
            h.wait(0.1)
            h.compose(reply)
            h.key("cmd-enter")
            h.wait(0.1)
        }
    }

    private let all = #"#1 open [0,5) ["Why?", "Because.", "Fixed."]"#
    private let edited = #"#1 open [0,5) ["Why not?", "Because.", "Fixed."]"#

    /// The buttons show on hover, or while the card is focused.
    @MainActor
    func testButtonsShowOnHoverOrFocus() throws {
        let h = try Harness("First line.\nSecond line.\n")
        defer { h.close() }
        thread(h)
        XCTAssertEqual(h.comments, all + "\nactive 1")
        h.find("Second")
        h.wait(0.1)
        XCTAssertTrue(try h.hover(card: 1))
        XCTAssertFalse(try h.hover(card: 1, on: false))
        try h.click(card: 1)
        h.wait(0.1)
        XCTAssertTrue(try h.hover(card: 1, on: false))
    }

    /// Edit the comment on the card: Escape cancels, Save keeps, and an
    /// empty edit can't be saved.
    @MainActor
    func testEditingAMessage() throws {
        let h = try Harness("First line.\nSecond line.\n")
        defer { h.close() }
        thread(h)
        h.find("Second")
        h.wait(0.1)
        try h.click(card: 1)
        h.wait(0.1)
        XCTAssertEqual(try h.menu(ofMessage: 0, onCard: 1), ["Edit", "Delete"])
        XCTAssertEqual(try h.menu(ofMessage: 0, onCard: 1, context: true), ["Reply", "Resolve", "—", "Edit", "Delete"])

        try h.menu(ofMessage: 0, onCard: 1, choose: "Edit")
        h.wait(0.1)
        XCTAssertEqual(h.focus, "ComposerTextView")
        h.key("cmd-a")
        h.compose("Why not?")
        h.key("escape")
        h.wait(0.1)
        XCTAssertEqual(h.comments, all + "\nactive none")

        try h.menu(ofMessage: 0, onCard: 1, context: true, choose: "Edit")
        h.wait(0.1)
        h.key("cmd-a")
        h.compose("Why not?")
        h.key("cmd-enter")
        h.wait(0.1)
        XCTAssertEqual(h.comments, edited + "\nactive none")
        XCTAssertEqual(h.undoName, "Undo Edit")
        h.key("cmd-z")
        h.wait(0.1)
        XCTAssertEqual(h.comments, all + "\nactive 1")
        h.key("cmd-shift-z")
        h.wait(0.1)
        XCTAssertEqual(h.comments, edited + "\nactive 1")

        try h.menu(ofMessage: 0, onCard: 1, choose: "Edit")
        h.wait(0.1)
        h.key("cmd-a")
        h.key("backspace")
        h.key("cmd-enter")
        h.wait(0.1)
        h.key("escape")
        h.wait(0.1)
        XCTAssertEqual(h.comments, edited + "\nactive none")
    }

    /// The Comments menu acts on the focused thread. Deleting a reply, then
    /// the comment (the thread); undo brings each back.
    @MainActor
    func testDeletingMessages() throws {
        let h = try Harness("First line.\nSecond line.\n")
        defer { h.close() }
        thread(h)
        h.find("First")
        h.wait(0.1)
        XCTAssertEqual(h.menu("Comments > Resolve"), "enabled")
        XCTAssertEqual(h.menu("Comments > Edit"), "enabled")
        XCTAssertEqual(h.menu("Comments > Delete"), "enabled")

        try h.menu(ofMessage: 1, onCard: 1, context: true, choose: "Delete")
        h.wait(0.1)
        XCTAssertEqual(h.comments, #"#1 open [0,5) ["Why?", "Fixed."]"# + "\nactive 1")
        XCTAssertEqual(h.banner, "Reply deleted")
        XCTAssertEqual(h.undoName, "Undo Delete Reply")
        h.key("cmd-z")
        h.wait(0.1)
        XCTAssertEqual(h.comments, all + "\nactive 1")

        try h.menu(ofMessage: 0, onCard: 1, choose: "Delete")
        h.wait(0.1)
        XCTAssertEqual(h.comments, "active none")
        XCTAssertEqual(h.banner, "Comment deleted")
        XCTAssertEqual(h.undoName, "Undo Delete Comment")
        h.key("cmd-z")
        h.wait(0.1)
        XCTAssertEqual(h.comments, all + "\nactive 1")
    }
}
