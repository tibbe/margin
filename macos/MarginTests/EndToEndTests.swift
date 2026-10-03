import AppKit
import XCTest

@testable import MarginKit

/// What a writer does and sees, through `Editor` alone: these tests don't
/// depend on how the editor draws or edits text, so any text engine for it
/// has to pass them.
///
/// Fill tests sample just above the x-height, so the text they sample has
/// no capitals, ascenders or dots: `we see our users`.
final class EndToEndTests: XCTestCase {
    /// A list item hard-wrapped in the file, with bold and code.
    private let item = "- **Cause:** the root `x` never says which\n  keeps its checks in an `x` beside its code.\n"
    private let itemShown = "Cause: the root x never says which keeps its checks in an x beside its code."

    /// The same, in letters a fill can be sampled on.
    private let plain = "- **use:** we see our users\n  across more rows now, as ever.\n"

    /// A paragraph hard-wrapped over many short lines, long enough to wrap
    /// in a narrow window once reflowed.
    private var long: String {
        (1...12).map { "line \($0) has some words" }.joined(separator: "\n") + "\n"
    }

    // MARK: - Reading

    @MainActor
    func testHiddenSyntaxIsNotDrawn() throws {
        let e = try Editor("# Title\n\nSome **bold**, `code` and [a link](https://example.com).\n")
        defer { e.close() }
        XCTAssertEqual(e.lines, ["Title", "Some bold, code and a link."])
    }

    @MainActor
    func testReflowShowsAHardWrappedItemAsOneParagraph() throws {
        let off = try Editor(item)
        XCTAssertEqual(off.lines, ["Cause: the root x never says which", "keeps its checks in an x beside its code."])
        off.close()
        let on = try Editor(item, reflow: true)
        defer { on.close() }
        XCTAssertEqual(on.lines, [itemShown])
        XCTAssertEqual(on.file, item, "reflowing doesn't touch the file")
    }

    @MainActor
    func testReflowedTextWrapsAtTheWindow() throws {
        let e = try Editor(long, reflow: true, width: 520)
        defer { e.close() }
        let shown = (1...12).map { "line \($0) has some words" }.joined(separator: " ")
        XCTAssertGreaterThan(e.lines.count, 2)
        XCTAssertEqual(e.lines.joined(), shown, "every word, once, in order")
    }

    // MARK: - The caret

    @MainActor
    func testClickingAfterAJoinedBreakPutsTheCaretThere() throws {
        let e = try Editor(item, reflow: true)
        defer { e.close() }
        e.click("keeps")
        XCTAssertEqual(e.caret, itemShown.replacingOccurrences(of: "keeps", with: "|keeps"))
        e.type("Z")
        XCTAssertEqual(e.file, item.replacingOccurrences(of: "  keeps", with: "  Zkeeps"))
        XCTAssertEqual(e.lines, [itemShown.replacingOccurrences(of: "keeps", with: "Zkeeps")])
    }

    @MainActor
    func testUpAndDownKeepTheColumnAcrossJoinedAndWrappedLines() throws {
        let e = try Editor(long, reflow: true, width: 520)
        defer { e.close() }
        e.click("line 6 has", offset: 2)
        let at = e.caret
        e.key("up")
        XCTAssertNotEqual(e.caret, at)
        e.key("down")
        XCTAssertEqual(e.caret, at)
        e.key("down")
        XCTAssertNotEqual(e.caret, at)
        e.key("up")
        XCTAssertEqual(e.caret, at)
    }

    @MainActor
    func testUpAndDownBesideHiddenSyntaxComeBack() throws {
        let e = try Editor("one **bold** c\nsecond line here\n")
        defer { e.close() }
        e.click("bold", offset: 4)
        XCTAssertEqual(e.caret, "one bold| c")
        e.key("down")
        e.key("up")
        XCTAssertEqual(e.caret, "one bold| c")
    }

    @MainActor
    func testArrowsStepOverAnEmojiWhole() throws {
        let e = try Editor("a😀x\n")
        defer { e.close() }
        e.click("😀")
        e.key("right")
        e.type("Z")
        XCTAssertEqual(e.file, "a😀Zx\n")
    }

