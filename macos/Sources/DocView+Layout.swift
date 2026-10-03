import AppKit
import margin_ffi

/// A shown paragraph of the core's display map (see `Projection`), as
/// `paragraphsPacked` gives it. Positions in UTF-16 units.
struct ShownParagraph {
    struct Piece {
        var kind: PieceKind
        var sourceStart: UInt32
        var sourceEnd: UInt32
        var shownStart: UInt32
        var shownLen: UInt32
        /// What a replaced piece or an object shows as.
        var character: UInt32
    }

    var sourceStart: UInt32
    var sourceEnd: UInt32
    var firstLine: UInt32
    var endLine: UInt32
    var pieces: [Piece]

    var shownLength: Int { pieces.last.map { Int($0.shownStart + $0.shownLen) } ?? 0 }

    static func decode(_ data: Data) -> [ShownParagraph] {
        var out: [ShownParagraph] = []
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            var i = 0
            func next() -> UInt32 {
                defer { i += 4 }
                return UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: i, as: UInt32.self))
            }
            guard raw.count >= 4 else { return }
            let n = Int(next())
            out.reserveCapacity(n)
            for _ in 0..<n {
                let (a, b, f, e, count) = (next(), next(), next(), next(), Int(next()))
                var pieces: [Piece] = []
                pieces.reserveCapacity(count)
                for _ in 0..<count {
                    let kind: PieceKind
                    switch next() {
                    case 0: kind = .shown
                    case 1: kind = .hidden
                    case 2: kind = .replaced
                    default: kind = .object
                    }
                    pieces.append(
                        Piece(
                            kind: kind, sourceStart: next(), sourceEnd: next(), shownStart: next(), shownLen: next(),
                            character: next()))
                }
                out.append(ShownParagraph(sourceStart: a, sourceEnd: b, firstLine: f, endLine: e, pieces: pieces))
            }
        }
        return out
    }
}

/// A shown paragraph laid out.
struct LaidParagraph {
    /// Its text as laid out: the shown text, with the text an input method
    /// is composing in it if that is here.
    let text: NSAttributedString
    /// The composition in it: where, in `text`, and how long.
    let splice: (at: Int, length: Int)?
    let lines: [LaidLine]
    let top: CGFloat
    let bottom: CGFloat
}

/// A line of a laid-out paragraph. String indices of `line` are offsets in
/// the paragraph's text.
struct LaidLine {
    let line: CTLine
    /// Its part of the paragraph's text.
    let range: NSRange
    let x: CGFloat
    /// The line's top, with the paragraph's space above on its first line.
    let top: CGFloat
    /// Its text's top, below that space.
    let textTop: CGFloat
    let baseline: CGFloat
    /// Its bottom, line spacing included.
    let bottom: CGFloat
    let spacing: CGFloat
    let width: CGFloat
}

/// What a typeset paragraph depends on: its source, how it shows (its
/// pieces, relative to its start), how its lines are styled (the styler's
/// signatures), and the width, appearance and text size.
struct TypesetKey: Hashable {
    let source: String
    let pieces: [UInt32]
    let styles: [Int]
    let width: CGFloat
    let appearance: String
    let generation: Int
}

/// What every paragraph's typesetting depends on.
struct TypesetContext: Equatable {
    var width: CGFloat
    var appearance: String
    var generation: Int
}

/// A paragraph typeset: its text, lines and metrics, without positions.
final class Typeset {
    struct Line {
        let line: CTLine
        let range: NSRange
        let ascent: CGFloat
        let descent: CGFloat
        let leading: CGFloat
        let width: CGFloat
    }

    let text: NSAttributedString
    let lines: [Line]
    let indent: CGFloat
    let above: CGFloat
    let spacing: CGFloat

    init(text: NSAttributedString, lines: [Line], indent: CGFloat, above: CGFloat, spacing: CGFloat) {
        self.text = text
        self.lines = lines
        self.indent = indent
        self.above = above
        self.spacing = spacing
    }
}

/// A table laid out as a grid: its columns' left edges and widths, from the
/// text column's left edge.
struct TableGrid {
    var info: TableInfo
    var columnX: [CGFloat]
    var columnWidths: [CGFloat]
    var width: CGFloat {
        columnWidths.reduce(0, +) + 2 * Theme.tableCellPad * Theme.scale * CGFloat(columnWidths.count)
    }
}

/// The size of a laid-out character that isn't text: an object, or a table
/// cell's gap.
nonisolated final class RunBox {
    let ascent: CGFloat
    let descent: CGFloat
    let width: CGFloat

    init(ascent: CGFloat, descent: CGFloat, width: CGFloat) {
        self.ascent = ascent
        self.descent = descent
        self.width = width
    }

    func delegate() -> CTRunDelegate? {
        var callbacks = CTRunDelegateCallbacks(
            version: kCTRunDelegateCurrentVersion,
            dealloc: { Unmanaged<RunBox>.fromOpaque($0).release() },
            getAscent: { Unmanaged<RunBox>.fromOpaque($0).takeUnretainedValue().ascent },
            getDescent: { Unmanaged<RunBox>.fromOpaque($0).takeUnretainedValue().descent },
            getWidth: { Unmanaged<RunBox>.fromOpaque($0).takeUnretainedValue().width })
        return CTRunDelegateCreate(&callbacks, Unmanaged.passRetained(self).toOpaque())
    }
}

extension NSAttributedString.Key {
    static let ctForeground = NSAttributedString.Key(kCTForegroundColorAttributeName as String)
    static let ctRunDelegate = NSAttributedString.Key(kCTRunDelegateAttributeName as String)
}

extension DocView {
    // MARK: - Laying out

    func indent(quotes: UInt8, items: UInt8) -> CGFloat {
        (CGFloat(quotes) * Theme.quoteStep + CGFloat(items) * Theme.itemStep) * Theme.scale
    }

    /// The source position whose style a shown paragraph takes: its first
    /// shown character's, else its first line's start.
    private func styleAnchor(_ p: ShownParagraph) -> Int? {
        if let piece = p.pieces.first(where: { $0.kind != .hidden }) { return Int(piece.sourceStart) }
        if Int(p.firstLine) < lines.count { return Int(lines[Int(p.firstLine)].start) }
        return nil
    }

