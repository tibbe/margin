import AppKit
import XCTest

@testable import MarginKit

/// A document in an editor window as its writer sees and drives it: what is
/// drawn, where the caret is, what the file holds; clicks on shown text,
/// typing, keys, input methods, comments, an agent editing the file.
///
/// Tests written against this say nothing about how the editor is built.
/// Only the section marked "How the editor draws" knows the text view, so a
/// different text engine replaces that section and keeps the tests.
@MainActor
final class Editor {
    let h: Harness
    /// Links the writer opened, in order.
    private(set) var opened: [String] = []

    init(_ file: String, reflow: Bool = false, width: CGFloat = 1000) throws {
        h = try Harness(file)
        h.size(width, 800)
        h.win.makeFirstResponder(h.view)
        if reflow { h.key("cmd-opt-z") }
        h.view.onOpenLink = { [weak self] in self?.opened.append($0) }
        h.wait(0.1)
    }

    func close() { h.close() }

    // MARK: - What the writer sees

    /// The lines as drawn, top to bottom: hidden syntax isn't drawn, a line
    /// break shown as a space is one. The empty line after a final line
    /// break, where only the caret can be, isn't listed.
    var lines: [String] {
        var out = drawnLines.map { String($0.glyphs.map(\.text).joined()) }
        if out.count > 1 && out.last == "" { out.removeLast() }
        return out
    }

    /// The line the caret is drawn on, with `|` where it is.
    var caret: String {
        let c = caretRect
        guard let line = drawnLines.first(where: { $0.top - 1 <= c.midY && c.midY <= $0.bottom + 1 }) else {
            return "no line at caret \(c)"
        }
        let before = line.glyphs.filter { $0.rect.midX < c.minX }
        let after = line.glyphs.dropFirst(before.count)
        return before.map(\.text).joined() + "|" + after.map(\.text).joined()
    }

    /// The file, once the editor has saved.
    var file: String {
        h.save()
        return h.fileText
    }

    /// The fill behind each shown character of `s` (its first occurrence),
    /// sampled just above its x-height, as runs of characters filled alike:
    /// `"oov":a " now":b`. Pick letters without ascenders.
    func fills(_ s: String) -> String {
        guard let glyphs = locate(s) else { return "\(s.debugDescription) not shown" }
        let v = h.view
        let b = v.bounds
        guard let rep = v.bitmapImageRepForCachingDisplay(in: b) else { return "no bitmap" }
        v.cacheDisplay(in: b, to: rep)
        let scale = CGFloat(rep.pixelsWide) / b.width
        var seen: [[Int]] = []
        var runs: [(text: String, fill: Int)] = []
        for g in glyphs {
            let p = NSPoint(x: g.rect.midX, y: g.baseline - g.xHeight - 2)
            guard let c = rep.colorAt(x: Int(p.x * scale), y: Int(p.y * scale))?.usingColorSpace(.sRGB) else {
                continue
            }
            let rgb = [c.redComponent, c.greenComponent, c.blueComponent].map { Int(($0 * 255).rounded()) }
            let fill =
                seen.firstIndex { zip($0, rgb).allSatisfy { abs($0 - $1) <= 2 } }
                ?? {
                    seen.append(rgb)
                    return seen.count - 1
                }()
            if let last = runs.last, last.fill == fill {
                runs[runs.count - 1].text += g.text
            } else {
                runs.append((g.text, fill))
            }
        }
        return runs.map { "\($0.text.debugDescription):\(Character(UnicodeScalar(UInt8(97 + $0.fill))))" }
            .joined(separator: " ")
    }

    /// Where the input method's candidate window goes for the text being
    /// composed, against where that text is drawn: "beside it" or why not.
    var candidateWindow: String {
        let client = inputClient
        let marked = client.markedRange()
        guard marked.location != NSNotFound else { return "nothing composed" }
        let screen = client.firstRect(forCharacterRange: marked, actualRange: nil)
        let r = h.view.convert(h.win.convertFromScreen(screen), from: nil)
        let text = (client.attributedSubstring(forProposedRange: marked, actualRange: nil)?.string) ?? ""
        guard let first = locate(text)?.first else { return "composition \(text.debugDescription) not shown" }
        let dx = abs(r.minX - first.rect.minX)
        let sameLine = r.midY >= first.rect.minY - 2 && r.midY <= first.rect.maxY + 2
        return dx <= 2 && sameLine ? "beside it" : "at \(r), text at \(first.rect)"
    }

