import AppKit
import XCTest

@testable import MarginKit

/// The window's menus, links and agents.
final class WindowTests: XCTestCase {
    /// Menu items' enabled and checked states follow the window.
    @MainActor
    func testMenuItemsFollowTheWindow() throws {
        let h = try Harness()
        defer { h.close() }
        h.reset("Some **bold** text\n")
        XCTAssertEqual(h.menu("Show Markdown"), "enabled")
        h.key("cmd-/")
        XCTAssertEqual(h.menu("Show Markdown"), "enabled, checked")
        XCTAssertEqual(h.menu("Bold"), "disabled")
        h.key("cmd-/")
        XCTAssertEqual(h.menu("Show Markdown"), "enabled")
        XCTAssertEqual(h.menu("Show Resolved"), "enabled")
        h.action(#selector(DocumentWindow.marginToggleShowResolved(_:)))
        XCTAssertEqual(h.menu("Show Resolved"), "enabled, checked")
        XCTAssertEqual(h.menu("Reflow Paragraphs"), "enabled")
        h.key("cmd-opt-z")
        XCTAssertEqual(h.menu("Reflow Paragraphs"), "enabled, checked")
        XCTAssertEqual(h.menu("Reply"), "disabled")
    }

    /// Command-click opens Markdown files in Margin, relative to the
    /// document, including names with spaces and fragments.
    @MainActor
    func testCommandClickOpensLinkedDocuments() throws {
        let h = try Harness()
        defer { h.close() }
        h.sh(
            "printf 'Other.\\n' > other.md; printf 'Spaced.\\n' > 'my notes.md'; mkdir -p sub; printf 'Deep.\\n' > sub/deep.md"
        )
        h.reset("See [other](other.md), [spaced](my%20notes.md), [raw](<my notes.md>), [deep](sub/deep.md#top).\n")
        h.save()
        h.click(text: "other", mods: .command)
        h.wait(0.3)
        XCTAssertEqual(h.windows, ["doc.md", "other.md"])
        h.click(text: "spaced", mods: .command)
        h.wait(0.3)
        XCTAssertEqual(h.windows, ["doc.md", "other.md", "my notes.md"])
        h.click(text: "raw", mods: .command)
        h.wait(0.3)
        XCTAssertEqual(h.windows, ["doc.md", "other.md", "my notes.md"])
        h.click(text: "deep", mods: .command)
        h.wait(0.3)
        XCTAssertEqual(h.windows, ["doc.md", "other.md", "my notes.md", "deep.md"])
        // A plain click places the cursor.
        h.click(text: "other")
        h.wait(0.1)
        XCTAssertEqual(h.windows, ["doc.md", "other.md", "my notes.md", "deep.md"])
        XCTAssertEqual(h.selection, "7 0")
    }

    /// Agent activity is announced in the window, and posted as one system
    /// notification per thread change while the document isn't in front;
    /// coming back clears them.
    @MainActor
    func testAgentActivityIsAnnouncedAndNotified() throws {
        let h = try Harness("One two three.\n")
        defer { h.close() }
        for (text, body) in [("One", "Why?"), ("two", "And this?")] {
            h.select(text)
            h.key("cmd-opt-m")
            h.compose(body)
            h.key("cmd-enter")
            h.wait(0.1)
        }
        // In front: announced only.
        XCTAssertEqual(h.margin("reply", "doc.md", "1", "Because."), "Replied to #1.\n")
        h.wait(forBanner: "1 new reply")
        XCTAssertEqual(h.banner, "1 new reply")

        // In the background: each change is its own notification.
        h.looking = false
        XCTAssertEqual(h.margin("reply", "doc.md", "1", "Done.", "--resolve"), "Replied to #1 and resolved it.\n")
        h.wait(forBanner: "comment resolved")
        XCTAssertEqual(h.banner, "1 new reply, 1 comment resolved")
        XCTAssertEqual(h.margin("add", "doc.md", "--quote", "three", "Plural?"), "Added #3 at doc.md:1:9.\n")
        h.wait(forBanner: "new comment")
        XCTAssertEqual(h.margin("reopen", "doc.md", "1"), "Reopened #1.\n")
        h.wait(forBanner: "reopened")
        XCTAssertEqual(h.margin("delete", "doc.md", "2"), "Deleted #2.\n")
        h.wait(forBanner: "deleted")
        XCTAssertEqual(h.banner, "1 comment deleted")

        // Back in front.
        h.looking = true
        h.doc.windowDidBecomeKey(Notification(name: NSWindow.didBecomeKeyNotification, object: h.win))
        XCTAssertEqual(
            h.notifications.log,
            [
                "notification doc.md | Resolved “One” | Done.",
                "notification doc.md | New comment on “three” | Plural?",
                "notification doc.md | Reopened “One” | -",
                "notification doc.md | Deleted “two” | -",
                "notifications cleared for doc.md",
            ])
        XCTAssertEqual(
            h.comments,
            #"#1 open [0,3) ["Why?", "Because.", "Done."]"# + "\n" + #"#3 open [8,13) ["Plural?"]"# + "\nactive none")
    }

    /// Send to Agent reaches only an agent waiting with `margin wait`, and
    /// the toolbar follows it: waiting, then working until it waits again.
    @MainActor
    func testSendToAgent() throws {
        let h = try Harness("Retry three times.\n")
        defer { h.close() }
        func agent() -> String {
            _ = h.win.toolbar?.items
            h.doc.updateAgent()
            let count = h.doc.countLabel.stringValue
            return "\(h.doc.agentState) | send \(h.doc.sendItem?.isEnabled == true ? "enabled" : "disabled") | count "
                + (count.isEmpty ? "-" : count)
        }
        XCTAssertEqual(agent(), "none | send disabled | count -")
        h.select("three")
        h.key("cmd-opt-m")
        h.compose("Enough?")
        h.key("cmd-enter")
        h.wait(0.1)

        // No agent waiting: nothing to send to.
        XCTAssertEqual(agent(), "none | send disabled | count 1 open comment")
        XCTAssertEqual(h.menu("Comments > Send to Agent"), "disabled")
        h.key("cmd-shift-enter")
        XCTAssertNil(h.banner)

        h.sh("\(Harness.cli) wait doc.md > wait1.out 2>&1 &")
        h.wait(0.7)
        XCTAssertEqual(agent(), "waiting | send enabled | count 1 open comment")
        XCTAssertEqual(h.menu("Comments > Send to Agent"), "enabled")
        h.key("cmd-shift-enter")
        h.wait(forBanner: "Sent")
        XCTAssertEqual(h.banner, "Sent 1 open comment to the agent")
        h.wait(0.7)
        XCTAssertEqual(
            h.sh("sed -E 's|`/[^`]*/|`|g' wait1.out"),
            """
            I left comments on `doc.md`. Please address them.

            #1 `doc.md:1:7-1:11` "three"
              - User: Enough?

            Once you have answered them, run `margin wait doc.md` again for the next round.

            """)
        XCTAssertEqual(agent(), "working | send disabled | count 1 open comment · Agent working")
        XCTAssertEqual(h.margin("reply", "doc.md", "1", "Five, with backoff."), "Replied to #1.\n")
        h.wait(forBanner: "new reply")
        XCTAssertEqual(agent(), "working | send disabled | count 1 open comment · Agent working")

        // Waiting again ends working; a waiter that dies stops counting.
        h.sh("\(Harness.cli) wait doc.md > wait2.out 2>&1 & echo $! > wait2.pid")
        h.wait(0.7)
        XCTAssertEqual(agent(), "waiting | send enabled | count 1 open comment")
        h.sh("kill $(cat wait2.pid)")
        h.wait(0.3)
        XCTAssertEqual(agent(), "none | send disabled | count 1 open comment")
    }

    /// With nothing left open, a send still goes when the writer resolved
    /// threads since the last: it gives the agent those, and not the ones
    /// the agent resolved itself.
    @MainActor
    func testSendToAgentGivesWhatTheWriterResolved() throws {
        let h = try Harness("Retry three times.\n")
        defer { h.close() }
        func send() -> String {
            _ = h.win.toolbar?.items
            h.doc.updateAgent()
            return "send \(h.doc.sendItem?.isEnabled == true ? "enabled" : "disabled") | "
                + (h.doc.sendItem?.toolTip ?? "-")
        }
        XCTAssertEqual(h.margin("add", "doc.md", "--quote", "Retry", "Why retry?"), "Added #1 at doc.md:1:1.\n")
        XCTAssertEqual(h.margin("add", "doc.md", "--quote", "three", "Why three?"), "Added #2 at doc.md:1:7.\n")
        h.sh("\(Harness.cli) wait doc.md > wait.out 2>&1 &")
        h.wait(0.7)
        XCTAssertEqual(send(), "send enabled | Send to Agent (⇧⌘↩)")

        XCTAssertEqual(h.margin("resolve", "doc.md", "2"), "Resolved #2.\n")
        h.wait(forBanner: "resolved")
        h.action(#selector(DocumentWindow.marginResolveAll(_:)))
        XCTAssertEqual(send(), "send enabled | Send to Agent (⇧⌘↩)")
        h.key("cmd-shift-enter")
        h.wait(forBanner: "Sent")
        XCTAssertEqual(h.banner, "Sent 1 resolved comment to the agent")
        h.wait(0.7)
        XCTAssertEqual(
            h.sh("sed -E 's|`/[^`]*/|`|g' wait.out"),
            """
            I resolved comments on `doc.md`.

            #1 `doc.md:1:1-1:5` "Retry"
              - Agent: Why retry?
              - User resolved the thread.

            Nothing needs an answer. Run `margin wait doc.md` again for the next round.

            """)

        // The next send starts from this one.
        h.sh("\(Harness.cli) wait doc.md > wait2.out 2>&1 &")
        h.wait(0.7)
        XCTAssertEqual(
            send(), "send disabled | An agent is waiting, but nothing changed since the last send.")
    }

    /// A send covers every document the agent waits on: Send to Agent in
    /// one window sends the open comments on the others too, and those
    /// documents' windows show the agent working.
    @MainActor
    func testSendToAgentCoversTheRound() throws {
        let h = try Harness("Retry three times.\n")
        defer { h.close() }
        try "Use canary deploys.\n".write(
            toFile: h.dir.appendingPathComponent("spec.md").path, atomically: true, encoding: .utf8)
        let spec = try XCTUnwrap(TestApp.delegate.open(path: h.dir.appendingPathComponent("spec.md").path, show: false))
        func agent(_ d: DocumentWindow) -> String {
            d.updateAgent()
            return "\(d.agentState) | send \(d.sendItem?.isEnabled == true ? "enabled" : "disabled") | "
                + (d.sendItem?.toolTip ?? "-")
        }
        _ = h.win.toolbar?.items
        _ = spec.window?.toolbar?.items
        XCTAssertEqual(
            h.margin("add", "spec.md", "--quote", "canary", "Which regions first?"), "Added #1 at spec.md:1:5.\n")

        h.sh("\(Harness.cli) wait doc.md spec.md > wait.out 2>&1 &")
        h.wait(0.7)
        // The open thread is on spec.md, but doc.md's window can send it.
        XCTAssertEqual(
            agent(h.doc),
            "waiting | send enabled | Send This and 1 Other Document to the Agent (⇧⌘↩)")
        h.key("cmd-shift-enter")
        h.wait(forBanner: "Sent")
        XCTAssertEqual(h.banner, "Sent 1 open comment to the agent")
        h.wait(0.7)
        XCTAssertEqual(
            h.sh("sed -E 's|`/[^`]*/|`|g' wait.out"),
            """
            I left comments on `spec.md`. Please address them.

            #1 `spec.md:1:5-1:10` "canary"
              - Agent: Which regions first?

            Once you have answered them, run `margin wait doc.md spec.md` again for the next round.

            """)
        XCTAssertEqual(agent(spec).prefix(7), "working")

        // With comments on both, one send gives both, named in full.
        h.select("three")
        h.key("cmd-opt-m")
        h.compose("Enough?")
        h.key("cmd-enter")
        h.wait(0.1)
        h.sh("\(Harness.cli) wait doc.md spec.md > wait2.out 2>&1 &")
        h.wait(0.7)
        XCTAssertEqual(agent(spec).prefix(7), "waiting")
        h.key("cmd-shift-enter")
        h.wait(forBanner: "Sent 2")
        XCTAssertEqual(h.banner, "Sent 2 open comments on 2 documents to the agent")
        h.wait(0.7)
        XCTAssertEqual(
            h.sh("sed -E 's|`/[^`]*/|`|g' wait2.out"),
            """
            I left comments on `doc.md` and `spec.md`. Please address them.

            #1 `doc.md:1:7-1:11` "three"
              - User: Enough?

            #1 `spec.md:1:5-1:10` "canary"
              - Agent: Which regions first?

            Once you have answered them, run `margin wait doc.md spec.md` again for the next round.

            """)
    }

    /// `margin wait` stops once no window shows its document, since no
    /// comments can come then.
    @MainActor
    func testClosingTheDocumentEndsTheWait() throws {
        let h = try Harness("Retry three times.\n")
        defer { h.close() }
        func exited() -> Bool { h.sh("kill -0 $(cat wait.pid) 2>/dev/null || echo exited") == "exited\n" }
        h.sh("\(Harness.cli) wait doc.md > wait.out 2>&1 & echo $! > wait.pid")
        h.wait(0.7)
        XCTAssertFalse(exited())
        h.win.close()
        h.wait(until: exited, "margin wait to exit")
        XCTAssertEqual(
            h.sh("sed -E 's|`/[^`]*/|`|g' wait.out"),
            "`doc.md` is no longer open in Margin, so no comments will come. "
                + "Stop waiting: the writer will ask if they want another review.\n")
        XCTAssertEqual(
            h.sh("\(Harness.cli) wait doc.md | sed -E 's|`/[^`]*/|`|g'"),
            "`doc.md` is not open in Margin, so no comments will come. "
                + "If the writer wants to review it, run `margin open doc.md`, then `margin wait doc.md`.\n")
    }

    /// Closing the document sends no comments, but `margin wait` still gives
    /// the threads the writer resolved before closing it.
    @MainActor
    func testClosingTheDocumentGivesWhatTheWriterResolved() throws {
        let h = try Harness("Retry three times.\n")
        defer { h.close() }
        func exited() -> Bool { h.sh("kill -0 $(cat wait.pid) 2>/dev/null || echo exited") == "exited\n" }
        XCTAssertEqual(h.margin("add", "doc.md", "--quote", "Retry", "Why retry?"), "Added #1 at doc.md:1:1.\n")
        XCTAssertEqual(h.margin("add", "doc.md", "--quote", "three", "Why three?"), "Added #2 at doc.md:1:7.\n")
        h.sh("\(Harness.cli) wait doc.md > wait.out 2>&1 & echo $! > wait.pid")
        h.wait(0.7)
        h.action(#selector(DocumentWindow.marginResolveAll(_:)))
        // A comment left unsent stays with the writer.
        h.select("times")
        h.key("cmd-opt-m")
        h.compose("Plural?")
        h.key("cmd-enter")
        h.wait(0.1)
        h.win.close()
        h.wait(until: exited, "margin wait to exit")
        XCTAssertEqual(
            h.sh("sed -E 's|`/[^`]*/|`|g' wait.out"),
            """
            I resolved comments on `doc.md`.

            #1 `doc.md:1:1-1:5` "Retry"
              - Agent: Why retry?
              - User resolved the thread.
            #2 `doc.md:1:7-1:11` "three"
              - Agent: Why three?
              - User resolved the thread.

            `doc.md` is no longer open in Margin, so no comments will come. \
            Stop waiting: the writer will ask if they want another review.

            """)
    }
}
