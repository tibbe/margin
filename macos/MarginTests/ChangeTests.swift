import AppKit
import XCTest

@testable import MarginKit

/// Lines changed since the last commit have a bar in the left margin, and
/// Next and Previous Change step through them.
final class ChangeTests: XCTestCase {
    @MainActor
    func testChangesSinceTheLastCommitAreMarked() throws {
        let h = try Harness("One.\n\nTwo.\n\nThree.\n")
        defer { h.close() }
        // Outside a repository, and before the first commit: nothing.
        XCTAssertEqual(h.changes, [])
        h.sh("git init -q")
        h.bringToFront()
        h.wait(0.3)
        XCTAssertNil(h.view.committed)

        h.commit()
        XCTAssertEqual(h.changes, [])
        h.find("Two")
        h.type(" more")
        h.find("Three.")
        h.type("\nFour.")
        XCTAssertEqual(h.changes, ["changed 3", "added 6-7"])
        XCTAssertEqual(h.bar(beside: "One"), "none")
        XCTAssertEqual(h.bar(beside: "Two more"), "changed")
        XCTAssertEqual(h.bar(beside: "Three"), "none")
        XCTAssertEqual(h.bar(beside: "Four"), "added")
        // Show Markdown marks the same lines.
        h.key("cmd-/")
        XCTAssertEqual(h.bar(beside: "Two more"), "changed")
        XCTAssertEqual(h.bar(beside: "Three"), "none")
        h.key("cmd-/")

        // An agent's edits are marked too.
        h.save()
        h.external("One.\n\nThree.\n\nFour.\n")
        h.wait(forBanner: "Updated from disk")
        XCTAssertEqual(h.changes, ["deleted before 3", "added 4-5"])
        XCTAssertEqual(h.mark(before: "Three"), "deleted")

        // A commit made elsewhere shows once the window is focused again.
        h.sh("git commit -qam next")
        h.bringToFront()
        h.wait(until: { h.changes.isEmpty }, "the commit")
    }

    @MainActor
    func testNextAndPreviousChangeStepThroughThemAndWrap() throws {
        let h = try Harness("One.\n\nTwo.\n\nThree.\n\nFour.\n")
        defer { h.close() }
        h.commit()
        XCTAssertEqual(h.menu("Next Change"), "disabled")
        XCTAssertEqual(h.menu("Previous Change"), "disabled")
        h.reset("One.\n\nTwo!\n\nThree.\n\nFour!\n")
        XCTAssertEqual(h.menu("Next Change"), "enabled")
        h.findBefore("One")
        h.key("cmd-opt-shift-down")
        XCTAssertEqual(h.selection, "6 0")
        h.key("cmd-opt-shift-down")
        XCTAssertEqual(h.selection, "20 0")
        h.key("cmd-opt-shift-down")
        XCTAssertEqual(h.selection, "6 0")
        h.key("cmd-opt-shift-up")
        XCTAssertEqual(h.selection, "20 0")
        h.key("cmd-opt-shift-up")
        XCTAssertEqual(h.selection, "6 0")
        // From inside a change, Previous goes to its start.
        h.find("Fo")
        h.key("cmd-opt-shift-up")
        XCTAssertEqual(h.selection, "20 0")
    }
}

extension Harness {
    /// Commits the document as it is on disk, in a repository in its folder,
    /// and waits for the window to read it.
    func commit() {
        save()
        sh(
            "git init -q && git add doc.md && "
                + "git -c user.name=T -c user.email=t@example.com -c commit.gpgsign=false commit -qm c")
        bringToFront()
        wait(until: { view.committed != nil }, "the commit")
    }

    /// The window coming to the front.
    func bringToFront() {
        doc.windowDidBecomeKey(Notification(name: NSWindow.didBecomeKeyNotification, object: win))
    }

    /// The changes marked, with 1-based line numbers: `changed 3`,
    /// `added 6-7`, `deleted before 3`.
    var changes: [String] {
        view.ensureFresh()
        return view.changes.map { c in
            let (first, end) = (Int(c.firstLine) + 1, Int(c.endLine))
            switch c.kind {
            case .deleted: return "deleted before \(first)"
            case .added, .changed:
                return "\(c.kind == .added ? "added" : "changed") \(first == end ? "\(first)" : "\(first)-\(end)")"
            }
        }
    }

    /// The change bar beside the first line of `s` on screen: `added`,
    /// `changed` or `none`.
    func bar(beside s: String) -> String {
        let r = (view.string as NSString).range(of: s)
        XCTAssertNotEqual(r.location, NSNotFound, "\(s) not found")
        return change(at: NSPoint(x: changeX, y: view.location(of: r.location).midY))
    }

    /// The deletion mark between the line holding `s` and the line shown
    /// before it.
    func mark(before s: String) -> String {
        let r = (view.string as NSString).range(of: s)
        let li = Int(view.analysis.lineIndex(pos: UInt32(r.location)))
        let prev = (0..<li).last { view.lines[$0].kind != .blank } ?? 0
        let y = (view.textBottom(line: prev) + view.textTop(line: li)) / 2
        return change(at: NSPoint(x: changeX, y: y))
    }

    /// Where bars and marks are drawn, a little inside their left edge.
    var changeX: CGFloat { view.geometry.left - 18 * Theme.scale + 1.5 * Theme.scale }

    /// The change mark drawn at `p`: `added`, `changed`, `deleted` or `none`.
    func change(at p: NSPoint) -> String {
        view.ensureFresh()
        let b = view.bounds
        guard let rep = view.bitmapImageRepForCachingDisplay(in: b) else { return "no bitmap" }
        view.cacheDisplay(in: b, to: rep)
        let scale = CGFloat(rep.pixelsWide) / b.width
        guard let c = rep.colorAt(x: Int(p.x * scale), y: Int(p.y * scale))?.usingColorSpace(.sRGB) else {
            return "no color"
        }
        // The bitmap's color space shifts the system colors, so go by hue:
        // green, blue or red, or gray (the page).
        let rgb = [c.redComponent, c.greenComponent, c.blueComponent]
        guard let hi = rgb.max(), let lo = rgb.min(), hi - lo > 0.2 else { return "none" }
        return ["deleted", "added", "changed"][rgb.firstIndex(of: hi)!]
    }
}
