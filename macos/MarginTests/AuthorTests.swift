import AppKit
import XCTest

@testable import MarginKit

/// Messages from the editor are the writer's ("You"), messages from the CLI
/// an agent's, marked with a symbol; the CLI prints them as user and agent.
final class AuthorTests: XCTestCase {
    @MainActor
    func testMessagesShowWhoWroteThem() throws {
        let h = try Harness("One two three.\n")
        defer { h.close() }
        h.select("two")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        XCTAssertEqual(h.margin("reply", "doc.md", "1", "Because."), "Replied to #1.\n")
        h.wait(forBanner: "1 new reply")
        h.action(#selector(DocumentWindow.marginReply(_:)))
        h.wait(0.1)
        h.compose("Fine.")
        h.key("cmd-enter")
        h.wait(0.1)
        XCTAssertEqual(try h.authors(onCard: 1), ["You", "✦ Agent", "You"])

        XCTAssertEqual(h.margin("add", "doc.md", "--quote", "three", "Plural?"), "Added #2 at doc.md:1:9.\n")
        h.wait(forBanner: "new comment")
        XCTAssertEqual(try h.authors(onCard: 2), ["✦ Agent"])

        XCTAssertEqual(h.sh("\(Harness.cli) thread doc.md 1 | grep ' · ' | cut -d' ' -f3"), "user\nagent\nuser\n")
    }

    /// The buttons' targets reach into the padding beside their symbols.
    @MainActor
    func testButtonsReachIntoThePadding() throws {
        let h = try Harness("One two three.\n")
        defer { h.close() }
        h.select("two")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        let card = try h.card(1)
        func more(_ v: NSView) -> NSButton? {
            if let b = v as? NSButton, b.toolTip == "More" { return b }
            for s in v.subviews { if let f = more(s) { return f } }
            return nil
        }
        let b = try XCTUnwrap(more(card))
        let frame = try XCTUnwrap(h.win.contentView?.superview)
        let p = b.convert(NSPoint(x: b.bounds.maxX - 1, y: b.bounds.midY), to: nil)
        XCTAssertFalse(card.stack.bounds.contains(card.stack.convert(p, from: nil)), "not in the padding")
        XCTAssertTrue(frame.hitTest(frame.convert(p, from: nil)) === b)
    }
}
