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

    /// Typed edits move a thread as the spec's table has it, and the CLI
    /// sees the same once the document is saved.
    @MainActor
    func testTypingFollowsTheTable() throws {
        /// The text after `edit`, with the thread's text in brackets, or
        /// where it is detached.
        func anchored(_ edit: (Harness) -> Void) throws -> String {
            let h = try Harness("the quick fox jumps\n")
            defer { h.close() }
            h.select("quick fox")
            h.key("cmd-opt-m")
            h.compose("Why?")
            h.key("cmd-enter")
            h.wait(0.1)
            edit(h)
            h.save()
            h.wait(0.1)
            let text = h.view.string as NSString
            let item = try XCTUnwrap(h.item(1))
            let listing = h.margin("comments", "doc.md").split(separator: "\n").map(String.init)
            let status = item.range == nil ? "open, detached: the commented text was deleted" : "open"
            XCTAssertEqual(listing.dropFirst().first, "#1 doc.md:1:\(item.start + 1) (\(status))")
            guard let r = item.range else {
                return "\(text.trimmingCharacters(in: .newlines)) (detached at \(item.start), \(listing[2]))"
            }
            return text.replacingCharacters(in: r, with: "[\(text.substring(with: r))]")
                .trimmingCharacters(in: .newlines)
        }
        XCTAssertEqual(
            try anchored {
                $0.findBefore("the")
                $0.type("then ")
            }, "then the [quick fox] jumps")
        XCTAssertEqual(
            try anchored {
                $0.find("quick")
                $0.type(" brown")
            }, "the [quick brown fox] jumps")
        XCTAssertEqual(
            try anchored {
                $0.findBefore("quick")
                $0.type("very ")
            }, "the very [quick fox] jumps")
        XCTAssertEqual(
            try anchored {
                $0.find("fox")
                $0.type(" now")
            }, "the [quick fox] now jumps")
        XCTAssertEqual(
            try anchored {
                $0.select(" fox jumps")
                $0.type(".")
            }, "the [quick].")
        XCTAssertEqual(
            try anchored {
                $0.select("quick fox ")
                $0.key("backspace")
            }, #"the jumps (detached at 4,   on "quick fox")"#)
        XCTAssertEqual(
            try anchored {
                $0.select("quick fox")
                $0.type("lazy dog")
            }, #"the lazy dog jumps (detached at 12,   on "quick fox")"#)
        XCTAssertEqual(
            try anchored {
                $0.select("quick")
                $0.type("slow")
            }, "the slow[ fox] jumps")
        XCTAssertEqual(
            try anchored {
                $0.select("quick fox")
                $0.key("cmd-b")
            }, "the **[quick fox]** jumps")
    }

    /// An agent rewrites the commented text: the thread detaches where it
    /// was, keeping its quote, the same as the CLI has it.
    @MainActor
    func testRewrittenTextDetachesItsThread() throws {
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
        XCTAssertEqual(h.comments, #"#1 open detached [25,25) ["Clearer?"]"# + "\nactive none")
        XCTAssertEqual(
            h.margin("comments", "doc.md").split(separator: "\n").prefix(3).joined(separator: "\n"),
            """
            doc.md: 1 open thread
            #1 doc.md:1:26 (open, detached: the commented text was deleted)
              on "insensitive"
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