    /// The spec: typing at the end of bold continues the bold only when the
    /// cursor is inside it. The arrows stop inside it there; typing right
    /// after the closing markers stays outside.
    @MainActor
    func testTypingAtTheEndOfBoldContinuesItOnlyFromInside() throws {
        let inside = try Editor("a **bold** c\n")
        inside.click("bold", offset: 2)
        inside.key("right")
        XCTAssertEqual(inside.caret, "a bol|d c")
        inside.key("right")
        XCTAssertEqual(inside.caret, "a bold| c")
        inside.type("X")
        XCTAssertEqual(inside.file, "a **boldX** c\n")
        inside.key("right")
        XCTAssertEqual(inside.caret, "a boldX |c", "one press, one character")
        inside.key("left")
        XCTAssertEqual(inside.caret, "a boldX| c")
        inside.key("left")
        XCTAssertEqual(inside.caret, "a bold|X c")
        inside.close()

        let outside = try Editor("")
        defer { outside.close() }
        outside.type("a **bold**X")
        XCTAssertEqual(outside.file, "a **bold**X")
    }

    @MainActor
    func testAnEmptyDocumentShowsTheCaretAndTakesTyping() throws {
        let e = try Editor("")
        defer { e.close() }
        XCTAssertEqual(e.caret, "|")
        e.type("hi")
        XCTAssertEqual(e.file, "hi")
    }

    @MainActor
    func testTheEmptyLastLineTakesTheCaret() throws {
        let e = try Editor("one\n")
        defer { e.close() }
        e.click("one", offset: 3)
        e.key("down")
        XCTAssertEqual(e.caret, "|")
        e.type("two")
        XCTAssertEqual(e.file, "one\n\ntwo", "a paragraph of its own, not the one above continued")
    }

    // MARK: - Selecting

    @MainActor
    func testDeletingASelectionAcrossAJoinedBreak() throws {
        let e = try Editor(item, reflow: true)
        defer { e.close() }
        e.drag(from: "which", to: "keeps")
        e.key("backspace")
        XCTAssertEqual(e.file, "- **Cause:** the root `x` never says  its checks in an `x` beside its code.\n")
        XCTAssertEqual(e.lines, ["Cause: the root x never says  its checks in an x beside its code."])
    }

    @MainActor
    func testDoubleClickingAWordAfterAJoinedBreakSelectsIt() throws {
        let e = try Editor(item, reflow: true)
        defer { e.close() }
        e.click("keeps", offset: 2, count: 2)
        e.type("holds")
        XCTAssertEqual(e.file, item.replacingOccurrences(of: "keeps", with: "holds"))
    }