    /// Lays out every shown paragraph for the page's width. A paragraph
    /// whose source, pieces and styling are as before keeps its typesetting.
    func relayout() {
        let left = geometry.left
        var y = textContainerInset.height
        var out: [LaidParagraph] = []
        out.reserveCapacity(shown.count)
        let composing = marked.map { _ in projection.toShown(pos: UInt32(markedAt)) }
        let appearance = effectiveAppearance.name.rawValue
        var cache: [TypesetKey: Typeset] = [:]
        var typesets: [Typeset?] = []
        typesets.reserveCapacity(shown.count)
        // What was laid out before is reusable for the same page.
        let context = TypesetContext(width: geometry.docWidth, appearance: appearance, generation: Theme.generation)
        if previous.context != context { previous = (context, [], [], [:]) }
        effectiveAppearance.performAsCurrentDrawingAppearance {
            for (i, p) in shown.enumerated() {
                var splice: (at: Int, length: Int)?
                if let m = marked, let c = composing, Int(c.paragraph) == i {
                    splice = (Int(c.offset), (m.text as NSString).length)
                }
                let t: Typeset
                if splice == nil, let old = unchanged(i, p, paragraphDelta: shown.count - previous.shown.count) {
                    t = old
                } else {
                    let key = splice == nil ? typesetKey(p, appearance: appearance) : nil
                    if let key, let hit = typesetCache[key] {
                        t = hit
                    } else {
                        t = typeset(i, p, splice: splice)
                    }
                    if let key { cache[key] = t }
                }
                typesets.append(splice == nil ? t : nil)
                var lines: [LaidLine] = []
                lines.reserveCapacity(t.lines.count)
                let top = y
                y += t.above
                for tl in t.lines {
                    let lineTop = lines.isEmpty ? top : y
                    let baseline = (y + tl.ascent).rounded()
                    let bottom = y + ceil(tl.ascent + tl.descent + tl.leading) + t.spacing
                    lines.append(
                        LaidLine(
                            line: tl.line, range: tl.range, x: left + t.indent, top: lineTop, textTop: y,
                            baseline: baseline, bottom: bottom, spacing: t.spacing, width: tl.width))
                    y = bottom
                }
                out.append(LaidParagraph(text: t.text, splice: splice, lines: lines, top: top, bottom: y))
            }
        }
        typesetCache = cache.merging(typesetCache.count < 4 * cache.count ? typesetCache : [:]) { new, _ in new }
        previous = (context, shown, typesets, tableGaps)
        lastEdit = nil
        restyled = []
        laidOut = out
        layoutHeight = y + textContainerInset.height
        lineShown = Array(repeating: false, count: self.lines.count)
        for p in shown where p.endLine > p.firstLine {
            for li in Int(p.firstLine)..<min(Int(p.endLine), self.lines.count) { lineShown[li] = true }
        }
        sizeToFit()
        updateCaret()
        needsDisplay = true
        window?.invalidateCursorRects(for: self)
        onLayout?()
    }

    /// Paragraph `i`'s typesetting from the last layout, when nothing it
    /// depends on changed: it is the same paragraph as before (before the
    /// last edit, or after it shifted by it), shown the same way, and none
    /// of its lines was restyled.
    private func unchanged(_ i: Int, _ p: ShownParagraph, paragraphDelta: Int) -> Typeset? {
        guard let restyled, !previous.shown.isEmpty else { return nil }
        let j: Int
        let shift: Int
        if let e = lastEdit {
            if Int(p.sourceEnd) < e.range.location {
                (j, shift) = (i, 0)
            } else if Int(p.sourceStart) > NSMaxRange(e.range) {
                (j, shift) = (i - paragraphDelta, e.delta)
            } else {
                return nil
            }
        } else if paragraphDelta == 0 {
            (j, shift) = (i, 0)
        } else {
            return nil
        }
        guard j >= 0, j < previous.shown.count, let t = previous.typesets[j] else { return nil }
        let old = previous.shown[j]
        guard Int(old.sourceStart) + shift == Int(p.sourceStart),
            old.sourceEnd - old.sourceStart == p.sourceEnd - p.sourceStart,
            old.pieces.count == p.pieces.count
        else { return nil }
        for (a, b) in zip(old.pieces, p.pieces) {
            if a.kind != b.kind || a.character != b.character || a.shownLen != b.shownLen
                || a.sourceStart - old.sourceStart != b.sourceStart - p.sourceStart
                || a.sourceEnd - old.sourceStart != b.sourceEnd - p.sourceStart || a.kind == .object
            {
                return nil
            }
            // A table cell's gap reaches the same column as before.
            if a.kind == .replaced && a.character == 9
                && previous.gaps[Int(a.sourceStart)] != tableGaps[Int(b.sourceStart)]
            {
                return nil
            }
        }
        if !restyled.isEmpty {
            let first = lineIndex(of: Int(p.sourceStart))
            let last = lineIndex(of: Int(p.sourceEnd))
            if restyled.contains(integersIn: first...max(first, last))
                || restyled.intersects(integersIn: first...max(first, last))
            {
                return nil
            }
        }
        return t
    }

    /// What paragraph `p`'s typesetting depends on; nil when it holds an
    /// object or a table cell's gap, whose sizes come from elsewhere.
    private func typesetKey(_ p: ShownParagraph, appearance: String) -> TypesetKey? {
        let start = Int(p.sourceStart)
        let end = Int(p.sourceEnd)
        guard end <= text.length else { return nil }
        var pieces: [UInt32] = []
        pieces.reserveCapacity(p.pieces.count * 4)
        for piece in p.pieces {
            if piece.kind == .object { return nil }
            pieces.append(contentsOf: [
                UInt32(piece.kind == .shown ? 0 : piece.kind == .hidden ? 1 : 2),
                piece.sourceStart - p.sourceStart, piece.sourceEnd - p.sourceStart, piece.character,
            ])
            // A table cell's gap: the column it reaches.
            if piece.kind == .replaced && piece.character == 9 {
                pieces.append(Float(tableGaps[Int(piece.sourceStart)] ?? -1).bitPattern)
            }
        }
        // The styled lines it spans, by the signatures the styler leaves on
        // their first characters.
        var styles: [Int] = []
        if !lines.isEmpty, styled.length > 0 {
            let first = lineIndex(of: start)
            let last = lineIndex(of: min(end, text.length))
            for li in first...max(first, last) where li < lines.count {
                let at = Int(lines[li].start)
                if at < styled.length {
                    styles.append(styled.attribute(.marginLineSig, at: at, effectiveRange: nil) as? Int ?? 0)
                }
            }
        }
        return TypesetKey(
            source: text.substring(with: NSRange(location: start, length: end - start)), pieces: pieces,
            styles: styles, width: geometry.docWidth, appearance: appearance, generation: Theme.generation)
    }