    /// How far the card of the thread on `quote` (shown text) is from the
    /// top of that text's line, in points.
    func cardOffset(_ quote: String) -> CGFloat? {
        h.layOut()
        guard let first = locate(quote)?.first,
            let it = h.layer.items.first(where: { $0.range != nil && h.layer.visible($0.thread) })
        else { return nil }
        let card = it.card.convert(it.card.bounds, to: nil)
        let text = h.view.convert(first.lineRect, to: nil)
        // Window coordinates go up: compare the tops.
        return text.maxY - card.maxY
    }

    // MARK: - What the writer does

    /// A click on shown text: before its `offset`-th character, or just
    /// after it with `offset` equal to its length.
    func click(_ s: String, offset: Int = 0, count: Int = 1, mods: NSEvent.ModifierFlags = []) {
        guard let glyphs = locate(s) else { return XCTFail("\(s.debugDescription) not shown in \(lines)") }
        let p: NSPoint
        if offset >= glyphs.count {
            let g = glyphs[glyphs.count - 1]
            p = NSPoint(x: g.rect.maxX - 0.5, y: g.rect.midY)
        } else {
            let g = glyphs[offset]
            p = NSPoint(x: g.rect.minX + 0.5, y: g.rect.midY)
        }
        mouse(at: p, count: count, mods: mods)
    }

    /// A click at the end of the last line.
    func clickAtEnd() {
        guard let last = drawnLines.last else { return XCTFail("nothing drawn") }
        mouse(at: NSPoint(x: (last.glyphs.last?.rect.maxX ?? last.left) + 30, y: (last.top + last.bottom) / 2))
    }

    /// A drag across shown text, from before `from` to after `to`.
    func drag(from: String, to: String) {
        guard let a = locate(from)?.first, let b = locate(to)?.last else {
            return XCTFail("\(from.debugDescription) or \(to.debugDescription) not shown in \(lines)")
        }
        h.drag(
            h.view, from: NSPoint(x: a.rect.minX + 0.5, y: a.rect.midY),
            to: NSPoint(x: b.rect.maxX - 0.5, y: b.rect.midY))
    }

    func type(_ s: String) { h.type(s) }

    func key(_ names: String...) { for n in names { h.key(n) } }

    /// Composes text with an input method, without committing it.
    func compose(_ s: String) {
        inputClient.setMarkedText(
            s, selectedRange: NSRange(location: (s as NSString).length, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0))
    }

    /// The input method commits `s` in place of what was composed.
    func commit(_ s: String) {
        inputClient.insertText(s, replacementRange: NSRange(location: NSNotFound, length: 0))
    }

    /// Escape in the input method: the composition goes away.
    func cancelComposition() {
        compose("")
        inputClient.unmarkText()
    }

    /// Comments on shown text through the editor: select it, Comment, type
    /// the comment, send it, and leave the card.
    func comment(on s: String, _ body: String) {
        drag(from: s, to: s)
        h.key("cmd-opt-m")
        h.compose(body)
        h.key("cmd-enter")
        h.key("escape")
        h.wait(0.1)
    }

    /// An agent writes the file while it is open; the editor takes it in.
    func agentWrites(_ text: String) {
        h.external(text)
        h.wait(until: { self.h.view.string == text }, "the editor to take in the agent's edit")
        h.wait(0.1)
    }

    /// Opens the find bar and searches for `s`.
    func find(_ s: String) {
        h.key("cmd-f")
        h.compose(s)
        h.wait(0.1)
    }

    // MARK: - How the editor draws (the only part that knows the engine)

    struct Glyph {
        let text: String
        /// Where it is drawn, in the text view, a line high.
        let rect: NSRect
        let lineRect: NSRect
        let baseline: CGFloat
        let xHeight: CGFloat
    }

    struct DrawnLine {
        var glyphs: [Glyph]
        let top: CGFloat
        let bottom: CGFloat
        let left: CGFloat
    }

    private var inputClient: NSTextInputClient {
        (h.win.firstResponder as? NSTextInputClient) ?? h.view
    }

    /// Where the caret is drawn, in the text view: where the text view tells
    /// an input method it is.
    private var caretRect: NSRect {
        let client = inputClient
        let screen = client.firstRect(forCharacterRange: client.selectedRange(), actualRange: nil)
        return h.view.convert(h.win.convertFromScreen(screen), from: nil)
    }