    @MainActor
    func testTheSelectionIsDrawnOnWhatIsSelected() throws {
        let e = try Editor(plain, reflow: true)
        defer { e.close() }
        e.drag(from: "our", to: "across")
        XCTAssertEqual(e.fills("we see our users across more"), #""we see ":a "our users across":b " more":a"#)
    }

    // MARK: - Comments

    @MainActor
    func testACommentAcrossAJoinedBreakIsHighlightedEvenly() throws {
        let e = try Editor(plain, reflow: true)
        defer { e.close() }
        e.comment(on: "our users across more", "Why?")
        XCTAssertEqual(
            e.fills("we see our users across more rows"), #""we see ":a "our users across more":b " rows":a"#)
    }

    @MainActor
    func testACommentIsHighlightedOnEveryLineItWrapsOnto() throws {
        let e = try Editor(
            "- **use:** " + (1...10).map { _ in "some more words" }.joined(separator: "\n  ") + "\n", reflow: true,
            width: 520)
        defer { e.close() }
        let all = (1...10).map { _ in "some more words" }.joined(separator: " ")
        e.comment(on: all, "Why?")
        XCTAssertEqual(e.fills(all), "\(all.debugDescription):a")
    }

    @MainActor
    func testACardSitsBesideItsText() throws {
        let e = try Editor(plain, reflow: true)
        defer { e.close() }
        e.comment(on: "across more", "Why?")
        XCTAssertEqual(try XCTUnwrap(e.cardOffset("across more")), 0, accuracy: 3)
    }

    @MainActor
    func testTypingInsideCommentedTextKeepsItCommented() throws {
        let e = try Editor("we see our users now\n")
        defer { e.close() }
        e.comment(on: "our users", "Why?")
        e.click("users")
        e.type("x")
        XCTAssertEqual(e.fills("we see our xusers now"), #""we see ":a "our xusers":b " now":a"#)
    }

    /// Typing at the end of the bold a comment starts on.
    @MainActor
    func testTypingAtTheEndOfCommentedBoldKeepsItCommented() throws {
        let e = try Editor("- **use:** our users\n")
        defer { e.close() }
        e.comment(on: "use: our", "Why?")
        e.click("use:", offset: 2)
        e.key("right", "right")
        e.type("x")
        XCTAssertEqual(e.fills("use:x our users"), #""use:x our":a " users":b"#)
    }

    // MARK: - Input methods

    @MainActor
    func testComposingAfterAJoinedBreak() throws {
        let e = try Editor(item, reflow: true)
        defer { e.close() }
        e.click("keeps")
        e.compose("かん")
        XCTAssertEqual(e.candidateWindow, "beside it")
        XCTAssertEqual(e.lines, [itemShown.replacingOccurrences(of: "keeps", with: "かんkeeps")])
        e.commit("漢")
        XCTAssertEqual(e.file, item.replacingOccurrences(of: "  keeps", with: "  漢keeps"))
    }

    @MainActor
    func testCancellingACompositionLeavesTheFileAsItWas() throws {
        let e = try Editor(item, reflow: true)
        defer { e.close() }
        e.click("keeps")
        e.compose("かん")
        e.cancelComposition()
        XCTAssertEqual(e.file, item)
        XCTAssertEqual(e.lines, [itemShown])
    }

    @MainActor
    func testComposingInsideBold() throws {
        let e = try Editor(item, reflow: true)
        defer { e.close() }
        e.click("Cause", offset: 2)
        e.compose("に")
        XCTAssertEqual(e.candidateWindow, "beside it")
        e.commit("日")
        XCTAssertEqual(e.file, item.replacingOccurrences(of: "Cause", with: "Ca日use"))
    }

    // MARK: - Agents editing the file

    @MainActor
    func testAnAgentsEditAboveKeepsTheCaretOnItsText() throws {
        let e = try Editor(item, reflow: true)
        defer { e.close() }
        e.click("keeps")
        let edited = "# Notes\n\n" + item.replacingOccurrences(of: "the root", with: "the very root")
        e.agentWrites(edited)
        XCTAssertEqual(
            e.caret,
            itemShown.replacingOccurrences(of: "the root", with: "the very root")
                .replacingOccurrences(of: "keeps", with: "|keeps"))
        e.type("Z")
        XCTAssertEqual(e.file, edited.replacingOccurrences(of: "  keeps", with: "  Zkeeps"))
    }

    @MainActor
    func testAnAgentsEditKeepsACommentOnItsText() throws {
        let e = try Editor(plain, reflow: true)
        defer { e.close() }
        e.comment(on: "our users across", "Why?")
        e.agentWrites("# Notes\n\n" + plain.replacingOccurrences(of: "we see", with: "we can see"))
        XCTAssertEqual(e.fills("we can see our users across more"), #""we can see ":a "our users across":b " more":a"#)
    }

    // MARK: - Undo

    @MainActor
    func testUndoingEverythingRestoresTheFileExactly() throws {
        let e = try Editor(item, reflow: true)
        defer { e.close() }
        e.click("keeps")
        e.type("Z")
        e.drag(from: "never", to: "which")
        e.key("backspace")
        e.click("Cause", offset: 2)
        e.compose("に")
        e.commit("日")
        XCTAssertNotEqual(e.file, item)
        for _ in 0..<8 { e.key("cmd-z") }
        XCTAssertEqual(e.file, item)
    }

    // MARK: - Links and find

    @MainActor
    func testCommandClickOpensALinkAfterAJoinedBreak() throws {
        let e = try Editor("- **Use:** see the\n  [docs](https://example.com/docs) now\n", reflow: true)
        defer { e.close() }
        XCTAssertEqual(e.lines, ["Use: see the docs now"])
        e.click("docs", offset: 2, mods: .command)
        XCTAssertEqual(e.opened, ["https://example.com/docs"])
    }

    @MainActor
    func testFindHighlightsAMatchAfterAJoinedBreak() throws {
        let e = try Editor(plain, reflow: true)
        defer { e.close() }
        e.find("across more")
        XCTAssertEqual(e.fills("our users across more rows"), #""our users ":a "across more":b " rows":a"#)
    }
}
