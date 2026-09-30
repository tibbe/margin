import AppKit

extension NSAttributedString.Key {
    /// Markdown syntax the rendered view hides (drawn as null glyphs).
    static let marginHidden = NSAttributedString.Key("marginHidden")
    /// On a line's first character: the signature of the styling applied to
    /// the line, so unchanged lines are left alone.
    static let marginLineSig = NSAttributedString.Key("marginLineSig")
    /// The one character left of a table cell's padding and `|`, laid out
    /// as space wide enough to reach the cell's column.
    static let marginTableGap = NSAttributedString.Key("marginTableGap")
}

extension NSRange {
    init(_ r: TextRange) {
        self.init(location: Int(r.start), length: Int(r.end) - Int(r.start))
    }

    init(from start: UInt32, to end: UInt32) {
        self.init(location: Int(start), length: max(0, Int(end) - Int(start)))
    }
}

/// A style span as the core packs it (see `Analysis.spansPacked`).
struct Span {
    var start: Int
    var end: Int
    var code: Code
    var param: UInt32

    enum Code: UInt32 {
        case para, heading, codeBlock, fence, table, htmlBlock, frontMatter, rule, raw, quote
        case indent, above, strong, emphasis, strike, code, link, image, inlineHtml, tableHeader, taskDone, hidden
        /// Not from the core: added by the view for tables laid out as grids.
        case tableGap
    }

    static func decode(_ data: Data) -> [Span] {
        var out: [Span] = []
        out.reserveCapacity(data.count / 16)
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let n = raw.count / 16
            for i in 0..<n {
                let base = i * 16
                let start = UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: base, as: UInt32.self))
                let end = UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: base + 4, as: UInt32.self))
                let code = UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: base + 8, as: UInt32.self))
                let param = UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: base + 12, as: UInt32.self))
                if let c = Code(rawValue: code), end > start {
                    out.append(Span(start: Int(start), end: Int(end), code: c, param: param))
                }
            }
        }
        return out
    }
}

extension LineInfo {
    private static let kinds: [LineType] = [
        .blank, .paragraph, .heading, .setextUnderline, .codeContent, .fence,
        .table, .html, .frontMatter, .rule, .raw,
    ]

    /// Lines as the core packs them (see `Analysis.linesPacked`).
    static func decode(_ data: Data) -> [LineInfo] {
        var out: [LineInfo] = []
        out.reserveCapacity(data.count / 28)
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            func u(_ i: Int, _ k: Int) -> UInt32 {
                UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: i * 28 + k * 4, as: UInt32.self))
            }
            for i in 0..<(raw.count / 28) {
                out.append(
                    LineInfo(
                        start: u(i, 0), end: u(i, 1), contentStart: u(i, 2), visibleStart: u(i, 3),
                        kind: kinds[min(Int(u(i, 4)), kinds.count - 1)],
                        quotes: UInt8(clamping: u(i, 5)), items: UInt8(clamping: u(i, 6))))
            }
        }
        return out
    }
}

/// What the view needs to know about a line besides its attributes.
struct LineMetrics {
    /// Space above the line's text (from `Above` spans), in points.
    var spaceAbove: CGFloat
    var lineSpacing: CGFloat
}

/// Applies the analysis's style spans to the text storage, one line at a
/// time. Lines an edit touched are always restyled; other lines only when
/// the spans over them changed.
final class Styler {
    struct Options: Hashable {
        var sourceMode: Bool
        /// Character ranges whose hidden syntax is shown (the blank line the
        /// cursor is on, fences of the code block it is in).
        var revealed: [NSRange]
        var generation: Int
    }

    private(set) var metrics: [LineMetrics] = []