    /// The first occurrence of `s` in the drawn text, line breaks between
    /// drawn lines not counting.
    private func locate(_ s: String) -> [Glyph]? {
        let glyphs = drawnLines.flatMap(\.glyphs)
        let want = Array(s)
        let have = glyphs.map { Character($0.text) }
        guard !want.isEmpty, want.count <= have.count else { return nil }
        for i in 0...(have.count - want.count) where Array(have[i..<(i + want.count)]) == want {
            return Array(glyphs[i..<(i + want.count)])
        }
        return nil
    }

    /// TextKit 1: each line fragment with height is a drawn line; its
    /// glyphs, less null (hidden) ones, are what is drawn on it, and a
    /// newline laid out as a space (Reflow Paragraphs) is a space. Positions
    /// come from glyph locations, which TextKit keeps right where its range
    /// rectangles aren't.
    private var drawnLines: [DrawnLine] {
        let v = h.view
        guard let lm = v.layoutManager, let tc = v.textContainer, let storage = v.textStorage else { return [] }
        lm.ensureLayout(for: tc)
        let s = storage.string as NSString
        let origin = v.textContainerOrigin
        var out: [DrawnLine] = []
        lm.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: lm.numberOfGlyphs)) {
            frag, used, _, range, _ in
            guard frag.height > 0.5 else { return }
            var glyphs: [Glyph] = []
            var g = range.location
            while g < NSMaxRange(range) {
                let ci = lm.characterIndexForGlyph(at: g)
                let seq = s.rangeOfComposedCharacterSequence(at: ci)
                let next = max(g + 1, NSMaxRange(lm.glyphRange(forCharacterRange: seq, actualCharacterRange: nil)))
                defer { g = next }
                let prop = lm.propertyForGlyph(at: g)
                if prop.contains(.null) { continue }
                var text = s.substring(with: seq)
                if text == "\n" {
                    guard v.reflowsParagraphs && !v.sourceMode && v.softBreaks.contains(ci) else { continue }
                    text = " "
                }
                // Up to the next drawn glyph on the line, or the line's end.
                var k = next
                while k < NSMaxRange(range) && lm.propertyForGlyph(at: k).contains(.null) { k += 1 }
                let x0 = frag.minX + lm.location(forGlyphAt: g).x
                let x1 = k < NSMaxRange(range) ? frag.minX + lm.location(forGlyphAt: k).x : used.maxX
                let font =
                    storage.attribute(.font, at: ci, effectiveRange: nil) as? NSFont ?? Theme.font(size: Theme.bodySize)
                let lineRect = frag.offsetBy(dx: origin.x, dy: origin.y)
                glyphs.append(
                    Glyph(
                        text: text,
                        rect: NSRect(
                            x: x0 + origin.x, y: lineRect.minY, width: max(0, x1 - x0), height: lineRect.height),
                        lineRect: lineRect, baseline: lineRect.minY + lm.location(forGlyphAt: g).y,
                        xHeight: font.xHeight))
            }
            let r = frag.offsetBy(dx: origin.x, dy: origin.y)
            // Fragments of one drawn line (none here) would share a top.
            if let last = out.last, abs(last.top - r.minY) < 0.5 {
                out[out.count - 1].glyphs += glyphs
            } else {
                out.append(DrawnLine(glyphs: glyphs, top: r.minY, bottom: r.maxY, left: r.minX + used.minX - frag.minX))
            }
        }
        let extra = lm.extraLineFragmentRect
        if extra.height > 0.5 {
            let r = extra.offsetBy(dx: origin.x, dy: origin.y)
            out.append(DrawnLine(glyphs: [], top: r.minY, bottom: r.maxY, left: r.minX))
        }
        return out
    }

    private func mouse(at p: NSPoint, count: Int = 1, mods: NSEvent.ModifierFlags = []) {
        h.layOut()
        let inWindow = h.view.convert(p, to: nil)
        func event(_ type: NSEvent.EventType, after dt: Double) -> NSEvent? {
            NSEvent.mouseEvent(
                with: type, location: inWindow, modifierFlags: mods,
                timestamp: ProcessInfo.processInfo.systemUptime + dt, windowNumber: h.win.windowNumber, context: nil,
                eventNumber: 1, clickCount: count, pressure: type == .leftMouseUp ? 0 : 1)
        }
        if let up = event(.leftMouseUp, after: 0.05) { NSApp.postEvent(up, atStart: false) }
        guard let down = event(.leftMouseDown, after: 0), let frame = h.win.contentView?.superview,
            let hit = frame.hitTest(frame.convert(inWindow, from: nil))
        else { return }
        hit.mouseDown(with: down)
        NSApp.discardEvents(matching: [.leftMouseUp, .leftMouseDragged], before: nil)
    }
}
