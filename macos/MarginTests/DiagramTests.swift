import AppKit
import XCTest

@testable import MarginKit

/// A `mermaid` code block shows as its diagram, drawn by the core in the
/// page's colors; one that can't be drawn says why, over its source; and
/// it is one piece in editing, find, comments and change bars.
final class DiagramTests: XCTestCase {
    private let flowchart = "```mermaid\nflowchart LR\n  A[Start] --> B[Done]\n```"

    @MainActor
    func testAMermaidBlockShowsAsItsDiagram() throws {
        let h = try Harness()
        defer { h.close() }
        h.reset("Intro.\n\n\(flowchart)\n\nAfter it.\n\n```js\nlet x = 1\n```\n")
        h.waitForImages()
        // Other code blocks stay code.
        XCTAssertEqual(h.images.count, 1)
        XCTAssertTrue(h.images[0].hasPrefix("diagram "), h.images[0])
        let r = h.imageRect()
        XCTAssertGreaterThan(r.width, 50)
        XCTAssertGreaterThan(r.height, 20)
        let line = { (s: String) in
            Int(h.view.analysis.lineIndex(pos: UInt32((h.text as NSString).range(of: s).location)))
        }
        XCTAssertGreaterThan(h.view.textTop(line: line("After it")), r.maxY)
        // Nothing of its source shows.
        let source = (h.text as NSString).range(of: flowchart)
        XCTAssertEqual(h.view.visibleText(source), "")
        // Show Markdown shows the source instead.
        h.key("cmd-/")
        XCTAssertEqual(h.images, [])
        XCTAssertEqual(h.view.visibleText(source), flowchart)
    }

    @MainActor
    func testABrokenDiagramSaysWhy() throws {
        let h = try Harness()
        defer { h.close() }
        h.reset("```mermaid\nflowchart TD\n  A[Start --> B\n```\n\n```mermaid\nflowchat TD\n  A --> B\n```\n")
        h.waitForImages()
        XCTAssertEqual(h.images.count, 2)
        XCTAssertTrue(h.images[0].hasPrefix("diagram error "), h.images[0])
        XCTAssertEqual(h.images[1], #"diagram error "Unknown diagram type “flowchat”." at -"#)
        // The block shows the source, every line of it.
        if case .diagramError(let e) = h.view.look(of: h.view.objects[0]) {
            XCTAssertEqual(e.lines.map(\.text), ["flowchart TD", "  A[Start --> B"])
        } else {
            XCTFail("not an error block")
        }
        // Its source shows, so find matches it.
        let find: FindBar = h.doc.findBar
        find.open(showingReplaceField: false)
        find.search.stringValue = "flowchat"
        find.refresh(goingToMatchAtOrAfterCursor: true)
        XCTAssertEqual(find.textMatches.count, 1)
        find.close()
    }

    /// Find matches the labels a diagram draws, not its source; Replace
    /// All replaces the text's and says how many it left in diagrams.
    @MainActor
    func testFindMatchesWhatADiagramDraws() throws {
        let h = try Harness()
        defer { h.close() }
        h.reset("Start here.\n\n\(flowchart)\n")
        h.waitForImages()
        let find: FindBar = h.doc.findBar
        find.open(showingReplaceField: true)
        find.search.stringValue = "start"
        find.refresh(goingToMatchAtOrAfterCursor: true)
        XCTAssertEqual(find.matches.count, 2)
        XCTAssertEqual(find.textMatches, [NSRange(location: 0, length: 5)])
        XCTAssertEqual(find.labelHighlights().count, 1)
        find.search.stringValue = "flowchart"
        find.refresh(goingToMatchAtOrAfterCursor: true)
        XCTAssertEqual(find.matches.count, 0)
        find.search.stringValue = "Start"
        find.replaceField.stringValue = "Begin"
        find.refresh(goingToMatchAtOrAfterCursor: true)
        find.replaceAll()
        XCTAssertTrue(h.text.hasPrefix("Begin here."), h.text)
        XCTAssertTrue(h.text.contains("A[Start]"), h.text)
        XCTAssertEqual(find.matches.count, 1)
        find.close()
    }

    /// Like an image: a key toward it selects it, a click too; selected,
    /// copying gives its Markdown and Backspace deletes it.
    @MainActor
    func testADiagramIsOnePiece() throws {
        let h = try Harness()
        defer { h.close() }
        let text = "Intro.\n\n\(flowchart)\n\nNext.\n"
        h.reset(text)
        h.waitForImages()
        let range = (h.text as NSString).range(of: flowchart)
        h.find("Intro.")
        h.key("right")
        XCTAssertEqual(h.view.selectedRange(), range)
        h.key("right")
        XCTAssertEqual(h.view.cursor, (h.text as NSString).range(of: "Next.").location)
        h.click(image: 0)
        XCTAssertEqual(h.view.selectedRange(), range)
        h.key("cmd-c")
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), flowchart)
        h.key("backspace")
        XCTAssertFalse(h.text.contains("mermaid"), h.text)
        h.key("cmd-z")
        XCTAssertEqual(h.text, text)
        // Up and Down pass it.
        h.find("Intro")
        h.key("down")
        XCTAssertEqual(
            h.view.analysis.lineIndex(pos: UInt32(h.view.cursor)),
            h.view.analysis.lineIndex(pos: UInt32(range.location + range.length + 2)))
    }

    /// A comment on a diagram is on all of its source, with its card beside
    /// its top; a change to it is marked along its height.
    @MainActor
    func testCommentsAndChangesOnADiagram() throws {
        let h = try Harness("Intro.\n\n\(flowchart)\n\nNext.\n")
        defer { h.close() }
        h.commit()
        h.waitForImages()
        h.click(image: 0)
        h.key("cmd-opt-m")
        h.compose("Add a step.")
        h.key("cmd-enter")
        h.wait(0.2)
        let source = (h.text as NSString).range(of: flowchart)
        XCTAssertEqual(h.comments, "#1 open [\(source.location),\(NSMaxRange(source))) [\"Add a step.\"]\nactive 1")
        XCTAssertEqual(h.cardOffsets, [1: 0])
        XCTAssertEqual(h.view.location(of: source.location).minY, h.imageRect().minY)
        // Edit a label: the diagram is marked along its height.
        h.external(h.text.replacingOccurrences(of: "B[Done]", with: "B[Shipped]"))
        h.wait(forBanner: "Updated from disk")
        h.waitForImages()
        let r = h.imageRect()
        for y in [r.minY + 3, r.midY, r.maxY - 3] {
            XCTAssertEqual(h.change(at: NSPoint(x: h.changeX, y: y)), "changed", "at \(y - r.minY)")
        }
    }
}