    /// Restyles lines of `storage`: those intersecting `dirty` (edited since
    /// the last call) and those whose spans changed.
    func apply(lines: [LineInfo], spans: [Span], dirty: NSRange?, to storage: NSTextStorage, options: Options) {
        let length = storage.length
        let n = lines.count
        guard n > 0 else { metrics = []; return }
        let starts = lines.map { Int($0.start) }
        func lineOf(_ p: Int) -> Int {
            var lo = 0
            var hi = n
            while lo < hi {
                let mid = (lo + hi) / 2
                if starts[mid] <= p { lo = mid + 1 } else { hi = mid }
            }
            return max(0, lo - 1)
        }
        // Bucket spans by line (a counting sort, spans may cover several).
        var counts = [Int](repeating: 0, count: n + 1)
        var ranges: [(Int, Int)] = []
        ranges.reserveCapacity(spans.count)
        for s in spans {
            let a = lineOf(s.start)
            var b = lineOf(max(s.start, s.end - 1))
            b = max(a, b)
            ranges.append((a, b))
            for l in a...b { counts[l + 1] += 1 }
        }
        for i in 0..<n { counts[i + 1] += counts[i] }
        var fill = counts
        var bucket = [Int](repeating: 0, count: counts[n])
        for (si, (a, b)) in ranges.enumerated() {
            for l in a...b {
                bucket[fill[l]] = si
                fill[l] += 1
            }
        }
        var dirtyLines = 0..<0
        if let d = dirty {
            let a = lineOf(d.location)
            let b = lineOf(min(NSMaxRange(d), max(length - 1, 0)))
            dirtyLines = a..<(max(a, b) + 1)
        }

        var metrics: [LineMetrics] = []
        metrics.reserveCapacity(n)
        storage.beginEditing()
        for (i, line) in lines.enumerated() {
            let start = Int(line.start)
            let end = min(Int(line.end) + 1, length)
            let lineSpans = bucket[counts[i]..<counts[i + 1]].map { spans[$0] }
            let block = lineStyle(lineSpans, sourceMode: options.sourceMode)
            metrics.append(LineMetrics(spaceAbove: block.above, lineSpacing: block.spacing))
            guard start < end else { continue }
            let range = NSRange(location: start, length: end - start)
            let revealed = options.revealed.filter { NSIntersectionRange($0, range).length > 0 }
            var h = Hasher()
            h.combine(range.length)
            h.combine(options.sourceMode)
            h.combine(options.generation)
            for r in revealed { h.combine(r.location - start); h.combine(r.length) }
            for s in lineSpans {
                h.combine(s.start - start)
                h.combine(s.end - start)
                h.combine(s.code)
                h.combine(s.param)
            }
            let sig = h.finalize()
            if !dirtyLines.contains(i),
                let old = storage.attribute(.marginLineSig, at: start, effectiveRange: nil) as? Int, old == sig
            {
                continue
            }
            for (r, attrs) in runs(
                range: range, spans: lineSpans, block: block, revealed: revealed, sourceMode: options.sourceMode)
            where r.length > 0 {
                storage.setAttributes(attrs, range: r)
            }
            storage.addAttribute(.marginLineSig, value: sig, range: NSRange(location: start, length: 1))
        }
        storage.endEditing()
        self.metrics = metrics
    }

    /// Styling that covers a whole line.
    private struct LineStyle {
        var size: CGFloat = 1
        var weight: NSFont.Weight = .regular
        var mono = false
        var height: CGFloat = 1.5
        var charWrap = false
        /// A table row laid out as a grid: one line, never wrapped.
        var gridRow = false
        var indent: CGFloat = 0
        var above: CGFloat = 0
        var spacing: CGFloat = 0
        var paragraph = NSParagraphStyle()
    }

    private var paragraphs: [[CGFloat]: NSParagraphStyle] = [:]

