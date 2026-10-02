import AppKit
import XCTest

@testable import MarginKit

/// A thread whose text is deleted keeps its quote, through Delete and Undo;
/// a draft whose text is gone cannot post a thread on nothing.
final class AnchorTests: XCTestCase {
    @MainActor
    func testDeletedTextKeepsItsQuote() throws {
        let h = try Harness("Keep this. Gone text here.\n")
        defer { h.close() }
        h.select("Gone text")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        XCTAssertEqual(h.comments, #"#1 open [11,20) ["Why?"]"# + "\nactive 1")

        // Deleting the commented text detaches the thread.
        h.select("Gone text")
        h.key("backspace")
        h.save()
        h.wait(0.1)
        XCTAssertEqual(h.comments, #"#1 open detached [11,11) ["Why?"]"# + "\nactive none")
        let listing = """
            doc.md: 1 open thread

            #1 doc.md:1:12 (open, detached: the commented text was deleted)
              on "Gone text"
            """
        XCTAssertEqual(
            h.margin("comments", "doc.md").split(separator: "\n", omittingEmptySubsequences: false).prefix(4).joined(
                separator: "\n"), listing)

        // Deleting the thread and undoing it brings it back detached, with
        // its quote.
        try h.click(card: 1)
        h.wait(0.1)
        h.action(#selector(DocumentWindow.marginDeleteComment(_:)))
        h.wait(0.1)
        XCTAssertEqual(h.comments, "active none")
        h.key("cmd-z")
        h.wait(0.1)
        XCTAssertEqual(h.comments, #"#1 open detached [11,11) ["Why?"]"# + "\nactive 1")
        XCTAssertEqual(
            h.margin("comments", "doc.md").split(separator: "\n", omittingEmptySubsequences: false).prefix(4).joined(
                separator: "\n"), listing)
    }

    /// An agent rewrites the commented text: the thread moves to what
    /// replaced it, the same as the CLI has it.
    @MainActor
    func testRewrittenTextKeepsItsThread() throws {
        let h = try Harness("Send to Agent is insensitive unless an agent is waiting.\n")
        defer { h.close() }
        h.select("insensitive")
        h.key("cmd-opt-m")
        h.compose("Clearer?")
        h.key("cmd-enter")
        h.wait(0.1)
        h.external("Send to Agent is disabled while it can't send.\n")
        h.wait(forText: "disabled")
        h.wait(0.1)
        XCTAssertEqual(h.comments, #"#1 open [17,25) ["Clearer?"]"# + "\nactive 1")
        XCTAssertEqual(
            h.margin("comments", "doc.md").split(separator: "\n").prefix(3).joined(separator: "\n"),
            """
            doc.md: 1 open thread
            #1 doc.md:1:18 (open)
              on "disabled"
            """)
    }

    /// Deleting the commented text shows the card as detached at once,
    /// before the document is saved, as the highlight goes at once.
    @MainActor
    func testACardShowsItsDeletedTextAtOnce() throws {
        let h = try Harness("Keep this. Gone text here.\n")
        defer { h.close() }
        h.select("Gone text")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        XCTAssertEqual(try h.deletedQuote(onCard: 1), nil)

        h.select("Gone text")
        h.key("backspace")
        XCTAssertEqual(h.comments, #"#1 open detached [11,11) ["Why?"]"# + "\nactive none")
        XCTAssertEqual(try h.deletedQuote(onCard: 1), "“Gone text”")
    }

    @MainActor
    func testADraftOnDeletedTextPostsNothing() throws {
        let h = try Harness("Keep this. Gone text here.\n")
        defer { h.close() }
        h.select("Gone text")
        h.key("cmd-opt-m")
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)

        // An agent deletes the draft's text; the draft stays, and posts
        // nothing.
        h.key("escape")
        h.select("this")
        h.key("cmd-opt-m")
        h.external("Keep here.\n")
        h.wait(forText: "Keep here.")
        let expected = #"#1 open detached [5,5) ["Why?"]"# + "\ndraft [5,5)\nactive none"
        XCTAssertEqual(h.comments, expected)
        h.compose("Why?")
        h.key("cmd-enter")
        h.wait(0.1)
        XCTAssertEqual(h.banner, "The text you were commenting on was deleted")
        XCTAssertEqual(h.comments, expected)
        XCTAssertEqual(h.storedThreads, 1)
    }
}
