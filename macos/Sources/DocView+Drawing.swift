import AppKit
import margin_ffi

extension DocView {
    // MARK: - Drawing

    override func draw(_ dirtyRect: NSRect) {
        backgroundColor.setFill()
        dirtyRect.fill()
        guard !laidOut.isEmpty else { return }
        drawChanges(in: dirtyRect)
        if !sourceMode { drawBlocks(in: dirtyRect) }
        let visible = laidOut.indices.filter {
            laidOut[$0].bottom >= dirtyRect.minY && laidOut[$0].top <= dirtyRect.maxY
        }
        guard let first = visible.first, let last = visible.last else { return }
        let range = first...last
        drawHighlights(paragraphs: range)
        drawSelection(paragraphs: range)
        effectiveAppearance.performAsCurrentDrawingAppearance {
            for pi in range { drawText(pi) }
        }
        if !sourceMode { drawMarkers(in: dirtyRect) }
        drawObjects(in: dirtyRect)
    }

    /// Source ranges less the objects in them, which draw their own
    /// highlights and selection.
    private func withoutObjects(_ r: NSRange) -> [NSRange] {
        var out = [r]
        for o in objects where NSIntersectionRange(o.range, r).length > 0 {
            out = out.flatMap { s -> [NSRange] in
                let i = NSIntersectionRange(s, o.range)
                guard i.length > 0 else { return [s] }
                var parts: [NSRange] = []
                if i.location > s.location {
                    parts.append(NSRange(location: s.location, length: i.location - s.location))
                }
                if NSMaxRange(i) < NSMaxRange(s) {
                    parts.append(NSRange(location: NSMaxRange(i), length: NSMaxRange(s) - NSMaxRange(i)))
                }
                return parts
            }
        }
        return out
    }

    /// Comment and find highlights: where they overlap, the later one.
    private func drawHighlights(paragraphs: ClosedRange<Int>) {
        guard !highlights.isEmpty else { return }
        let lo = Int(shown[paragraphs.lowerBound].sourceStart)
        let hi = Int(shown[paragraphs.upperBound].sourceEnd) + 1
        let marks = highlights.filter { NSMaxRange($0.0) >= lo && $0.0.location <= hi && $0.0.length > 0 }
        guard !marks.isEmpty else { return }
        var points = Set<Int>()
        for (r, _) in marks {
            points.insert(r.location)
            points.insert(NSMaxRange(r))
        }
        let sorted = points.sorted()
        var segments: [(NSRange, NSColor)] = []
        for k in 0..<(sorted.count - 1) {
            let a = sorted[k]
            let b = sorted[k + 1]
            guard let color = marks.last(where: { $0.0.location <= a && b <= NSMaxRange($0.0) })?.1 else { continue }
            if let last = segments.last, NSMaxRange(last.0) == a, last.1 == color {
                segments[segments.count - 1].0.length += b - a
            } else {
                segments.append((NSRange(location: a, length: b - a), color))
            }
        }
        for (r, color) in segments {
            color.setFill()
            for part in withoutObjects(r) {
                for rect in rects(forSource: part) { rect.fill(using: .sourceOver) }
            }
        }
    }

    private func drawSelection(paragraphs: ClosedRange<Int>) {
        guard let sel = selection else { return }
        let focused = window?.isKeyWindow == true && window?.firstResponder === self
        (focused ? NSColor.selectedTextBackgroundColor : .unemphasizedSelectedTextBackgroundColor).setFill()
        for part in withoutObjects(sel) {
            for rect in rects(forSource: part, toEdge: true) { rect.fill() }
        }
    }