    /// The source line holding position `p`.
    func lineIndex(of p: Int) -> Int {
        var lo = 0
        var hi = lines.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if Int(lines[mid].start) <= p { lo = mid + 1 } else { hi = mid }
        }
        return max(0, lo - 1)
    }

    /// Typesets paragraph `i` for the page's width.
    private func typeset(_ i: Int, _ p: ShownParagraph, splice: (at: Int, length: Int)?) -> Typeset {
        let anchor = styleAnchor(p).flatMap { $0 < styled.length ? $0 : nil }
        let style =
            anchor.flatMap { styled.attribute(.paragraphStyle, at: $0, effectiveRange: nil) }
            as? NSParagraphStyle
        let font =
            anchor.flatMap { styled.attribute(.font, at: $0, effectiveRange: nil) as? NSFont }
            ?? Theme.font(size: Theme.bodySize)
        let indent = style?.headIndent ?? 0
        let above = style?.paragraphSpacingBefore ?? 0
        let spacing = style?.lineSpacing ?? max(0, (1.5 * font.pointSize - Theme.naturalHeight(font)).rounded())
        let text = attributedText(i, p, indent: indent, splice: splice, font: font)
        let ts = CTTypesetterCreateWithAttributedString(coreText(text))
        let width = Double(max(1, geometry.docWidth - indent))
        var lines: [Typeset.Line] = []
        var off = 0
        let n = text.length
        repeat {
            let count: Int
            if n == 0 {
                count = 0
            } else if style?.lineBreakMode == .byClipping {
                count = n - off
            } else if style?.lineBreakMode == .byCharWrapping {
                count = max(1, CTTypesetterSuggestClusterBreak(ts, off, width))
            } else {
                count = max(1, CTTypesetterSuggestLineBreak(ts, off, width))
            }
            let line = CTTypesetterCreateLine(ts, CFRange(location: off, length: count))
            var ascent: CGFloat = 0
            var descent: CGFloat = 0
            var leading: CGFloat = 0
            let w = CGFloat(CTLineGetTypographicBounds(line, &ascent, &descent, &leading))
            if count == 0 || n == 0 {
                ascent = font.ascender
                descent = -font.descender
                leading = font.leading
            }
            lines.append(
                Typeset.Line(
                    line: line, range: NSRange(location: off, length: count), ascent: ascent, descent: descent,
                    leading: leading, width: w))
            off += count
        } while off < n
        return Typeset(text: text, lines: lines, indent: indent, above: above, spacing: spacing)
    }

    /// Paragraph `i`'s shown text, with the styled source's attributes: what
    /// is hidden left out, what is replaced as its replacement, objects as
    /// one character as wide as they are drawn, table cells' gaps reaching
    /// their columns.
    private func attributedText(
        _ i: Int, _ p: ShownParagraph, indent: CGFloat, splice: (at: Int, length: Int)?, font: NSFont
    ) -> NSAttributedString {
        let out = NSMutableAttributedString()
        // How far the text has got, for table cells' gaps.
        var x: CGFloat = 0
        func width(_ s: NSAttributedString) -> CGFloat {
            CGFloat(CTLineGetTypographicBounds(CTLineCreateWithAttributedString(coreText(s)), nil, nil, nil))
        }
        for piece in p.pieces {
            let source = NSRange(from: piece.sourceStart, to: piece.sourceEnd)
            switch piece.kind {
            case .hidden:
                continue
            case .shown:
                guard NSMaxRange(source) <= styled.length else { continue }
                let s = styled.attributedSubstring(from: source)
                if !tableGaps.isEmpty { x += width(s) }
                out.append(s)
            case .replaced:
                let t = String(Character(Unicode.Scalar(piece.character) ?? " "))
                var attrs =
                    source.location < styled.length
                    ? styled.attributes(at: source.location, effectiveRange: nil) : [.font: font]
                if t == "\t", let target = tableGaps[source.location] {
                    // To the cell's column, from where the text has got to.
                    // Laid out as an object character: Core Text sets a tab
                    // at its own tab stops, whatever its run delegate says.
                    let w = max(0, target - indent - x)
                    attrs[.ctRunDelegate] = RunBox(ascent: 0, descent: 0, width: w).delegate()
                    x += w
                    out.append(NSAttributedString(string: "\u{FFFC}", attributes: attrs))
                    continue
                } else if !tableGaps.isEmpty {
                    x += width(NSAttributedString(string: t, attributes: attrs))
                }
                out.append(NSAttributedString(string: t, attributes: attrs))
            case .object:
                var attrs: [NSAttributedString.Key: Any] = [.font: font, .marginObject: true]
                if let o = objectAtSource(source.location) {
                    let look = look(of: o)
                    let box: RunBox
                    if case .broken(let b) = look {
                        box = RunBox(ascent: b.ascent, descent: b.descent, width: b.width)
                    } else {
                        box = RunBox(ascent: look.size.height, descent: 0, width: look.size.width)
                    }
                    attrs[.ctRunDelegate] = box.delegate()
                }
                out.append(NSAttributedString(string: "\u{FFFC}", attributes: attrs))
            }
        }
        if let s = splice, let m = marked {
            let at = min(s.at, out.length)
            var attrs = at > 0 ? out.attributes(at: at - 1, effectiveRange: nil) : [.font: font]
            attrs[.ctRunDelegate] = nil
            attrs[.marginObject] = nil
            attrs[.backgroundColor] = nil
            let composing = NSMutableAttributedString(string: m.text, attributes: attrs)
            composing.addAttribute(
                .underlineStyle, value: NSUnderlineStyle.single.rawValue,
                range: NSRange(location: 0, length: composing.length))
            if m.selected.length > 0, NSMaxRange(m.selected) <= composing.length {
                composing.addAttribute(.underlineStyle, value: NSUnderlineStyle.thick.rawValue, range: m.selected)
            }
            out.insert(composing, at: at)
        }
        return out
    }

    /// `text` with Core Text's attributes: colors resolved for the current
    /// appearance; paragraph styles, which Core Text reads differently, and
    /// background colors, which the view draws (flipped, Core Text would
    /// draw them below the text), out.
    private func coreText(_ text: NSAttributedString) -> NSAttributedString {
        let out = NSMutableAttributedString(attributedString: text)
        let all = NSRange(location: 0, length: out.length)
        out.removeAttribute(.paragraphStyle, range: all)
        out.removeAttribute(.backgroundColor, range: all)
        text.enumerateAttribute(.foregroundColor, in: all) { value, r, _ in
            let c = (value as? NSColor) ?? Theme.text
            out.addAttribute(.ctForeground, value: c.cgColor, range: r)
        }
        return out
    }

    /// The object whose source starts at `p`.
    func objectAtSource(_ p: Int) -> DocObject? {
        objects.first { $0.start == p }
    }

    // MARK: - Tables

    /// Sizes each table's columns to their widest cell, and sets where each
    /// cell's text starts.
    func layOutTables() {
        var grids: [TableGrid] = []
        var gaps: [Int: CGFloat] = [:]
        var cache: [String: (grid: TableGrid, gaps: [Int: CGFloat], start: Int)] = [:]
        if !sourceMode {
            let pad = Theme.tableCellPad * Theme.scale
            for t in tableInfos {
                let n = t.aligns.count
                guard n > 0, let first = t.rows.first, Int(first.line) < lines.count else { continue }
                // A table as before (its source, styled the same) keeps its
                // grid; its gaps move with it.
                guard Int(t.lastLine) < lines.count else { continue }
                let start = Int(lines[Int(t.firstLine)].start)
                let key =
                    text.substring(with: NSRange(from: lines[Int(t.firstLine)].start, to: lines[Int(t.lastLine)].end))
                    + "\(Theme.generation) \(lines[Int(first.line)].quotes) \(lines[Int(first.line)].items)"
                if let hit = tableCache[key] {
                    grids.append(TableGrid(info: t, columnX: hit.grid.columnX, columnWidths: hit.grid.columnWidths))
                    for (k, v) in hit.gaps { gaps[k - hit.start + start] = v }
                    cache[key] = hit
                    continue
                }
                var tableGapsHere: [Int: CGFloat] = [:]
                var widths = [CGFloat](repeating: 2 * pad, count: n)
                var cellWidths: [[CGFloat]] = []
                for row in t.rows {
                    let ws = row.cells.prefix(n).map { shownWidth(NSRange($0.content)) }
                    for (j, w) in ws.enumerated() { widths[j] = max(widths[j], w) }
                    cellWidths.append(ws)
                }
                let l = lines[Int(first.line)]
                var x = indent(quotes: l.quotes, items: l.items)
                var columnX: [CGFloat] = []
                for w in widths {
                    columnX.append(x)
                    x += w + 2 * pad
                }
                for (row, ws) in zip(t.rows, cellWidths) {
                    for (j, cell) in row.cells.prefix(n).enumerated() where cell.lead.end > cell.lead.start {
                        let slack = widths[j] - ws[j]
                        let offset: CGFloat
                        switch t.aligns[j] {
                        case .right: offset = slack
                        case .center: offset = (slack / 2).rounded()
                        default: offset = 0
                        }
                        tableGapsHere[Int(cell.lead.end) - 1] = columnX[j] + pad + offset
                    }
                }
                let grid = TableGrid(info: t, columnX: columnX, columnWidths: widths)
                grids.append(grid)
                gaps.merge(tableGapsHere) { a, _ in a }
                cache[key] = (grid, tableGapsHere, start)
            }
        }
        tables = grids
        tableGaps = gaps
        tableCache = cache
    }

    /// The width of a range's shown text.
    private func shownWidth(_ r: NSRange) -> CGFloat {
        guard r.length > 0, NSMaxRange(r) <= styled.length else { return 0 }
        var w: CGFloat = 0
        let at = projection.toShown(pos: UInt32(r.location))
        guard Int(at.paragraph) < shown.count else { return 0 }
        for piece in shown[Int(at.paragraph)].pieces where piece.kind == .shown {
            let s = NSIntersectionRange(NSRange(from: piece.sourceStart, to: piece.sourceEnd), r)
            if s.length > 0 { w += styled.attributedSubstring(from: s).size().width }
        }
        return ceil(w)
    }

    // MARK: - Where things are

    /// Where source position `p` is laid out: a paragraph, a line in it, and
    /// an offset in its laid-out text.
    func laid(_ p: Int) -> (paragraph: Int, line: Int, offset: Int)? {
        guard !laidOut.isEmpty else { return nil }
        let at = projection.toShown(pos: UInt32(min(max(0, p), text.length)))
        let pi = min(Int(at.paragraph), laidOut.count - 1)
        var o = Int(at.offset)
        if let s = laidOut[pi].splice, o > s.at || (o == s.at && p > markedAt) { o += s.length }
        return (pi, lineIndex(pi, offset: o), o)
    }

    /// The line of paragraph `pi` holding offset `o`: at a wrap, the line
    /// after it.
    func lineIndex(_ pi: Int, offset o: Int) -> Int {
        let ls = laidOut[pi].lines
        for (i, l) in ls.enumerated() where o < NSMaxRange(l.range) || i == ls.count - 1 {
            return i
        }
        return 0
    }

    /// The x of offset `o` on line `l`.
    func x(_ o: Int, on l: LaidLine) -> CGFloat {
        l.x + CTLineGetOffsetForStringIndex(l.line, o, nil)
    }

    /// One rectangle per line for a range of paragraph `pi`'s laid-out text.
    func rects(paragraph pi: Int, from a: Int, to b: Int, toEdge: Bool = false) -> [NSRect] {
        var out: [NSRect] = []
        let ls = laidOut[pi].lines
        for (i, l) in ls.enumerated() {
            let lo = max(a, l.range.location)
            let hi = min(b, NSMaxRange(l.range))
            let last = i == ls.count - 1
            if lo > hi || (lo == hi && !(toEdge && last && b >= NSMaxRange(l.range))) { continue }
            let x0 = x(lo, on: l)
            var x1 = x(hi, on: l)
            if toEdge && last && b >= NSMaxRange(l.range) { x1 = max(x1, geometry.left + geometry.docWidth) }
            out.append(
                NSRect(
                    x: min(x0, x1), y: l.textTop, width: abs(x1 - x0), height: l.bottom - l.spacing - l.textTop))
        }
        return out
    }

    /// Where a source range is drawn, a rectangle per line; with `toEdge`,
    /// a range going on past a paragraph's end reaches the column's edge
    /// there, as a selection does.
    func rects(forSource r: NSRange, toEdge: Bool = false) -> [NSRect] {
        guard let a = laid(r.location), let b = laid(NSMaxRange(r)) else { return [] }
        var out: [NSRect] = []
        for pi in a.paragraph...max(a.paragraph, b.paragraph) {
            let from = pi == a.paragraph ? a.offset : 0
            let to = pi == b.paragraph ? b.offset : laidOut[pi].text.length
            out += rects(paragraph: pi, from: from, to: to, toEdge: toEdge && pi < b.paragraph)
        }
        return out
    }

    /// Where a range of the input method's text is drawn.
    func clientRects(_ r: NSRange) -> [NSRect] {
        guard let m = marked, let at = laid(markedAt), let s = laidOut[at.paragraph].splice else {
            return rects(forSource: r)
        }
        let n = (m.text as NSString).length
        if r.location >= markedAt && NSMaxRange(r) <= markedAt + n {
            let a = s.at + r.location - markedAt
            return rects(paragraph: at.paragraph, from: a, to: a + r.length)
        }
        return rects(forSource: NSRange(location: sourcePositionOfClient(r.location), length: 0))
    }

    private func sourcePositionOfClient(_ p: Int) -> Int {
        guard let m = marked else { return p }
        let n = (m.text as NSString).length
        return p <= markedAt ? p : (p <= markedAt + n ? markedAt : p - n)
    }

    /// Where the caret is drawn.
    func caretRect() -> NSRect {
        let line: LaidLine
        let o: Int
        if let m = marked, let at = laid(markedAt), let s = laidOut[at.paragraph].splice {
            o = s.at + m.selected.location
            line = laidOut[at.paragraph].lines[lineIndex(at.paragraph, offset: o)]
        } else if let at = laid(head) {
            o = at.offset
            line = laidOut[at.paragraph].lines[at.line]
        } else {
            return NSRect(x: geometry.left, y: textContainerInset.height, width: 1, height: Theme.bodySize)
        }
        let h = line.bottom - line.spacing - line.textTop
        return NSRect(x: x(o, on: line), y: line.textTop, width: 1, height: h)
    }

    func updateCaret() {
        let r = caretRect()
        caretView.frame = NSRect(x: r.minX - 1, y: r.minY, width: 2, height: r.height)
        let focused = window?.firstResponder === self
        caretView.displayMode = focused && (selection == nil || marked != nil) ? .automatic : .hidden
    }

    /// The paragraph laid out at y, or the nearest.
    private func paragraph(atY y: CGFloat) -> Int? {
        guard !laidOut.isEmpty else { return nil }
        var lo = 0
        var hi = laidOut.count - 1
        while lo < hi {
            let mid = (lo + hi + 1) / 2
            if laidOut[mid].top <= y { lo = mid } else { hi = mid - 1 }
        }
        return lo
    }

    /// The source position nearest a point: where a click puts the cursor.
    func position(at p: NSPoint) -> Int {
        guard let pi = paragraph(atY: p.y) else { return 0 }
        let ls = laidOut[pi].lines
        let li = ls.lastIndex { $0.top <= p.y } ?? 0
        let l = ls[li]
        var o = CTLineGetStringIndexForPosition(l.line, CGPoint(x: p.x - l.x, y: 0))
        if o == kCFNotFound { o = l.range.location }
        o = min(max(o, l.range.location), NSMaxRange(l.range))
        // A click past a wrapped line's end goes before its last space.
        if o == NSMaxRange(l.range), li < ls.count - 1, o > l.range.location,
            let c = (laidOut[pi].text.string as NSString).substring(with: NSRange(location: o - 1, length: 1)).first,
            c.isWhitespace
        {
            o -= 1
        }
        return source(paragraph: pi, offset: o)
    }

    /// The source position for an offset in a laid-out paragraph.
    func source(paragraph pi: Int, offset o: Int) -> Int {
        var o = o
        if let s = laidOut[pi].splice {
            if o > s.at + s.length { o -= s.length } else if o > s.at { return markedAt }
        }
        return Int(projection.toSource(at: ShownPos(paragraph: UInt32(pi), offset: UInt32(o))))
    }

    /// Source lines drawn in `rect`, with a line of margin.
    func lines(in rect: NSRect) -> ClosedRange<Int> {
        guard !lines.isEmpty, let a = paragraph(atY: rect.minY), let b = paragraph(atY: rect.maxY) else {
            return 0...0
        }
        let first = Int(shown[a].firstLine)
        let last = max(first, Int(shown[b].endLine) - 1)
        return max(0, first - 1)...min(lines.count - 1, last + 1)
    }

    /// Whether line `li` shows nothing (a hidden blank line, a fence).
    func isCollapsed(line li: Int) -> Bool {
        li < lineShown.count && !lineShown[li] && !lines.isEmpty
    }

    /// The laid-out line holding a source line's first shown character.
    private func laidLine(startingLine li: Int) -> LaidLine? {
        guard li < lines.count, let at = laid(Int(lines[li].visibleStart)) else { return nil }
        return laidOut[at.paragraph].lines[at.line]
    }

    /// The rectangle of the laid-out line holding character `ci`.
    func fragmentRect(at ci: Int) -> NSRect {
        guard let at = laid(ci) else {
            return NSRect(x: geometry.left, y: textContainerInset.height, width: geometry.docWidth, height: 0)
        }
        let l = laidOut[at.paragraph].lines[at.line]
        return NSRect(x: geometry.left, y: l.top, width: geometry.docWidth, height: l.bottom - l.top)
    }

    /// Where a character sits: its line's top (below any space above) and
    /// height.
    func location(of ci: Int) -> NSRect {
        guard let at = laid(ci) else { return fragmentRect(at: ci) }
        let l = laidOut[at.paragraph].lines[at.line]
        return NSRect(x: geometry.left, y: l.textTop, width: geometry.docWidth, height: l.bottom - l.textTop)
    }

    /// The top of a line's text, below the space above it.
    func textTop(line li: Int) -> CGFloat {
        laidLine(startingLine: li)?.textTop ?? fragmentRect(at: Int(lines[li].start)).minY
    }

    /// The bottom of a line's last laid-out line, without its line spacing.
    func textBottom(line li: Int) -> CGFloat {
        guard let at = laid(Int(lines[li].end)) else { return 0 }
        let l = laidOut[at.paragraph].lines[at.line]
        return l.bottom - l.spacing
    }

    /// Where a list item's marker goes: its text's baseline and left edge.
    /// Beside an image, the baseline of a line of text at its top.
    func markerPosition(_ it: ItemInfo) -> (baseline: CGFloat, xText: CGFloat) {
        let font = Theme.font(size: Theme.bodySize)
        let x = geometry.left + indent(quotes: it.quotes, items: it.items)
        if let i = objectOnLine[Int(it.line)], case let look = look(of: objects[i]), !look.isText,
            let r = objectRect(objects[i], look: look)
        {
            return (r.minY + font.ascender, x)
        }
        guard let l = laidLine(startingLine: Int(it.line)) else { return (font.ascender, x) }
        return (l.baseline, x)
    }

    /// A task item's drawn checkbox.
    func checkboxBox(_ it: ItemInfo) -> NSRect {
        let font = Theme.font(size: Theme.bodySize)
        let (baseline, xText) = markerPosition(it)
        let size = (14 * Theme.scale).rounded()
        return NSRect(
            x: xText - size - 8 * Theme.scale, y: (baseline - font.xHeight / 2 - size / 2).rounded(), width: size,
            height: size)
    }

    /// Where image `o` is drawn.
    func objectRect(_ o: DocObject, look: ObjectLook) -> NSRect? {
        guard let at = laid(o.start) else { return nil }
        let l = laidOut[at.paragraph].lines[at.line]
        let x = x(at.offset, on: l)
        switch look {
        case .image, .placeholder, .diagramError:
            return NSRect(origin: NSPoint(x: x, y: l.textTop), size: look.size)
        case .broken(let b):
            return NSRect(x: x, y: l.baseline - b.ascent, width: b.width, height: b.ascent + b.descent)
        }
    }

    /// The image drawn under a point, if any.
    func object(atPoint p: NSPoint) -> DocObject? {
        guard !sourceMode else { return nil }
        return objects.first { o in objectRect(o, look: look(of: o))?.contains(p) ?? false }
    }

    /// The task whose checkbox is under `p`, as an index for `toggleTask`.
    func task(at p: NSPoint) -> UInt32? {
        guard !sourceMode, !lines.isEmpty else { return nil }
        let shownLines = lines(in: NSRect(x: 0, y: p.y - 20, width: 1, height: 40))
        for it in items where it.task != nil && shownLines.contains(Int(it.line)) {
            if checkboxBox(it).insetBy(dx: -4, dy: -4).contains(p) { return analysis.taskOnLine(line: it.line) }
        }
        return nil
    }

    /// The destination of the link under a point, if any.
    func link(at p: NSPoint) -> String? {
        guard let pi = paragraph(atY: p.y) else { return nil }
        let ls = laidOut[pi].lines
        guard let l = ls.last(where: { $0.top <= p.y }), p.x >= l.x, p.x <= l.x + l.width, p.y <= l.bottom else {
            return nil
        }
        let c = position(at: p)
        return analysis.linkAt(pos: UInt32(c)) ?? analysis.linkAt(pos: UInt32(max(0, c - 1)))
    }

    // MARK: - The page

    /// Sets the page's geometry, from `PageView`.
    func setPage(_ g: PageGeometry, force: Bool = false) {
        if force || g.left != geometry.left || g.docWidth != geometry.docWidth || g.gutterX != geometry.gutterX {
            geometry = g
            relayout()
        }
    }

    func updateGeometry(force: Bool) {
        if usesFullWidth {
            setPage(PageGeometry(fullWidth: bounds.width), force: force)
        } else {
            onRetile?()
        }
    }

    func sizeToFit() {
        let h = max(layoutHeight, minSize.height)
        if frame.height != h { setFrameSize(NSSize(width: frame.width, height: h)) }
    }

    func scrollRangeToVisible(_ r: NSRange) {
        let rect = (rects(forSource: r).first ?? caretRect()).insetBy(dx: 0, dy: -8)
        scrollToVisible(rect)
    }

    func showFindIndicator(for r: NSRange) {
        scrollRangeToVisible(r)
    }

    // MARK: - Moving the cursor

    /// Moves the cursor to `p`, or the selection's moving end with
    /// `extending`.
    private func moveHead(to p: Int, extending: Bool, keepGoal: Bool = false) {
        select(anchor: extending ? anchor : p, head: p, keepGoal: keepGoal)
        scrollRangeToVisible(NSRange(location: head, length: 0))
    }

    /// Right (`forward`) or Left: one shown character over. Toward an image
    /// it selects it; from a selected image, past it.
    private func step(forward: Bool, extending: Bool) {
        // From a selected image, Shift extends from its far side.
        if extending, let o = selectedObject {
            let to = Int(projection.step(pos: UInt32(forward ? o.end : o.start), forward: forward))
            return select(anchor: forward ? o.start : o.end, head: to)
        }
        if !extending, let o = selectedObject {
            let from = forward ? o.end : o.start
            let to = Int(projection.step(pos: UInt32(from), forward: forward))
            return moveHead(to: to == from ? from : to, extending: false)
        }
        if !extending, let sel = selection {
            return moveHead(to: forward ? NSMaxRange(sel) : sel.location, extending: false)
        }
        let to = Int(projection.step(pos: UInt32(head), forward: forward))
        if !extending, let o = objects.first(where: { forward ? $0.start == to : $0.end == to }),
            forward ? head < o.start : head > o.end
        {
            return select(anchor: o.start, head: o.end)
        }
        // Shift extends over an image whole.
        if extending, let o = objects.first(where: { forward ? $0.start == to : $0.end == to }) {
            return moveHead(to: forward ? o.end : o.start, extending: true)
        }
        if extending, let o = object(at: to), to > o.start, to < o.end {
            return moveHead(to: forward ? o.end : o.start, extending: true)
        }
        moveHead(to: to, extending: extending)
    }

    @objc override func moveLeft(_ sender: Any?) { step(forward: false, extending: false) }
    @objc override func moveRight(_ sender: Any?) { step(forward: true, extending: false) }
    @objc override func moveBackward(_ sender: Any?) { step(forward: false, extending: false) }
    @objc override func moveForward(_ sender: Any?) { step(forward: true, extending: false) }
    @objc override func moveLeftAndModifySelection(_ sender: Any?) { step(forward: false, extending: true) }
    @objc override func moveRightAndModifySelection(_ sender: Any?) { step(forward: true, extending: true) }
    @objc override func moveBackwardAndModifySelection(_ sender: Any?) { step(forward: false, extending: true) }
    @objc override func moveForwardAndModifySelection(_ sender: Any?) { step(forward: true, extending: true) }

    /// Up or Down: to the line above or below, keeping the column, past
    /// images.
    private func vertical(down: Bool, extending: Bool) {
        if !extending, let sel = selection, selectedObject == nil {
            select(anchor: sel.location, head: sel.location)
        }
        guard let at = laid(head) else { return }
        let caret = laidOut[at.paragraph].lines[at.line]
        let gx = goalX ?? x(at.offset, on: caret)
        var pi = at.paragraph
        var li = at.line
        while true {
            if down {
                li += 1
                if li >= laidOut[pi].lines.count {
                    pi += 1
                    li = 0
                }
                if pi >= laidOut.count {
                    return moveHead(to: text.length, extending: extending)
                }
            } else {
                li -= 1
                if li < 0 {
                    pi -= 1
                    if pi < 0 { return moveHead(to: source(paragraph: 0, offset: 0), extending: extending) }
                    li = laidOut[pi].lines.count - 1
                }
            }
            let l = laidOut[pi].lines[li]
            var o = CTLineGetStringIndexForPosition(l.line, CGPoint(x: gx - l.x, y: 0))
            if o == kCFNotFound { o = l.range.location }
            o = min(max(o, l.range.location), NSMaxRange(l.range))
            if o == NSMaxRange(l.range), li < laidOut[pi].lines.count - 1, o > l.range.location { o -= 1 }
            let p = source(paragraph: pi, offset: o)
            // Up and Down pass an image, keeping the column.
            if let obj = object(at: p), objects.contains(where: { $0.start == obj.start }) {
                let only = laidOut[pi].text.string.trimmingCharacters(in: .whitespaces) == "\u{FFFC}"
                if only { continue }
            }
            goalX = gx
            select(anchor: extending ? anchor : p, head: p, keepGoal: true)
            scrollRangeToVisible(NSRange(location: head, length: 0))
            return
        }
    }

    @objc override func moveUp(_ sender: Any?) { vertical(down: false, extending: false) }
    @objc override func moveDown(_ sender: Any?) { vertical(down: true, extending: false) }
    @objc override func moveUpAndModifySelection(_ sender: Any?) { vertical(down: false, extending: true) }
    @objc override func moveDownAndModifySelection(_ sender: Any?) { vertical(down: true, extending: true) }

    /// The start or end of the laid-out line holding `p`.
    func lineEdge(from p: Int, end: Bool) -> Int {
        guard let at = laid(p) else { return p }
        let ls = laidOut[at.paragraph].lines
        let l = ls[at.line]
        var o = end ? NSMaxRange(l.range) : l.range.location
        if end, at.line < ls.count - 1, o > l.range.location,
            (laidOut[at.paragraph].text.string as NSString).character(at: o - 1) == 32
        {
            o -= 1
        }
        return source(paragraph: at.paragraph, offset: o)
    }

    /// The next word boundary from `p` in the shown text, over paragraph
    /// ends.
    func wordBoundary(from p: Int, forward: Bool) -> Int {
        guard let at = laid(p) else { return p }
        let t = laidOut[at.paragraph].text
        if forward && at.offset >= t.length {
            return at.paragraph + 1 < laidOut.count ? source(paragraph: at.paragraph + 1, offset: 0) : text.length
        }
        if !forward && at.offset == 0 {
            return at.paragraph > 0
                ? source(paragraph: at.paragraph - 1, offset: laidOut[at.paragraph - 1].text.length) : 0
        }
        var o = t.nextWord(from: at.offset, forward: forward)
        if forward {
            // To the word's end, as the arrows go on macOS.
            let s = t.string as NSString
            o = at.offset
            while o < s.length && !isWordCharacter(s.character(at: o)) { o += 1 }
            while o < s.length && isWordCharacter(s.character(at: o)) { o += 1 }
        }
        return source(paragraph: at.paragraph, offset: o)
    }

    private func isWordCharacter(_ c: unichar) -> Bool {
        guard let u = Unicode.Scalar(c) else { return true }
        return CharacterSet.alphanumerics.contains(u) || c == 0x27 || c == 0x2019
    }

    @objc override func moveWordLeft(_ sender: Any?) {
        moveHead(to: wordBoundary(from: head, forward: false), extending: false)
    }
    @objc override func moveWordRight(_ sender: Any?) {
        moveHead(to: wordBoundary(from: head, forward: true), extending: false)
    }
    @objc override func moveWordBackward(_ sender: Any?) { moveWordLeft(sender) }
    @objc override func moveWordForward(_ sender: Any?) { moveWordRight(sender) }
    @objc override func moveWordLeftAndModifySelection(_ sender: Any?) {
        moveHead(to: wordBoundary(from: head, forward: false), extending: true)
    }
    @objc override func moveWordRightAndModifySelection(_ sender: Any?) {
        moveHead(to: wordBoundary(from: head, forward: true), extending: true)
    }
    @objc override func moveWordBackwardAndModifySelection(_ sender: Any?) { moveWordLeftAndModifySelection(sender) }
    @objc override func moveWordForwardAndModifySelection(_ sender: Any?) { moveWordRightAndModifySelection(sender) }

    @objc override func moveToBeginningOfLine(_ sender: Any?) {
        moveHead(to: lineEdge(from: head, end: false), extending: false)
    }
    @objc override func moveToEndOfLine(_ sender: Any?) {
        moveHead(to: lineEdge(from: head, end: true), extending: false)
    }
    @objc override func moveToLeftEndOfLine(_ sender: Any?) { moveToBeginningOfLine(sender) }
    @objc override func moveToRightEndOfLine(_ sender: Any?) { moveToEndOfLine(sender) }
    @objc override func moveToBeginningOfLineAndModifySelection(_ sender: Any?) {
        moveHead(to: lineEdge(from: head, end: false), extending: true)
    }
    @objc override func moveToEndOfLineAndModifySelection(_ sender: Any?) {
        moveHead(to: lineEdge(from: head, end: true), extending: true)
    }
    @objc override func moveToLeftEndOfLineAndModifySelection(_ sender: Any?) {
        moveToBeginningOfLineAndModifySelection(sender)
    }
    @objc override func moveToRightEndOfLineAndModifySelection(_ sender: Any?) {
        moveToEndOfLineAndModifySelection(sender)
    }

    /// The start or end of the shown paragraph holding `p`.
    private func paragraphEdge(from p: Int, end: Bool) -> Int {
        guard let at = laid(p) else { return p }
        return source(paragraph: at.paragraph, offset: end ? laidOut[at.paragraph].text.length : 0)
    }

    @objc override func moveToBeginningOfParagraph(_ sender: Any?) {
        moveHead(to: paragraphEdge(from: head, end: false), extending: false)
    }
    @objc override func moveToEndOfParagraph(_ sender: Any?) {
        moveHead(to: paragraphEdge(from: head, end: true), extending: false)
    }
    @objc override func moveToBeginningOfParagraphAndModifySelection(_ sender: Any?) {
        moveHead(to: paragraphEdge(from: head, end: false), extending: true)
    }
    @objc override func moveToEndOfParagraphAndModifySelection(_ sender: Any?) {
        moveHead(to: paragraphEdge(from: head, end: true), extending: true)
    }
    @objc override func moveParagraphBackwardAndModifySelection(_ sender: Any?) {
        moveToBeginningOfParagraphAndModifySelection(sender)
    }
    @objc override func moveParagraphForwardAndModifySelection(_ sender: Any?) {
        moveToEndOfParagraphAndModifySelection(sender)
    }

    private var documentStart: Int { laidOut.isEmpty ? 0 : source(paragraph: 0, offset: 0) }

    @objc override func moveToBeginningOfDocument(_ sender: Any?) { moveHead(to: documentStart, extending: false) }
    @objc override func moveToEndOfDocument(_ sender: Any?) { moveHead(to: text.length, extending: false) }
    @objc override func moveToBeginningOfDocumentAndModifySelection(_ sender: Any?) {
        moveHead(to: documentStart, extending: true)
    }
    @objc override func moveToEndOfDocumentAndModifySelection(_ sender: Any?) {
        moveHead(to: text.length, extending: true)
    }

    @objc override func scrollToBeginningOfDocument(_ sender: Any?) { scroll(NSPoint(x: 0, y: 0)) }
    @objc override func scrollToEndOfDocument(_ sender: Any?) {
        scroll(NSPoint(x: 0, y: max(0, bounds.height - visibleRect.height)))
    }

    @objc override func scrollPageUp(_ sender: Any?) {
        scroll(NSPoint(x: 0, y: max(0, visibleRect.minY - visibleRect.height)))
    }
    @objc override func scrollPageDown(_ sender: Any?) {
        scroll(NSPoint(x: 0, y: min(max(0, bounds.height - visibleRect.height), visibleRect.minY + visibleRect.height)))
    }

    @objc override func pageUp(_ sender: Any?) { page(down: false, extending: false) }
    @objc override func pageDown(_ sender: Any?) { page(down: true, extending: false) }
    @objc override func pageUpAndModifySelection(_ sender: Any?) { page(down: false, extending: true) }
    @objc override func pageDownAndModifySelection(_ sender: Any?) { page(down: true, extending: true) }

    private func page(down: Bool, extending: Bool) {
        let c = caretRect()
        let h = max(visibleRect.height - 40, 40)
        let p = position(at: NSPoint(x: c.minX, y: c.midY + (down ? h : -h)))
        moveHead(to: p, extending: extending)
    }

    // MARK: - The mouse

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let p = convert(event.locationInWindow, from: nil)
        if let item = task(at: p) {
            apply(analysis.toggleTask(item: item, cursor: UInt32(cursor)))
            return
        }
        if event.modifierFlags.contains(.command), !sourceMode, let url = link(at: p) {
            onOpenLink?(url)
            return
        }
        // A click selects an image; Shift-click extends over it whole.
        if let o = object(atPoint: p) {
            let sel = sourceSelection
            setSelectedRange(event.modifierFlags.contains(.shift) ? NSUnionRange(sel, o.range) : o.range)
            return
        }
        let at = position(at: p)
        let start: Int
        switch event.clickCount {
        case 2:
            let w = analysis.wordAt(pos: UInt32(at)).map(NSRange.init) ?? NSRange(location: at, length: 0)
            select(anchor: w.location, head: NSMaxRange(w))
            start = w.location
        case 3...:
            select(anchor: paragraphEdge(from: at, end: false), head: paragraphEdge(from: at, end: true))
            start = anchor
        default:
            if event.modifierFlags.contains(.shift) {
                select(anchor: anchor, head: at)
                start = anchor
            } else {
                select(anchor: at, head: at)
                start = at
            }
        }
        // Dragging selects from where the press was.
        guard event.clickCount <= 1, let window else { return }
        while let e = window.nextEvent(matching: [.leftMouseDragged, .leftMouseUp]) {
            if e.type == .leftMouseUp { break }
            let q = position(at: convert(e.locationInWindow, from: nil))
            select(anchor: start, head: q)
            autoscroll(with: e)
        }
        // A selection over an image takes it whole.
        let sel = sourceSelection
        var a = sel.location
        var b = NSMaxRange(sel)
        if let o = object(at: a), a > o.start, a < o.end { a = o.start }
        if let o = object(at: b), b > o.start, b < o.end { b = o.end }
        if a != sel.location || b != NSMaxRange(sel) {
            select(anchor: anchor <= head ? a : b, head: anchor <= head ? b : a)
        }
    }
}