    private func lineStyle(_ spans: [Span], sourceMode: Bool) -> LineStyle {
        var st = LineStyle()
        for s in spans {
            switch s.code {
            case .heading:
                let i = Int(min(max(s.param, 1), 6)) - 1
                st.size = Theme.headingScales[i]
                st.weight = Theme.headingWeights[i]
                st.height = 1.12
            case .codeBlock:
                st.mono = true; st.size = 0.88; st.height = 1.2; st.charWrap = true
            case .fence:
                st.mono = true; st.size = 0.8; st.height = 1.2
            case .table:
                if sourceMode {
                    st.mono = true; st.size = 0.88; st.height = 1.2
                } else {
                    st.size = 0.94; st.height = 1.3; st.gridRow = true
                }
            case .htmlBlock:
                st.mono = true; st.size = 0.85; st.height = 1.2
            case .frontMatter:
                st.mono = true; st.size = 0.82; st.height = 1.2
            case .indent:
                let q = CGFloat(s.param >> 8)
                let it = CGFloat(s.param & 0xff)
                st.indent = (q * Theme.quoteStep + it * Theme.itemStep) * Theme.scale
            case .above:
                st.above = CGFloat(s.param) * Theme.scale
            default:
                break
            }
        }
        let font = Theme.font(size: Theme.bodySize * st.size, weight: st.weight, mono: st.mono)
        st.spacing = max(0, (st.height * font.pointSize - Theme.naturalHeight(font)).rounded())
        if st.gridRow {
            // Cell padding: the text starts inside its cell, and the row
            // has room above and below it for the grid's lines.
            let pad = (Theme.tableRowPad * Theme.scale).rounded()
            st.indent += Theme.tableCellPad * Theme.scale
            st.above += pad
            st.spacing += pad
        }
        let key = [st.indent, st.above, st.spacing, st.charWrap ? 1 : 0, st.gridRow ? 1 : 0]
        if let p = paragraphs[key] {
            st.paragraph = p
        } else {
            let p = NSMutableParagraphStyle()
            p.firstLineHeadIndent = st.indent
            p.headIndent = st.indent
            p.paragraphSpacingBefore = st.above
            p.lineSpacing = st.spacing
            p.lineBreakMode = st.gridRow ? .byClipping : st.charWrap ? .byCharWrapping : .byWordWrapping
            paragraphs[key] = p
            st.paragraph = p
        }
        return st
    }

    private func runs(range: NSRange, spans: [Span], block: LineStyle, revealed: [NSRange], sourceMode: Bool) -> [(
        NSRange, [NSAttributedString.Key: Any]
    )] {
        let start = range.location
        let end = NSMaxRange(range)
        var points = Set([start, end])
        for s in spans {
            points.insert(min(max(s.start, start), end))
            points.insert(min(max(s.end, start), end))
        }
        for r in revealed {
            points.insert(min(max(r.location, start), end))
            points.insert(min(max(NSMaxRange(r), start), end))
        }
        let sorted = points.sorted()
        let ordered = spans.sorted { $0.code.rawValue < $1.code.rawValue }
        var out: [(NSRange, [NSAttributedString.Key: Any])] = []
        for k in 0..<(sorted.count - 1) {
            let a = sorted[k]
            let b = sorted[k + 1]
            if a >= b { continue }
            var size = block.size
            var weight = block.weight
            var italic = false
            var mono = block.mono
            var color = Theme.text
            var background: NSColor?
            var underline = false
            var strike = false
            var hidden = false
            var gap = false
            // Later styles win, as in the GTK editor's tag priorities; the
            // codes are in that order.
            for s in ordered where s.start <= a && b <= s.end {
                switch s.code {
                case .heading:
                    color = s.param >= 6 ? Theme.dim : Theme.heading
                case .fence, .htmlBlock, .frontMatter, .raw, .inlineHtml:
                    color = Theme.dim
                case .quote:
                    color = Theme.text(alpha: 0.78)
                case .strong, .tableHeader:
                    weight = .bold
                case .emphasis:
                    italic = true
                case .strike:
                    strike = true
                    color = Theme.text(alpha: 0.65)
                case .code:
                    mono = true; size = 0.9
                    background = Theme.codeBackground
                case .link:
                    color = Theme.link; underline = true
                case .image:
                    color = Theme.link; italic = true
                case .taskDone:
                    strike = true
                    color = Theme.text(alpha: 0.5)
                case .hidden:
                    let shown = revealed.contains { $0.location <= a && b <= NSMaxRange($0) }
                    if sourceMode || shown { color = Theme.dim } else { hidden = true }
                case .tableGap:
                    gap = true
                default:
                    break
                }
            }
            var attrs: [NSAttributedString.Key: Any] = [
                .font: Theme.font(size: Theme.bodySize * size, weight: weight, italic: italic, mono: mono),
                .foregroundColor: color,
                .paragraphStyle: block.paragraph,
            ]
            if let bg = background { attrs[.backgroundColor] = bg }
            if underline { attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue }
            if strike { attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
            if hidden { attrs[.marginHidden] = true }
            if gap { attrs[.marginTableGap] = true }
            out.append((NSRange(location: a, length: b - a), attrs))
        }
        return out
    }

    /// Paragraph styles are cached per text size.
    func invalidate() {
        paragraphs.removeAll()
    }
}