    /// A paragraph's lines: inline backgrounds (code), the text, and lines
    /// through struck-through text.
    private func drawText(_ pi: Int) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let p = laidOut[pi]
        let all = NSRange(location: 0, length: p.text.length)
        for l in p.lines {
            p.text.enumerateAttribute(.backgroundColor, in: NSIntersectionRange(all, l.range)) { value, r, _ in
                guard let c = value as? NSColor, r.length > 0 else { return }
                c.setFill()
                let x0 = x(r.location, on: l)
                let x1 = x(NSMaxRange(r), on: l)
                NSRect(x: min(x0, x1), y: l.textTop, width: abs(x1 - x0), height: l.bottom - l.spacing - l.textTop)
                    .fill(using: .sourceOver)
            }
        }
        ctx.saveGState()
        ctx.textMatrix = CGAffineTransform(scaleX: 1, y: -1)
        for l in p.lines {
            ctx.textPosition = CGPoint(x: l.x, y: l.baseline)
            CTLineDraw(l.line, ctx)
        }
        ctx.restoreGState()
        if isContinuousSpellCheckingEnabled && p.splice == nil {
            NSColor.systemRed.withAlphaComponent(0.85).setFill()
            for r in misspelled(pi) {
                for l in p.lines {
                    let i = NSIntersectionRange(r, l.range)
                    guard i.length > 0 else { continue }
                    // A dotted line under the word.
                    let x0 = min(x(i.location, on: l), x(NSMaxRange(i), on: l))
                    let x1 = max(x(i.location, on: l), x(NSMaxRange(i), on: l))
                    var dx = x0
                    while dx < x1 {
                        NSBezierPath(ovalIn: NSRect(x: dx, y: l.baseline + 2, width: 2, height: 2)).fill()
                        dx += 4
                    }
                }
            }
        }
        for l in p.lines {
            p.text.enumerateAttribute(.strikethroughStyle, in: NSIntersectionRange(all, l.range)) { value, r, _ in
                guard value != nil, r.length > 0 else { return }
                let attrs = p.text.attributes(at: r.location, effectiveRange: nil)
                let font = attrs[.font] as? NSFont ?? Theme.font(size: Theme.bodySize)
                ((attrs[.foregroundColor] as? NSColor) ?? Theme.text).setFill()
                let x0 = x(r.location, on: l)
                let x1 = x(NSMaxRange(r), on: l)
                let y = (l.baseline - font.xHeight / 2).rounded()
                NSRect(x: min(x0, x1), y: y - 0.5, width: abs(x1 - x0), height: max(1, Theme.scale)).fill()
            }
        }
    }

    /// Code boxes, table grids, quote bars and rules.
    private func drawBlocks(in rect: NSRect) {
        let s = Theme.scale
        let left = geometry.left
        let shownLines = lines(in: rect)
        for cb in codeBlocks
        where cb.endContentLine > cb.firstContentLine
            && Int(cb.endContentLine) > shownLines.lowerBound && Int(cb.firstContentLine) <= shownLines.upperBound
            && objectOnLine[Int(cb.openLine ?? cb.firstContentLine)] == nil
        {
            let top = textTop(line: Int(cb.firstContentLine)) - 8 * s
            let bottom = textBottom(line: Int(cb.endContentLine) - 1) + 8 * s
            if bottom < rect.minY || top > rect.maxY { continue }
            let x0 = left + indent(quotes: cb.quotes, items: cb.items) - 12 * s
            let x1 = left + geometry.docWidth + 12 * s
            let box = NSRect(x: x0, y: top, width: x1 - x0, height: bottom - top)
            Theme.codeBackground.setFill()
            NSBezierPath(roundedRect: box, xRadius: 6, yRadius: 6).fill()
            if !cb.language.isEmpty {
                let attrs: [NSAttributedString.Key: Any] = [
                    .font: Theme.font(size: Theme.bodySize * 0.72, mono: true),
                    .foregroundColor: Theme.dim,
                ]
                let label = cb.language as NSString
                let size = label.size(withAttributes: attrs)
                label.draw(at: NSPoint(x: x1 - size.width - 8, y: top + 3), withAttributes: attrs)
            }
        }
        for t in tables
        where Int(t.info.lastLine) >= shownLines.lowerBound && Int(t.info.firstLine) <= shownLines.upperBound {
            drawGrid(t, in: rect)
        }
        for q in quotes where Int(q.lastLine) >= shownLines.lowerBound && Int(q.firstLine) <= shownLines.upperBound {
            let top = textTop(line: Int(q.firstLine))
            let bottom = textBottom(line: Int(q.lastLine))
            if bottom < rect.minY || top > rect.maxY { continue }
            let x = left + indent(quotes: q.quotes, items: q.items) + 2 * s
            Theme.border.setFill()
            NSBezierPath(
                roundedRect: NSRect(x: x, y: top, width: 3 * s, height: bottom - top), xRadius: 1.5 * s,
                yRadius: 1.5 * s
            ).fill()
        }
        for li in shownLines where lines[li].kind == .rule && !isCollapsed(line: li) {
            let l = lines[li]
            let top = textTop(line: li)
            let bottom = textBottom(line: li)
            if bottom < rect.minY || top > rect.maxY { continue }
            let y = ((top + bottom) / 2).rounded()
            let x = left + indent(quotes: l.quotes, items: l.items)
            Theme.border.setFill()
            NSRect(x: x, y: y, width: left + geometry.docWidth - x, height: max(1, s)).fill()
        }
    }

    /// A table's grid: a rounded box, the header's fill, and lines between
    /// rows and columns.
    private func drawGrid(_ t: TableGrid, in rect: NSRect) {
        let pad = (Theme.tableRowPad * Theme.scale).rounded()
        let rows = t.info.rows.map { row -> (CGFloat, CGFloat) in
            let li = Int(row.line)
            return (textTop(line: li) - pad, textBottom(line: li) + pad)
        }
        guard let top = rows.first?.0, let bottom = rows.last?.1, bottom > rect.minY, top < rect.maxY else { return }
        let x0 = geometry.left + t.columnX[0]
        let box = NSRect(x: x0, y: top, width: t.width, height: bottom - top).insetBy(dx: 0.5, dy: 0.5)
        let radius = 6 * Theme.scale
        let outline = NSBezierPath(roundedRect: box, xRadius: radius, yRadius: radius)
        NSGraphicsContext.saveGraphicsState()
        outline.addClip()
        Theme.codeBackground.setFill()
        NSRect(x: box.minX, y: top, width: box.width, height: rows[0].1 - top).fill()
        Theme.border.setFill()
        for (_, b) in rows.dropLast() {
            NSRect(x: box.minX, y: b.rounded() - 0.5, width: box.width, height: 1).fill()
        }
        for x in t.columnX.dropFirst() {
            NSRect(x: (geometry.left + x).rounded() - 0.5, y: top, width: 1, height: bottom - top).fill()
        }
        NSGraphicsContext.restoreGraphicsState()
        Theme.border.setStroke()
        outline.lineWidth = 1
        outline.stroke()
    }

    private func drawMarkers(in rect: NSRect) {
        guard !lines.isEmpty else { return }
        let s = Theme.scale
        let font = Theme.font(size: Theme.bodySize)
        let shownLines = lines(in: rect)
        for it in items where shownLines.contains(Int(it.line)) && !isCollapsed(line: Int(it.line)) {
            let (baseline, xText) = markerPosition(it)
            if let checked = it.task {
                let box = checkboxBox(it)
                let path = NSBezierPath(
                    roundedRect: box.insetBy(dx: 0.75, dy: 0.75), xRadius: 3.5 * s, yRadius: 3.5 * s)
                if checked {
                    Theme.accent.setFill()
                    path.fill()
                    let check = NSBezierPath()
                    check.move(to: NSPoint(x: box.minX + 0.25 * box.width, y: box.minY + 0.52 * box.height))
                    check.line(to: NSPoint(x: box.minX + 0.43 * box.width, y: box.minY + 0.70 * box.height))
                    check.line(to: NSPoint(x: box.minX + 0.76 * box.width, y: box.minY + 0.32 * box.height))
                    check.lineWidth = 1.8 * s
                    check.lineCapStyle = .round
                    check.lineJoinStyle = .round
                    NSColor.alternateSelectedControlTextColor.setStroke()
                    check.stroke()
                } else {
                    Theme.text.withAlphaComponent(0.55).setStroke()
                    path.lineWidth = 1.3 * s
                    path.stroke()
                }
            } else {
                let label = (it.number.map { "\($0)." } ?? bullet(depth: Int(it.depth))) as NSString
                let attrs: [NSAttributedString.Key: Any] = [
                    .font: font, .foregroundColor: Theme.text.withAlphaComponent(0.85),
                ]
                let w = label.size(withAttributes: attrs).width
                label.draw(at: NSPoint(x: xText - w - 8 * s, y: baseline - font.ascender), withAttributes: attrs)
            }
        }
    }

    private func bullet(depth: Int) -> String {
        ["•", "◦", "▪"][(max(depth, 1) - 1) % 3]
    }

    /// The images and diagrams, with their own highlights and selection.
    private func drawObjects(in rect: NSRect) {
        guard !objects.isEmpty else { return }
        let sel = sourceSelection
        let focused = window?.isKeyWindow == true && window?.firstResponder === self
        let selection = focused ? NSColor.selectedTextBackgroundColor : .unemphasizedSelectedTextBackgroundColor
        for o in objects {
            let look = look(of: o)
            guard let r = objectRect(o, look: look), r.intersects(rect) else { continue }
            let selected = NSIntersectionRange(sel, o.range).length > 0
            look.draw(
                in: r, for: o, highlights: objectHighlights, selection: selected ? selection : nil,
                labels: labelHighlights.filter { $0.start == o.start }.map { ($0.label, $0.color) },
                scale: window?.backingScaleFactor ?? 2,
                canWait: window != nil && NSPrintOperation.current == nil
                    && NSGraphicsContext.currentContextDrawingToScreen())
        }
    }

    // MARK: - Changes since the last commit

    /// Bars in the left margin beside the lines changed since the last
    /// commit, and a triangle between the lines where lines were deleted.
    private func drawChanges(in rect: NSRect) {
        guard !usesFullWidth, !lines.isEmpty else { return }
        let s = Theme.scale
        let x = geometry.left - 18 * s
        let shownLines = lines(in: rect)
        for c in changes
        where Int(c.endLine) >= shownLines.lowerBound && Int(c.firstLine) <= shownLines.upperBound + 1 {
            let laid = Array(
                Set((Int(c.firstLine)..<Int(c.endLine)).map { li in objectOnLine[li].map { objects[$0].line } ?? li })
            ).sorted().filter { $0 < lines.count && !isCollapsed(line: $0) }
            Theme.change(c.kind).setFill()
            if let first = laid.first, let last = laid.last {
                let top = textTop(line: first)
                let bottom = textBottom(line: last)
                if bottom < rect.minY || top > rect.maxY { continue }
                let w = 3 * s
                NSBezierPath(
                    roundedRect: NSRect(x: x, y: top, width: w, height: bottom - top), xRadius: w / 2, yRadius: w / 2
                ).fill()
            } else {
                let y = boundary(before: Int(c.firstLine))
                let h = 4 * s
                if y + h < rect.minY || y - h > rect.maxY { continue }
                let path = NSBezierPath()
                path.move(to: NSPoint(x: x, y: y - h))
                path.line(to: NSPoint(x: x + 5 * s, y: y))
                path.line(to: NSPoint(x: x, y: y + h))
                path.close()
                path.fill()
            }
        }
    }

    /// Halfway between line `li` and the shown line before it.
    private func boundary(before li: Int) -> CGFloat {
        let prev = (0..<min(li, lines.count)).last { !isCollapsed(line: $0) }
        let next = (li..<lines.count).first { !isCollapsed(line: $0) }
        switch (prev, next) {
        case (let p?, let n?): return (textBottom(line: p) + textTop(line: n)) / 2
        case (let p?, nil): return textBottom(line: p) + 4 * Theme.scale
        case (nil, let n?): return textTop(line: n) - 4 * Theme.scale
        case (nil, nil): return textContainerInset.height
        }
    }

    /// Where the cursor goes for change `c`: its first shown line, or for a
    /// deletion the line after it.
    private func start(of c: LineChange) -> Int {
        let li = Int(c.firstLine)
        guard
            let line = (li..<lines.count).first(where: { !isCollapsed(line: $0) })
                ?? (0..<min(li, lines.count)).last(where: { !isCollapsed(line: $0) })
        else { return 0 }
        return Int(lines[line].visibleStart)
    }

    /// Moves the cursor to the start of the nearest change after it, or
    /// before it, wrapping around, and scrolls it into view.
    func stepChange(forward: Bool) {
        let starts = changes.map(start(of:))
        guard let firstStart = starts.first, let lastStart = starts.last else { return }
        let c = cursor
        let target = forward ? (starts.first { $0 > c } ?? firstStart) : (starts.last { $0 < c } ?? lastStart)
        window?.makeFirstResponder(self)
        setSelectedRange(NSRange(location: target, length: 0))
        scrollRangeToVisible(NSRange(location: target, length: 0))
    }

    // MARK: - The pointer

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for a in trackingAreas where a.owner === self { removeTrackingArea(a) }
        addTrackingArea(
            NSTrackingArea(
                rect: .zero, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect, .cursorUpdate], owner: self,
                userInfo: nil))
    }

    /// The I-beam over text, the arrow over checkboxes and images, and the
    /// pointing hand over links while Command is held.
    private func pointer(at p: NSPoint, modifiers: NSEvent.ModifierFlags) -> NSCursor {
        if task(at: p) != nil || object(atPoint: p) != nil { return .arrow }
        if modifiers.contains(.command) && !sourceMode && link(at: p) != nil { return .pointingHand }
        return .iBeam
    }

    private func updatePointer(_ event: NSEvent) {
        guard let window else { return }
        let p = convert(window.mouseLocationOutsideOfEventStream, from: nil)
        guard visibleRect.contains(p) else { return }
        pointer(at: p, modifiers: event.modifierFlags).set()
    }

    override func mouseMoved(with event: NSEvent) { updatePointer(event) }
    override func cursorUpdate(with event: NSEvent) { updatePointer(event) }
    override func flagsChanged(with event: NSEvent) { updatePointer(event) }

    override func resetCursorRects() {
        addCursorRect(visibleRect, cursor: .iBeam)
        removeAllToolTips()
        guard !sourceMode, !lines.isEmpty else { return }
        let shownLines = lines(in: visibleRect)
        for it in items where it.task != nil && shownLines.contains(Int(it.line)) {
            addCursorRect(checkboxBox(it).insetBy(dx: -4, dy: -4), cursor: .arrow)
        }
        for o in objects where shownLines.contains(o.line) {
            let look = look(of: o)
            guard let r = objectRect(o, look: look) else { continue }
            addCursorRect(r, cursor: .arrow)
            if look.isText { addToolTip(r, owner: self, userData: nil) }
        }
        let first = Int(lines[shownLines.lowerBound].start)
        let last = Int(lines[shownLines.upperBound].end)
        for r in linkRanges where NSMaxRange(r) >= first && r.location <= last {
            for rect in rects(forSource: r) { addToolTip(rect, owner: self, userData: nil) }
        }
    }

    func view(
        _ view: NSView, stringForToolTip tag: NSView.ToolTipTag, point: NSPoint, userData data: UnsafeMutableRawPointer?
    ) -> String {
        if let o = object(atPoint: point) { return imageToolTip(o) ?? "" }
        return link(at: point).map { "\($0)\n⌘-click to open" } ?? ""
    }

    /// A broken image's path or URL and why it can't be shown.
    func imageToolTip(_ o: DocObject) -> String? {
        guard case .broken(let b) = look(of: o) else { return nil }
        return "\(o.source.location)\n\(b.reason)"
    }

    // MARK: - Spelling

    /// Misspelled words in laid-out paragraph `pi`, as ranges of its text:
    /// not in code, links, syntax shown or objects.
    func misspelled(_ pi: Int) -> [NSRange] {
        guard pi < previous.typesets.count, let t = previous.typesets[pi] else { return [] }
        let id = ObjectIdentifier(t)
        if let found = misspellings[id] { return found }
        let text = t.text
        let all = NSRange(location: 0, length: text.length)
        var out: [NSRange] = []
        let results = NSSpellChecker.shared.check(
            text.string, range: all, types: NSTextCheckingResult.CheckingType.spelling.rawValue, options: nil,
            inSpellDocumentWithTag: spellTag, orthography: nil, wordCount: nil)
        for result in results where result.resultType == .spelling {
            var skip = false
            text.enumerateAttributes(in: result.range) { attrs, _, stop in
                if attrs[.underlineStyle] != nil || attrs[.backgroundColor] != nil || attrs[.marginObject] != nil
                    || attrs[.marginHidden] != nil || (attrs[.font] as? NSFont)?.isFixedPitch == true
                {
                    skip = true
                    stop.pointee = true
                }
            }
            if !skip { out.append(result.range) }
        }
        if misspellings.count > 4 * max(laidOut.count, 64) { misspellings = [:] }
        misspellings[id] = out
        return out
    }

    /// The misspelled word under a point: it, and its range in the source.
    func misspelledWord(at p: NSPoint) -> (String, NSRange)? {
        guard isContinuousSpellCheckingEnabled, let pi = laidOut.indices.last(where: { laidOut[$0].top <= p.y })
        else { return nil }
        let par = laidOut[pi]
        guard let l = par.lines.last(where: { $0.top <= p.y }), p.y <= l.bottom else { return nil }
        let o = CTLineGetStringIndexForPosition(l.line, CGPoint(x: p.x - l.x, y: 0))
        guard let r = misspelled(pi).first(where: { $0.location <= o && o <= NSMaxRange($0) }) else { return nil }
        let word = (par.text.string as NSString).substring(with: r)
        let source = projection.sourceRange(
            paragraph: UInt32(pi), start: UInt32(r.location), end: UInt32(NSMaxRange(r)))
        return (word, NSRange(source))
    }

    // MARK: - Printing

    /// Breaks pages between lines, never through one.
    override func adjustPageHeightNew(
        _ newBottom: UnsafeMutablePointer<CGFloat>, top oldTop: CGFloat, bottom oldBottom: CGFloat,
        limit bottomLimit: CGFloat
    ) {
        newBottom.pointee = oldBottom
        for p in laidOut where p.top < oldBottom && p.bottom > oldTop {
            for l in p.lines where l.top < oldBottom && l.bottom > oldBottom && l.top > oldTop {
                if l.top >= bottomLimit { newBottom.pointee = l.top }
                return
            }
        }
    }

    // MARK: - Accessibility

    // As a text view: its value is the file's text, as Markdown.
    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .textArea }
    override func accessibilityValue() -> Any? { string }
    override func accessibilityNumberOfCharacters() -> Int { text.length }
    override func accessibilitySelectedTextRange() -> NSRange { sourceSelection }
    override func accessibilitySelectedText() -> String? { text.substring(with: sourceSelection) }
}
