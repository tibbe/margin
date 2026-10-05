import AppKit
import XCTest

@testable import MarginKit

/// An image alone in its paragraph shows as the image, local or remote,
/// sized for the column; one that can't be shown says why; and it is one
/// piece in editing, find, comments, change bars and print.
final class ImageTests: XCTestCase {
    @MainActor
    func testAnImageAloneInItsParagraphShowsAsTheImage() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("shot.png", 200, 100)
        try h.png("img/sub.png", 60, 30)
        let folder = (h.path as NSString).deletingLastPathComponent
        h.reset(
            "Intro.\n\n![A shot](shot.png)\n\nSee ![inline](shot.png) here.\n\n- ![listed](img/sub.png)\n\n"
                + "> ![quoted](\(folder)/img/sub.png)\n")
        h.waitForImages()
        // Relative paths are from the document's folder; one starting with
        // `/` is from the root.
        XCTAssertEqual(h.images, ["image 200×100", "image 60×30", "image 60×30"])
        XCTAssertEqual(h.imageColor(), "red")
        // An image in running text shows as its alt text.
        let line = (h.text as NSString).range(of: "See ![inline](shot.png) here.")
        XCTAssertEqual(h.view.visibleText(line), "See inline here.")
        // Nothing of an image's source shows.
        let source = (h.text as NSString).range(of: "![A shot](shot.png)")
        XCTAssertEqual(h.view.visibleText(source), "")
        // Show Markdown shows the source instead.
        h.key("cmd-/")
        XCTAssertEqual(h.images, [])
        XCTAssertEqual(h.view.visibleText(source), "![A shot](shot.png)")
    }

    @MainActor
    func testImagesFitTheColumnAndFollowTheZoom() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("big.png", 3000, 600)
        try h.png("small.png", 40, 20)
        h.reset("![big](big.png)\n\n![small](small.png)\n\n- ![item](big.png)\n")
        h.waitForImages()
        let column = h.view.geometry.docWidth
        let item = column - Theme.itemStep * Theme.scale
        // Never wider than the column, and never enlarged.
        XCTAssertEqual(
            h.images,
            [
                "image \(Int(column))×\(Int((600 * column / 3000).rounded()))", "image 40×20",
                "image \(Int(item.rounded()))×\(Int((600 * item / 3000).rounded()))",
            ])
        XCTAssertEqual(h.imageRect(0).width, column)
        // Its line is as tall as it is.
        XCTAssertEqual(h.view.textBottom(line: 2) - h.view.textTop(line: 2), 20)
        h.action(#selector(AppDelegate.marginLarger(_:)))
        XCTAssertEqual(h.images[1], "image 44×22")
        h.action(#selector(AppDelegate.marginActualSize(_:)))
        XCTAssertEqual(h.images[1], "image 40×20")
    }

    /// A placeholder holds an image's place until its size is known; the
    /// text being read then stays where it is in the window.
    @MainActor
    func testAPlaceholderHoldsTheImagesPlace() throws {
        let h = try Harness()
        defer { h.close() }
        let held = HeldImages()
        let url = "https://example.com/a.png"
        let paragraphs = (1...80).map { "Paragraph \($0)." }.joined(separator: "\n\n")
        h.reset("![remote](\(url))\n\n\(paragraphs)\n")
        h.layOut()
        let column = Int(h.view.geometry.docWidth)
        XCTAssertEqual(h.images, ["placeholder \(column)×150"])
        // A box on the code background, not the page's.
        let box = h.imageRect()
        XCTAssertNotEqual(h.imageColor(), h.color(at: NSPoint(x: box.midX, y: box.maxY + 4)))

        // Read from paragraph 40 down.
        let p40 = (h.text as NSString).range(of: "Paragraph 40.").location
        let clip = h.doc.scrollView.contentView
        clip.scroll(to: NSPoint(x: 0, y: h.view.location(of: p40).minY - 50))
        h.doc.scrollView.reflectScrolledClipView(clip)
        let before = h.view.location(of: p40).minY
        let shown = before - clip.bounds.minY
        held.release(url, Harness.png(300, 400))
        h.waitForImages()
        XCTAssertEqual(h.images, ["image 300×400"])
        XCTAssertEqual(h.view.location(of: p40).minY - before, 400 - 150)
        XCTAssertEqual(h.view.location(of: p40).minY - clip.bounds.minY, shown)
    }

    /// Remote images load over HTTP, saying only "margin" as the agent.
    @MainActor
    func testRemoteImagesLoad() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("served.png", 120, 80)
        let server = try ImageServer(serving: h.dir)
        defer { server.stop() }
        h.reset("![served](\(server.url("served.png")))\n\n![gone](\(server.url("gone.png")))\n")
        h.waitForImages(timeout: 10)
        XCTAssertEqual(h.images, ["image 120×80", #"broken "gone""#])
        XCTAssertEqual(
            h.view.imageToolTip(h.view.objects[1]), "\(server.url("gone.png"))\nThe server answered 404 (not found).")
        XCTAssertEqual(Set(server.agents), ["margin"])
    }

    @MainActor
    func testABrokenImageSaysWhy() throws {
        let h = try Harness()
        defer { h.close() }
        try Data("not an image".utf8).write(to: h.dir.appendingPathComponent("text.png"))
        h.reset("![The shot](missing.png)\n\n![](shots/empty.png)\n\n![Text](text.png)\n\n![odd](ftp://x.y/a.png)\n")
        h.waitForImages()
        // Its alt text, or its file name without one.
        XCTAssertEqual(h.images, [#"broken "The shot""#, #"broken "empty.png""#, #"broken "Text""#, #"broken "odd""#])
        let folder = (h.path as NSString).deletingLastPathComponent
        let tips = h.view.objects.map { h.view.imageToolTip($0) }
        XCTAssertEqual(
            tips,
            [
                "\(folder)/missing.png\nThere’s no file there.", "\(folder)/shots/empty.png\nThere’s no file there.",
                "\(folder)/text.png\nIt isn’t an image Margin can read.",
                "ftp://x.y/a.png\nMargin can’t load images from “ftp:” links.",
            ])
        // At text size, on a line of text's height.
        let r = h.imageRect(0)
        XCTAssertEqual(r.height, h.view.textBottom(line: 0) - h.view.textTop(line: 0), accuracy: 2)
        // Hovering it says so.
        XCTAssertEqual(
            h.view.view(h.view, stringForToolTip: 0, point: NSPoint(x: r.midX, y: r.midY), userData: nil), tips[0])
        // Put in place, it shows once the window is back in front.
        try h.png("missing.png", 30, 30)
        h.bringToFront()
        h.wait(until: { h.images.first == "image 30×30" }, "the image")
    }

    @MainActor
    func testFindMatchesWhatIsShown() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("ok.png", 50, 50)
        h.reset("![shown](ok.png)\n\n![lost](nope.png)\n\nText ![inline](ok.png), shown text.\n")
        h.waitForImages()
        let find: FindBar = h.doc.findBar
        find.open(showingReplaceField: true)
        func found(_ s: String) -> [String] {
            find.search.stringValue = s
            find.refresh(goingToMatchAtOrAfterCursor: false)
            return find.textMatches.map { (h.text as NSString).substring(with: $0) }
        }
        // An image drawn as an image matches nothing, neither its alt text
        // nor its path; alt text that is shown does.
        XCTAssertEqual(found("shown"), ["shown"])
        XCTAssertEqual(find.textMatches.first?.location, (h.text as NSString).range(of: "shown text").location)
        XCTAssertEqual(found("ok.png"), [])
        XCTAssertEqual(found("lost"), ["lost"])
        XCTAssertEqual(found("inline"), ["inline"])
        // Replacing the alt text of a missing image.
        find.search.stringValue = "lost"
        find.replaceField.stringValue = "found"
        find.refresh(goingToMatchAtOrAfterCursor: true)
        find.replaceAll()
        XCTAssertTrue(h.text.contains("![found](nope.png)"))
        // The match is highlighted where its text is drawn: below the
        // baseline of its "n".
        XCTAssertEqual(found("foun"), ["foun"])
        let r = h.imageRect(1)
        let n = NSPoint(x: r.maxX - 12, y: r.maxY - 1)
        let page = NSPoint(x: r.maxX + 20, y: r.maxY - 1)
        XCTAssertNotEqual(h.color(at: n), h.color(at: page))
        // Closing Find selects the match: the image that holds it.
        find.close()
        XCTAssertEqual(h.selection, "18 18")
        h.find("Text")
        XCTAssertEqual(h.color(at: n), h.color(at: page))
    }

    /// Left or Right toward an image selects it, and the next press moves
    /// past it; Up and Down pass it keeping the column; Shift extends over
    /// it whole; a click selects it too.
    @MainActor
    func testTheCursorNeverRestsOnAnImage() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("ok.png", 80, 40)
        h.reset("Same text.\n\n![a](ok.png)\n\nSame text.\n")
        h.waitForImages()
        let image = "12 12"
        h.find("Same text.")
        h.key("right")
        XCTAssertEqual(h.selection, image)
        h.key("right")
        XCTAssertEqual(h.selection, "26 0")
        h.key("left")
        XCTAssertEqual(h.selection, image)
        h.key("left")
        XCTAssertEqual(h.selection, "10 0")
        // Up and Down keep the column.
        h.find("Same")
        h.key("down")
        XCTAssertEqual(h.selection, "30 0")
        h.key("up")
        XCTAssertEqual(h.selection, "4 0")
        // Shift extends over it whole, either way.
        h.find("Same text.")
        h.key("shift-right")
        XCTAssertEqual(h.selection, "10 14")
        h.key("shift-right")
        XCTAssertEqual(h.selection, "10 16")
        h.key("shift-left")
        XCTAssertEqual(h.selection, "10 2")
        h.view.setSelectedRange(NSRange(location: 26, length: 0))
        h.key("shift-left")
        XCTAssertEqual(h.selection, "12 14")
        h.find("Same")
        h.key("shift-down")
        XCTAssertEqual(h.selection, "4 26")
        h.click(image: 0)
        XCTAssertEqual(h.selection, image)
        // From a selected image, Shift extends from its far side.
        h.key("shift-left")
        XCTAssertEqual(h.selection, "10 14")
        h.click(image: 0)
        h.key("shift-right")
        XCTAssertEqual(h.selection, "12 14")
    }

    /// Selected, Delete removes an image, typing replaces it, and copying
    /// gives its Markdown; Backspace and Delete toward it select it first.
    @MainActor
    func testASelectedImageIsOnePiece() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("ok.png", 80, 40)
        let text = "Intro.\n\n![a](ok.png)\n\nNext.\n"
        h.reset(text)
        h.waitForImages()
        h.click(image: 0)
        XCTAssertEqual(h.imageColor(), "reddish")
        h.key("cmd-c")
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "![a](ok.png)")
        h.key("backspace")
        XCTAssertEqual(h.text, "Intro.\n\n\n\nNext.\n")
        h.key("cmd-z")
        XCTAssertEqual(h.text, text)
        h.click(image: 0)
        h.type("x")
        XCTAssertEqual(h.text, "Intro.\n\nx\n\nNext.\n")
        h.reset(text)
        h.waitForImages()
        h.findBefore("Next.")
        h.key("backspace")
        XCTAssertEqual(h.selection, "8 12")
        XCTAssertEqual(h.text, text)
        h.find("Intro.")
        h.key("delete")
        XCTAssertEqual(h.selection, "8 12")
        XCTAssertEqual(h.text, text)
        // A selected image isn't tinted once the selection leaves it.
        h.key("right")
        XCTAssertEqual(h.imageColor(), "red")
        // Formatting has no text to act on.
        h.click(image: 0)
        h.key("cmd-b")
        XCTAssertEqual(h.text, text)
        XCTAssertEqual(h.selection, "8 12")
    }

    /// A comment on an image is on all of its source: the whole image is
    /// tinted, and its card sits beside the image's top.
    @MainActor
    func testACommentOnAnImageIsOnAllOfItsSource() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("ok.png", 80, 200)
        h.reset("Intro.\n\n- ![a](ok.png)\n\nNext.\n")
        h.waitForImages()
        h.click(image: 0)
        h.key("cmd-opt-m")
        h.compose("Crop it.")
        h.key("cmd-enter")
        h.wait(0.2)
        XCTAssertEqual(h.comments, "#1 open [10,22) [\"Crop it.\"]\nactive 1")
        XCTAssertEqual(h.cardOffsets, [1: 0])
        XCTAssertEqual(h.view.location(of: 10).minY, h.imageRect().minY)
        XCTAssertEqual(h.imageColor(), "reddish")
        h.key("escape")
        XCTAssertEqual(h.imageColor(), "reddish")
        // Selecting the image focuses its thread.
        h.click(image: 0)
        XCTAssertEqual(h.layer.active, 1)
        // The list item's bullet is beside its top.
        let marker = h.view.markerPosition(try XCTUnwrap(h.view.items.first))
        XCTAssertLessThan(marker.baseline, h.imageRect().minY + 30)
    }

    @MainActor
    func testAnImageIsMarkedAlongItsHeight() throws {
        let h = try Harness("Intro.\n\nNext.\n")
        defer { h.close() }
        h.commit()
        try h.png("tall.png", 50, 300)
        h.find("Intro.")
        h.view.pasteText("\n\n![tall](tall.png)")
        h.waitForImages()
        XCTAssertEqual(h.changes.count, 1)
        let r = h.imageRect()
        for y in [r.minY + 3, r.midY, r.maxY - 3] {
            XCTAssertEqual(h.change(at: NSPoint(x: h.changeX, y: y)), "added", "at \(y - r.minY)")
        }
        XCTAssertEqual(h.change(at: NSPoint(x: h.changeX, y: r.maxY + 12)), "none")
    }

    /// Print waits for images to load, and shows them.
    @MainActor
    func testPrintShowsImagesOnceLoaded() throws {
        let h = try Harness()
        defer { h.close() }
        let held = HeldImages()
        let url = "https://example.com/print.png"
        let folder = (h.path as NSString).deletingLastPathComponent
        let page = printView(text: "Before.\n\n![p](\(url))\n\nAfter.\n", imageFolder: folder, width: 500)
        var ready = false
        page.whenImagesSettled { ready = true }
        h.wait(0.2)
        XCTAssertFalse(ready)
        held.release(url, Harness.png(100, 60))
        h.wait(until: { ready }, "printing")
        page.sizeToFit()
        let o = page.objects[0]
        let r = try XCTUnwrap(page.objectRect(o, look: page.look(of: o)))
        XCTAssertEqual(r.size, NSSize(width: 100, height: 60))
        XCTAssertEqual(h.color(at: NSPoint(x: r.midX, y: r.midY), in: page), "red")
    }

    /// In a list or a quote, an image takes its space too: the text after
    /// it is below it, and the quote bar runs along it.
    @MainActor
    func testImagesInListsAndQuotesTakeTheirSpace() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("ok.png", 80, 100)
        h.reset("- First item.\n- ![a](ok.png)\n- Third item.\n\n> A quote:\n>\n> ![q](ok.png)\n>\n> After it.\n")
        h.waitForImages()
        XCTAssertEqual(h.images, ["image 80×100", "image 80×100"])
        let line = { (s: String) in
            Int(h.view.analysis.lineIndex(pos: UInt32((h.text as NSString).range(of: s).location)))
        }
        XCTAssertGreaterThan(h.view.textTop(line: line("Third")), h.imageRect(0).maxY)
        XCTAssertGreaterThan(h.view.textTop(line: line("After")), h.imageRect(1).maxY)
        let q = try XCTUnwrap(h.view.quotes.first)
        let bar = h.view.textTop(line: Int(q.firstLine))...h.view.textBottom(line: Int(q.lastLine))
        XCTAssertTrue(bar.contains(h.imageRect(1).minY) && bar.contains(h.imageRect(1).maxY))
        XCTAssertEqual(h.imageColor(0), "red")
        XCTAssertEqual(h.imageColor(1), "red")
    }

    /// Up and Down onto a short line above an image stop on that line, not
    /// on the image: its hidden source is laid out at the end of that line.
    @MainActor
    func testUpAndDownStopOnAShortLineAboveAnImage() throws {
        let h = try Harness()
        defer { h.close() }
        try h.png("ok.png", 80, 40)
        h.reset("- A long first item, long enough to be wider than the next.\n- Short.\n- ![a](ok.png)\n- After.\n")
        h.waitForImages()
        let s = h.text as NSString
        let short = s.range(of: "Short.")
        let after = s.range(of: "After.")
        h.find("than the next.")
        h.key("down")
        XCTAssertEqual(h.selection, "\(NSMaxRange(short)) 0")
        h.key("down")
        XCTAssertTrue(NSLocationInRange(h.view.cursor, NSRange(location: after.location, length: after.length + 1)))
        h.key("up")
        XCTAssertTrue(NSLocationInRange(h.view.cursor, NSRange(location: short.location, length: short.length + 1)))
    }

    /// Many large images cost the memory of those drawn, at the size drawn,
    /// within the library's budget.
    @MainActor
    func testManyLargeImagesStayWithinTheBudget() throws {
        let h = try Harness()
        defer { h.close() }
        let library = ImageLibrary.shared
        let budget = library.budget
        defer { library.budget = budget }
        library.budget = 30_000_000
        let names = (0..<40).map { "big\($0).png" }
        for n in names { try h.png(n, 2400, 1600) }
        h.size(1100, 700)
        h.reset(names.map { "Text before \($0).\n\n![\($0)](\($0))\n" }.joined(separator: "\n"))
        h.waitForImages(timeout: 20)
        let clip = h.doc.scrollView.contentView
        var y: CGFloat = 0
        while y < h.view.frame.height {
            clip.scroll(to: NSPoint(x: 0, y: y))
            h.doc.scrollView.reflectScrolledClipView(clip)
            _ = h.drawing(in: h.view.visibleRect)
            XCTAssertLessThanOrEqual(library.decodedBytes, library.budget)
            y += clip.bounds.height
        }
        // What is on screen is still drawn, from what is decoded now.
        let last = h.imageRect(names.count - 1)
        clip.scroll(to: NSPoint(x: 0, y: h.view.convert(last, to: clip.documentView).minY - 20))
        h.doc.scrollView.reflectScrolledClipView(clip)
        let r = h.view.visibleRect
        XCTAssertTrue(r.contains(NSPoint(x: last.midX, y: last.midY)), "\(last) in \(r)")
        XCTAssertEqual(h.color(at: NSPoint(x: last.midX, y: last.midY), drawing: r), "red")
        XCTAssertLessThanOrEqual(library.decodedBytes, library.budget)
    }
}
