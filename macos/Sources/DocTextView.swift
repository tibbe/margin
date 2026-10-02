import AppKit
import margin_ffi

/// The document view. Its text is exactly the Markdown file. Keystrokes and
/// every other edit go through the core's editing rules, which answer with
/// minimal changes to the source; styling is then re-derived from the
/// analysis. Hidden syntax is laid out as null glyphs (see
/// `HidingLayoutDelegate`), and bullets, checkboxes, quote bars, rules,
/// code boxes and table grids are drawn here. Uses TextKit 1, whose glyph generation can
/// hide characters.
final class DocTextView: NSTextView, NSTextStorageDelegate, NSTextViewDelegate, NSViewToolTipOwner {
    private(set) var analysis = Analysis(text: "")
    private(set) var lines: [LineInfo] = []
    private(set) var items: [ItemInfo] = []
    private(set) var quotes: [QuoteInfo] = []
    private(set) var codeBlocks: [CodeBlockInfo] = []
    private(set) var tables: [TableGrid] = []
    /// Where each table cell's text starts, in its line fragment, by the
    /// character before it that is laid out as the space up to there.
    private(set) var tableGaps: [Int: CGFloat] = [:]
    /// Starts of table rows with a leading `|`: the cursor starts in the
    /// first cell instead.
    private var tableRowStarts = Set<Int>()
    /// Newlines inside paragraphs, laid out as spaces while reflowing.
    private(set) var softBreaks = Set<Int>()
    /// Images alone in their paragraphs, from the analysis.
    private(set) var imageBlocks: [ImageBlockInfo] = []
    /// The image blocks shown as images (none in Show Markdown), in order.
    private(set) var objects: [ImageObject] = []
    /// Indices into `objects`, by line and by first character.
    private var objectOnLine: [Int: Int] = [:]
    private var objectAtChar: [Int: Int] = [:]
    /// The folder image paths are relative to: the document's.
    var imageFolder = NSHomeDirectory() {
        didSet {
            if oldValue != imageFolder {
                updateObjects()
                needsDisplay = true
            }
        }
    }
    /// Highlights over the text (comments, find), which an image draws
    /// itself: it covers those the layout manager draws.
    var objectHighlights: [(NSRange, NSColor)] = [] {
        didSet { if !objects.isEmpty { needsDisplay = true } }
    }
    /// An image loaded or failed to: its size, and what find matches, may
    /// have changed.
    var onImagesChanged: (() -> Void)?
    /// Moving up or down past images: the selection changes as AppKit
    /// moves it, to keep its column.
    private var movingVertically = false
    let styler = Styler()
    private let hiding = HidingLayoutDelegate()

    private(set) var stale = true
    /// Text edited since the last restyle (in current coordinates).
    private var pendingDirty: NSRange?
    /// Above zero: edits are ours (already planned by the core), or raw.
    private var raw = 0
    private var revealed: [NSRange] = []
    private(set) var geometry = PageGeometry(width: 1200, scale: 1, hasCards: false)
    /// Links and images, for their tooltips.
    private var linkRanges: [NSRange] = []
    private var tableInfos: [TableInfo] = []
    /// The file as of its last commit, to mark the lines changed since.
    var committed: CommittedText? {
        didSet {
            changes = committed?.changes(text: analysis) ?? []
            needsDisplay = true
        }
    }
    /// The lines changed since the last commit, in order.
    private(set) var changes: [LineChange] = []

    var sourceMode = false {
        didSet { if oldValue != sourceMode { forceRestyle() } }
    }
    var reflowsParagraphs = false {
        didSet {
            if oldValue != reflowsParagraphs, let lm = layoutManager {
                lm.invalidateLayout(
                    forCharacterRange: NSRange(location: 0, length: (string as NSString).length),
                    actualCharacterRange: nil)
                needsDisplay = true
            }
        }
    }

    /// The text changed (after restyling).
    var onChange: (() -> Void)?
    /// Raw edit notifications, for anchors: the edited range in the new
    /// text and the change in length.
    var onEdit: ((NSRange, Int) -> Void)?
    var onSelectionChange: (() -> Void)?
    /// Positions of lines changed: the gutter follows.
    var onLayout: (() -> Void)?
    var onOpenLink: ((String) -> Void)?
    /// Escape: leave a comment, close Find.
    var onEscape: (() -> Void)?
    /// Printing lays the text out across the whole page.
    var usesFullWidth = false
    /// Highlights drawn over the text (comments, find), recomputed on change.
    var onHighlight: (() -> Void)?

    static func make() -> DocTextView {
        let storage = NSTextStorage()
        let lm = DocLayoutManager()
        storage.addLayoutManager(lm)
        let container = NSTextContainer(size: NSSize(width: 700, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = false
        container.heightTracksTextView = false
        container.lineFragmentPadding = 0
        lm.addTextContainer(container)
        let view = DocTextView(frame: NSRect(x: 0, y: 0, width: 1200, height: 800), textContainer: container)
        lm.view = view
        view.setUp()
        return view
    }

    private func setUp() {
        textStorage?.delegate = self
        delegate = self
        hiding.view = self
        layoutManager?.delegate = hiding
        layoutManager?.allowsNonContiguousLayout = false
        isRichText = false
        importsGraphics = false
        allowsUndo = true
        usesFontPanel = false
        usesFindBar = false
        isIncrementalSearchingEnabled = false
        displaysLinkToolTips = false
        smartInsertDeleteEnabled = false
        isAutomaticQuoteSubstitutionEnabled = false
        isAutomaticDashSubstitutionEnabled = false
        isAutomaticTextReplacementEnabled = false
        isAutomaticSpellingCorrectionEnabled = false
        isAutomaticLinkDetectionEnabled = false
        isAutomaticDataDetectionEnabled = false
        isAutomaticTextCompletionEnabled = false
        isContinuousSpellCheckingEnabled = true
        isGrammarCheckingEnabled = false
        // Writing Tools in their panel: their rewrites arrive as ordinary
        // replacements, which go through the editing rules.
        writingToolsBehavior = .limited
        drawsBackground = true
        backgroundColor = .textBackgroundColor
        isVerticallyResizable = true
        isHorizontallyResizable = false
        textContainerInset = NSSize(width: 0, height: 28)
        minSize = NSSize(width: 0, height: 0)
        maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        typingAttributes = [.font: Theme.font(size: Theme.bodySize), .foregroundColor: Theme.text]
        NotificationCenter.default.addObserver(
            self, selector: #selector(imageChanged(_:)), name: ImageLibrary.changed, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(imageDecoded(_:)), name: ImageLibrary.decoded, object: nil)
    }

    // MARK: - Contents

    /// Replaces the whole text, e.g. when loading a file. Not undoable.
    func setContents(_ text: String) {
        raw += 1
        textStorage?.setAttributedString(NSAttributedString(string: text, attributes: typingAttributes))
        raw -= 1
        undoManager?.removeAllActions(withTarget: textStorage as Any)
        undoManager?.removeAllActions()
        setSelectedRange(NSRange(location: 0, length: 0))
        refresh()
    }

    /// Changes the text to `new` with minimal edits, so the cursor and
    /// comment anchors stay on unchanged text. One undo step ("Undo Outside
    /// Change"), so earlier steps stay valid.
    func applyExternal(_ new: String, actionName: String = "Outside Change") {
        let old = string
        if old == new { return }
        let changes = textChanges(old: old, new: new)
        raw += 1
        breakUndoCoalescing()
        undoManager?.beginUndoGrouping()
        // One edit at a time, so anchors hear about each.
        for c in changes.reversed() {
            let r = NSRange(from: c.start, to: c.end)
            if shouldChangeText(in: r, replacementString: c.text) {
                textStorage?.replaceCharacters(in: r, with: c.text)
            }
        }
        undoManager?.setActionName(actionName)
        undoManager?.endUndoGrouping()
        raw -= 1
        didChangeText()
    }

    func textStorage(
        _ textStorage: NSTextStorage, didProcessEditing editedMask: NSTextStorageEditActions,
        range editedRange: NSRange, changeInLength delta: Int
    ) {
        guard editedMask.contains(.editedCharacters) else { return }
        stale = true
        let loc = editedRange.location
        let oldEnd = loc + editedRange.length - delta
        func map(_ p: Int) -> Int { p <= loc ? p : (p >= oldEnd ? p + delta : NSMaxRange(editedRange)) }
        if let d = pendingDirty {
            let a = min(map(d.location), loc)
            let b = max(map(NSMaxRange(d)), NSMaxRange(editedRange))
            pendingDirty = NSRange(location: a, length: b - a)
        } else {
            pendingDirty = editedRange
        }
        onEdit?(editedRange, delta)
    }

    override func didChangeText() {
        super.didChangeText()
        // While an input method composes text, leave it alone: restyling
        // would wipe the marked-text underline. It is styled once committed.
        if !hasMarkedText() { refresh() }
        onChange?()
    }

    /// Re-analyzes the text and restyles the lines that changed.
    func refresh() {
        guard let storage = textStorage else { return }
        // Foundation's bulk UTF-8 conversion is much faster than bridging
        // the storage's string to a Swift string.
        let bytes = storage.mutableString.data(using: String.Encoding.utf8.rawValue) ?? Data()
        analysis = Analysis.fromUtf8(bytes: bytes)
        lines = LineInfo.decode(analysis.linesPacked())
        items = analysis.items()
        quotes = analysis.quotes()
        codeBlocks = analysis.codeBlocks()
        tableInfos = analysis.tables()
        imageBlocks = analysis.imageBlocks()
        let newSoft = Set(analysis.softBreaks().map { Int($0) })
        let changedSoft = newSoft.symmetricDifference(softBreaks)
        softBreaks = newSoft
        changes = committed?.changes(text: analysis) ?? []
        stale = false
        revealed = revealRanges()
        restyle()
        if reflowsParagraphs, let lm = layoutManager {
            let len = (storage.string as NSString).length
            for p in changedSoft where p < len {
                lm.invalidateLayout(forCharacterRange: NSRange(location: p, length: 1), actualCharacterRange: nil)
            }
        }
        needsDisplay = true
        onHighlight?()
        onLayout?()
    }

    func ensureFresh() {
        if stale { refresh() }
    }

    private func restyle() {
        guard let storage = textStorage else { return }
        let opts = Styler.Options(sourceMode: sourceMode, revealed: revealed, generation: Theme.generation)
        updateObjects()
        var spans = Span.decode(analysis.spansPacked())
        if !sourceMode { spans += tableSpans() + objectSpans() }
        styler.apply(lines: lines, spans: spans, dirty: pendingDirty, to: storage, options: opts)
        pendingDirty = nil
        layOutTables()
        linkRanges = spans.filter { ($0.code == .link || $0.code == .image) && object(at: $0.start) == nil }
            .map { NSRange(location: $0.start, length: $0.end - $0.start) }
        window?.invalidateCursorRects(for: self)
    }

    /// Restyles every line, e.g. after the text size changed.
    func forceRestyle() {
        Theme.generation += 1
        ensureFresh()
        restyle()
        updateGeometry(force: true)
        needsDisplay = true
        onHighlight?()
        onLayout?()
    }

    // MARK: - Tables

    /// A table laid out as a grid: its columns' left edges and widths, in
    /// text container coordinates.
    struct TableGrid {
        var info: TableInfo
        var columnX: [CGFloat]
        var columnWidths: [CGFloat]
        var width: CGFloat {
            columnWidths.reduce(0, +) + 2 * Theme.tableCellPad * Theme.scale * CGFloat(columnWidths.count)
        }
    }

    /// Tables show as grids: the delimiter row is hidden, and so are each
    /// cell's padding and `|` but for one character, which is laid out as
    /// the space to the cell's column.
    private func tableSpans() -> [Span] {
        var out: [Span] = []
        let len = (string as NSString).length
        for t in tableInfos {
            let d = lines[Int(t.delimiterLine)]
            out.append(Span(start: Int(d.start), end: min(Int(d.end) + 1, len), code: .hidden, param: 0))
            for row in t.rows {
                for cell in row.cells where cell.lead.end > cell.lead.start {
                    let gap = Int(cell.lead.end) - 1
                    out.append(Span(start: Int(cell.lead.start), end: gap, code: .hidden, param: 0))
                    out.append(Span(start: gap, end: gap + 1, code: .tableGap, param: 0))
                }
                out.append(Span(start: Int(row.trail.start), end: Int(row.trail.end), code: .hidden, param: 0))
            }
        }
        return out
    }

    /// Sizes each table's columns to their widest cell, and lays out again
    /// the tables whose columns moved.
    private func layOutTables() {
        guard let storage = textStorage, let lm = layoutManager else { return }
        var grids: [TableGrid] = []
        var gaps: [Int: CGFloat] = [:]
        tableRowStarts = []
        if !sourceMode {
            let pad = Theme.tableCellPad * Theme.scale
            let minWidth = 2 * pad
            for t in tableInfos {
                let n = t.aligns.count
                guard n > 0, let first = t.rows.first else { continue }
                var widths = [CGFloat](repeating: minWidth, count: n)
                var cellWidths: [[CGFloat]] = []
                for row in t.rows {
                    let ws = row.cells.prefix(n).map { visibleWidth(NSRange($0.content), in: storage) }
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
                    if let c = row.cells.first, c.lead.end > c.lead.start { tableRowStarts.insert(Int(c.lead.start)) }
                    for (j, cell) in row.cells.prefix(n).enumerated() where cell.lead.end > cell.lead.start {
                        let slack = widths[j] - ws[j]
                        let offset: CGFloat
                        switch t.aligns[j] {
                        case .right: offset = slack
                        case .center: offset = (slack / 2).rounded()
                        default: offset = 0
                        }
                        gaps[Int(cell.lead.end) - 1] = columnX[j] + pad + offset
                    }
                }
                grids.append(TableGrid(info: t, columnX: columnX, columnWidths: widths))
            }
        }
        tables = grids
        if gaps != tableGaps {
            tableGaps = gaps
            for t in grids {
                let a = Int(lines[Int(t.info.firstLine)].start)
                let b = Int(lines[Int(t.info.lastLine)].end)
                lm.invalidateLayout(forCharacterRange: NSRange(location: a, length: b - a), actualCharacterRange: nil)
            }
        }
    }

    /// The width of a range's shown text, hidden syntax left out.
    private func visibleWidth(_ r: NSRange, in storage: NSTextStorage) -> CGFloat {
        guard r.length > 0, NSMaxRange(r) <= storage.length else { return 0 }
        var w: CGFloat = 0
        storage.enumerateAttribute(.marginHidden, in: r, options: []) { v, sub, _ in
            if v == nil { w += storage.attributedSubstring(from: sub).size().width }
        }
        return ceil(w)
    }

    // MARK: - Images

    /// The image blocks shown as images: all of them, but in Show Markdown.
    /// Lines whose image moved are laid out again.
    private func updateObjects() {
        let old = objects.map(\.start)
        objects =
            sourceMode
            ? []
            : imageBlocks.enumerated().compactMap { i, b in
                let li = Int(b.line)
                guard li < lines.count else { return nil }
                return ImageObject(
                    index: i, info: b, source: ImageSource.resolve(b.url, from: imageFolder), line: li,
                    end: Int(lines[li].end))
            }
        objectOnLine = [:]
        objectAtChar = [:]
        for (i, o) in objects.enumerated() {
            objectOnLine[o.line] = i
            objectAtChar[o.start] = i
        }
        if objects.map(\.start) != old { invalidateObjects(objects) }
    }

    /// An image shows in place of its source: its first character is laid
    /// out as the space it is drawn in, and the rest of its line hidden.
    private func objectSpans() -> [Span] {
        objects.flatMap { o in
            [
                Span(start: o.start, end: o.start + 1, code: .object, param: 0),
                Span(start: o.start + 1, end: o.end, code: .hidden, param: 0),
            ]
        }
    }

    /// The image on the line of `p`, if `p` is on it: from its start to its
    /// line's end.
    func object(at p: Int) -> ImageObject? {
        guard !objects.isEmpty, !lines.isEmpty else { return nil }
        let li = Int(analysis.lineIndex(pos: UInt32(max(0, p))))
        guard let i = objectOnLine[li], objects[i].contains(p) else { return nil }
        return objects[i]
    }

    /// The image that `p` is on, where the cursor may not rest: it rests on
    /// an image's edge only where there is no text before it, or after it,
    /// to go to instead.
    private func objectZone(at p: Int) -> ImageObject? {
        guard let o = object(at: p) else { return nil }
        if p == o.start && (0..<o.line).allSatisfy(isCollapsed(line:)) { return nil }
        if p == o.end && (o.line + 1..<lines.count).allSatisfy(isCollapsed(line:)) { return nil }
        return o
    }

    /// The image the selection is, if it is one.
    var selectedObject: ImageObject? {
        let sel = selectedRange()
        guard sel.length > 0, let o = object(at: sel.location), sel.location == o.start,
            NSMaxRange(sel) >= o.sourceEnd, NSMaxRange(sel) <= o.end
        else { return nil }
        return o
    }

    /// The image blocks shown as their alt text, as indices for find: those
    /// that can't be loaded, or all of them in Show Markdown.
    var altTextImages: [UInt32] {
        if sourceMode { return imageBlocks.indices.map { UInt32($0) } }
        return objects.compactMap { o in
            if case .failed = ImageLibrary.shared.state(of: o.source) { UInt32(o.index) } else { nil }
        }
    }

    /// How image `o` looks now, its size from the text column and the zoom.
    func look(of o: ImageObject) -> ObjectLook {
        let l = lines[o.line]
        let room = max(1, geometry.docWidth - indent(quotes: l.quotes, items: l.items))
        switch ImageLibrary.shared.state(of: o.source) {
        case .loaded(let image):
            return .image(image, ObjectLook.size(of: image, zoom: Theme.bodySize / Theme.baseBodySize, room: room))
        case .loading:
            return .placeholder(NSSize(width: room.rounded(), height: (160 * Theme.scale).rounded()))
        case .failed(let reason):
            let s = (string as NSString)
            let alt = o.info.alt.map { r -> (NSRange, String) in (NSRange(r), s.substring(with: NSRange(r))) }
            return .broken(BrokenImage(alt: alt, fileName: o.source.fileName, reason: reason, room: room))
        }
    }

    /// The width laid out for the image whose first character is `ci`.
    func objectWidth(at ci: Int) -> CGFloat? {
        guard let i = objectAtChar[ci] else { return nil }
        return look(of: objects[i]).size.width
    }

    /// The height of the image laid out in the line fragment of `chars`,
    /// when it is taller than the text: drawn as the image or a placeholder.
    /// Hidden syntax after a newline joins the fragment before it, so in a
    /// list or a quote the image's fragment ends on the next line, with
    /// that line's hidden prefix: the image is the one starting in it.
    func objectHeight(inFragment chars: NSRange) -> CGFloat? {
        guard chars.length > 0, !objects.isEmpty else { return nil }
        // Objects are in text order: the first not before the fragment.
        var lo = 0
        var hi = objects.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if objects[mid].start < chars.location { lo = mid + 1 } else { hi = mid }
        }
        guard lo < objects.count, NSLocationInRange(objects[lo].start, chars) else { return nil }
        let look = look(of: objects[lo])
        return look.isText ? nil : look.size.height
    }

    /// Where image `o` is drawn, in view coordinates.
    func objectRect(_ o: ImageObject, look: ObjectLook) -> NSRect? {
        guard let lm = layoutManager, o.start < (string as NSString).length else { return nil }
        let g = lm.glyphIndexForCharacter(at: o.start)
        let frag = lm.lineFragmentRect(forGlyphAt: g, effectiveRange: nil)
        let at = lm.location(forGlyphAt: g)
        let origin = textContainerOrigin
        let x = origin.x + frag.minX + at.x
        switch look {
        case .image(_, let size), .placeholder(let size):
            let above = o.line < styler.metrics.count ? styler.metrics[o.line].spaceAbove : 0
            return NSRect(origin: NSPoint(x: x, y: origin.y + frag.minY + above), size: size)
        case .broken(let b):
            let baseline = origin.y + frag.minY + at.y
            return NSRect(x: x, y: baseline - b.ascent, width: b.width, height: b.ascent + b.descent)
        }
    }

    /// The image drawn under a point, if any.
    private func object(atPoint p: NSPoint) -> ImageObject? {
        guard !stale, !sourceMode else { return nil }
        // Across the view: the text column starts past its left margin.
        let shown = lines(in: NSRect(x: bounds.minX, y: p.y - 20, width: bounds.width, height: 40))
        return objects.first { o in
            shown.contains(o.line) && (objectRect(o, look: look(of: o))?.contains(p) ?? false)
        }
    }

    /// Lays out the lines of `objects` again.
    private func invalidateObjects(_ objects: [ImageObject]) {
        guard let lm = layoutManager else { return }
        let len = (string as NSString).length
        for o in objects where o.start < len {
            let l = lines[o.line]
            let range = NSRange(location: Int(l.start), length: min(Int(l.end) + 1, len) - Int(l.start))
            lm.invalidateLayout(forCharacterRange: range, actualCharacterRange: nil)
            lm.invalidateDisplay(forCharacterRange: range)
        }
    }

    /// Images whose size arrived, or that failed, not yet laid out again.
    private var imagesToLayOut = Set<ImageSource>()
    /// Whether some images are waiting to be laid out again.
    private(set) var imageLayoutPending = false

    /// An image loaded, or failed to: its lines are laid out for its size,
    /// keeping the text being read where it is. Images that arrive close
    /// together are laid out together, so a document of many images isn't
    /// laid out again for each.
    @objc private func imageChanged(_ note: Notification) {
        guard let source = note.userInfo?[ImageLibrary.sourceKey] as? ImageSource,
            objects.contains(where: { $0.source == source })
        else { return }
        imagesToLayOut.insert(source)
        guard !imageLayoutPending else { return }
        imageLayoutPending = true
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(50))
            self?.layOutArrivedImages()
        }
    }

    /// Lays out again the images that arrived since the last time.
    func layOutArrivedImages() {
        guard imageLayoutPending else { return }
        imageLayoutPending = false
        let sources = imagesToLayOut
        imagesToLayOut = []
        let changed = objects.filter { sources.contains($0.source) }
        guard !changed.isEmpty else { return }
        keepingReadingPlace(changing: Set(changed.map(\.line))) { invalidateObjects(changed) }
        needsDisplay = true
        window?.invalidateCursorRects(for: self)
        onLayout?()
        onImagesChanged?()
    }

    /// An image was decoded for drawing: it is drawn again.
    @objc private func imageDecoded(_ note: Notification) {
        guard let source = note.userInfo?[ImageLibrary.sourceKey] as? ImageSource else { return }
        for o in objects where o.source == source {
            if let r = objectRect(o, look: look(of: o)) { setNeedsDisplay(r) }
        }
    }

    /// Runs `change`, which changes the heights of `changing` lines,
    /// keeping the first line shown at the top of the window where it is,
    /// so the text being read stays put when an image above it grows.
    private func keepingReadingPlace(changing: Set<Int>, _ change: () -> Void) {
        guard let clip = enclosingScrollView?.contentView, let lm = layoutManager, let tc = textContainer,
            !lines.isEmpty
        else { return change() }
        let top = visibleRect.minY
        let anchor = lines(in: visibleRect).first { li in
            !changing.contains(li) && !isCollapsed(line: li) && textTop(line: li) >= top
        }
        let before = anchor.map { textTop(line: $0) }
        change()
        lm.ensureLayout(for: tc)
        sizeToFit()
        enclosingScrollView?.documentView?.layoutSubtreeIfNeeded()
        guard let anchor, let before else { return }
        let moved = textTop(line: anchor) - before
        if moved != 0 {
            clip.scroll(to: NSPoint(x: clip.bounds.minX, y: max(0, clip.bounds.minY + moved)))
            enclosingScrollView?.reflectScrolledClipView(clip)
        }
    }

    /// Runs `done` once every image has loaded or failed to, laid out.
    func whenImagesSettled(_ done: @escaping () -> Void) {
        ImageLibrary.shared.whenSettled(objects.map(\.source)) { [weak self] in
            self?.layOutArrivedImages()
            if let lm = self?.layoutManager, let tc = self?.textContainer { lm.ensureLayout(for: tc) }
            done()
        }
    }

    /// Puts the cursor at `p`, or past the image `p` is on.
    func placeCursor(at p: Int) {
        guard let o = objectZone(at: p) else { return setSelectedRange(NSRange(location: p, length: 0)) }
        raw += 1
        setSelectedRange(landing(from: o.end, forward: true))
        raw -= 1
    }

    /// Syntax the cursor needs to see: the blank line it is on, and the
    /// fences of the code block it is in.
    private func revealRanges() -> [NSRange] {
        if lines.isEmpty || sourceMode { return [] }
        let len = (string as NSString).length
        let cursor = min(selectedRange().location, len)
        let li = Int(analysis.lineIndex(pos: UInt32(cursor)))
        guard li < lines.count else { return [] }
        var out: [NSRange] = []
        let line = lines[li]
        if line.kind == .blank {
            out.append(NSRange(location: Int(line.start), length: min(Int(line.end) + 1, len) - Int(line.start)))
        }
        for cb in codeBlocks {
            let first = Int(cb.openLine ?? cb.firstContentLine)
            let last = Int(cb.closeLine ?? max(cb.firstContentLine, cb.endContentLine &- 1))
            guard li >= first && li <= last else { continue }
            for fl in [cb.openLine, cb.closeLine].compactMap({ $0 }) {
                let l = lines[Int(fl)]
                let end = min(Int(l.end) + 1, len)
                out.append(NSRange(location: Int(l.contentStart), length: max(0, end - Int(l.contentStart))))
            }
        }
        return out.filter { $0.length > 0 }
    }

    private func updateReveal() {
        guard !stale else { return }
        let want = revealRanges()
        if want != revealed {
            revealed = want
            restyle()
            onLayout?()
        }
    }

    /// No spelling marks in code, URLs or syntax.
    func textView(_ textView: NSTextView, shouldSetSpellingState value: Int, range: NSRange) -> Int {
        guard value != 0, !stale, let storage = textStorage, NSMaxRange(range) <= storage.length else { return value }
        var skip = false
        storage.enumerateAttributes(in: range, options: []) { attrs, _, stop in
            if attrs[.marginHidden] != nil || attrs[.marginObject] != nil || attrs[.underlineStyle] != nil
                || attrs[.backgroundColor] != nil || (attrs[.font] as? NSFont)?.isFixedPitch == true
            {
                skip = true
                stop.pointee = true
            }
        }
        return skip ? 0 : value
    }

    // MARK: - Geometry

    /// The text column starts after the page's left margin, which is part
    /// of the text view so clicking it places the cursor.
    override var textContainerOrigin: NSPoint {
        NSPoint(x: geometry.left, y: super.textContainerOrigin.y)
    }

    /// Asks the page to lay out again (the text size changed).
    var onRetile: (() -> Void)?

    func updateGeometry(force: Bool) {
        if usesFullWidth {
            setPage(PageGeometry(fullWidth: bounds.width), force: force)
        } else {
            onRetile?()
        }
    }

    /// Sets the page's geometry, from `PageView`.
    func setPage(_ g: PageGeometry, force: Bool = false) {
        if force || g.left != geometry.left || g.docWidth != geometry.docWidth || g.gutterX != geometry.gutterX {
            geometry = g
            textContainer?.containerSize = NSSize(width: g.docWidth, height: CGFloat.greatestFiniteMagnitude)
            invalidateTextContainerOrigin()
            window?.invalidateCursorRects(for: self)
            // The text moved under the insertion point, which AppKit
            // places only when the selection changes.
            updateInsertionPointStateAndRestartTimer(window?.firstResponder === self)
            needsDisplay = true
            onLayout?()
        }
    }

    /// The rectangle of the line fragment holding character `ci`, in view
    /// coordinates.
    func fragmentRect(at ci: Int) -> NSRect {
        guard let lm = layoutManager, let tc = textContainer else { return .zero }
        let len = (string as NSString).length
        let origin = textContainerOrigin
        if ci >= len {
            var r = lm.extraLineFragmentRect
            if r.isEmpty && len > 0 {
                let g = lm.glyphIndexForCharacter(at: len - 1)
                r = lm.lineFragmentRect(forGlyphAt: g, effectiveRange: nil)
            }
            return r.offsetBy(dx: origin.x, dy: origin.y)
        }
        lm.ensureLayout(for: tc)
        let g = lm.glyphIndexForCharacter(at: ci)
        return lm.lineFragmentRect(forGlyphAt: g, effectiveRange: nil).offsetBy(dx: origin.x, dy: origin.y)
    }

    /// A line's first shown character (or its end), where what is drawn
    /// beside it goes: hidden syntax that starts a line is laid out on the
    /// line before. The core's `visibleStart` is past Markdown's hidden
    /// syntax; this also passes what only this view hides, the leading `|`
    /// of a table row shown as a grid.
    private func visibleStart(_ l: LineInfo) -> Int {
        var ci = Int(l.visibleStart)
        while ci < Int(l.end) && isHidden(ci) { ci += 1 }
        return ci
    }

    /// The top of a line's text, below the space above it.
    func textTop(line li: Int) -> CGFloat {
        let l = lines[li]
        let r = fragmentRect(at: visibleStart(l))
        let above = li < styler.metrics.count ? styler.metrics[li].spaceAbove : 0
        return r.minY + above
    }

    /// The bottom of a line's last fragment, without its line spacing.
    func textBottom(line li: Int) -> CGFloat {
        let l = lines[li]
        let r = fragmentRect(at: Int(l.end))
        let spacing = li < styler.metrics.count ? styler.metrics[li].lineSpacing : 0
        return r.maxY - spacing
    }

    /// Where a character sits: its line's top and height, in view
    /// coordinates.
    func location(of ci: Int) -> NSRect {
        let r = fragmentRect(at: ci)
        let li = lines.isEmpty ? 0 : Int(analysis.lineIndex(pos: UInt32(ci)))
        let above = li < styler.metrics.count ? styler.metrics[li].spaceAbove : 0
        return NSRect(x: r.minX, y: r.minY + above, width: r.width, height: max(0, r.height - above))
    }

    /// The lines shown in `rect` (view coordinates), with a line of margin.
    private func lines(in rect: NSRect) -> ClosedRange<Int> {
        guard let lm = layoutManager, let tc = textContainer, !lines.isEmpty else { return 0...0 }
        let origin = textContainerOrigin
        let local = rect.offsetBy(dx: -origin.x, dy: -origin.y)
        let glyphs = lm.glyphRange(forBoundingRectWithoutAdditionalLayout: local, in: tc)
        let chars = lm.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        let first = Int(analysis.lineIndex(pos: UInt32(chars.location)))
        let last = Int(analysis.lineIndex(pos: UInt32(NSMaxRange(chars))))
        return max(0, first - 1)...min(lines.count - 1, last + 1)
    }

    private func indent(quotes: UInt8, items: UInt8) -> CGFloat {
        (CGFloat(quotes) * Theme.quoteStep + CGFloat(items) * Theme.itemStep) * Theme.scale
    }

    // MARK: - Drawing

    override func drawBackground(in rect: NSRect) {
        super.drawBackground(in: rect)
        guard !stale, !lines.isEmpty else { return }
        drawChanges(in: rect)
        // Show Markdown shows the syntax itself instead.
        guard !sourceMode else { return }
        let s = Theme.scale
        let left = geometry.left
        let shown = lines(in: rect)
        for cb in codeBlocks
        where cb.endContentLine > cb.firstContentLine
            && Int(cb.endContentLine) > shown.lowerBound && Int(cb.firstContentLine) <= shown.upperBound
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
        for t in tables where Int(t.info.lastLine) >= shown.lowerBound && Int(t.info.firstLine) <= shown.upperBound {
            drawGrid(t, in: rect)
        }
        for q in quotes where Int(q.lastLine) >= shown.lowerBound && Int(q.firstLine) <= shown.upperBound {
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
        for li in shown where lines[li].kind == .rule {
            let l = lines[li]
            // The rule's dashes are hidden and belong to the fragment
            // before; its newline is on the rule's own.
            let r = fragmentRect(at: Int(l.end))
            if r.maxY < rect.minY || r.minY > rect.maxY { continue }
            let above = li < styler.metrics.count ? styler.metrics[li].spaceAbove : 0
            let spacing = li < styler.metrics.count ? styler.metrics[li].lineSpacing : 0
            let y = ((r.minY + above + r.maxY - spacing) / 2).rounded()
            let x = left + indent(quotes: l.quotes, items: l.items)
            Theme.border.setFill()
            NSRect(x: x, y: y, width: left + geometry.docWidth - x, height: max(1, s)).fill()
        }
    }

    /// Bars in the left margin beside the lines changed since the last
    /// commit, and a triangle between the lines where lines were deleted.
    /// Lines laid out as nothing (blank lines, fences) get no bar; a change
    /// of only those is marked like a deletion.
    private func drawChanges(in rect: NSRect) {
        guard !usesFullWidth else { return }
        let s = Theme.scale
        let x = geometry.left - 18 * s
        let shown = lines(in: rect)
        for c in changes where Int(c.endLine) >= shown.lowerBound && Int(c.firstLine) <= shown.upperBound + 1 {
            let laidOut = (Int(c.firstLine)..<Int(c.endLine)).filter { !isCollapsed(line: $0) }
            Theme.change(c.kind).setFill()
            if let first = laidOut.first, let last = laidOut.last {
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

    /// Whether line `li` is laid out as nothing.
    private func isCollapsed(line li: Int) -> Bool {
        li < lines.count && inCollapsedLine(Int(lines[li].start))
    }

    /// Halfway between line `li` and the shown line before it.
    private func boundary(before li: Int) -> CGFloat {
        let prev = (0..<min(li, lines.count)).last { !isCollapsed(line: $0) }
        let next = (li..<lines.count).first { !isCollapsed(line: $0) }
        switch (prev, next) {
        case (let p?, let n?): return (textBottom(line: p) + textTop(line: n)) / 2
        case (let p?, nil): return textBottom(line: p) + 4 * Theme.scale
        case (nil, let n?): return textTop(line: n) - 4 * Theme.scale
        case (nil, nil): return fragmentRect(at: 0).minY
        }
    }

    /// Where the cursor goes for change `c`: its first shown line, or for a
    /// deletion the line after it.
    private func start(of c: LineChange) -> Int {
        let li = Int(c.firstLine)
        // The cursor can't rest on a line laid out as nothing.
        guard
            let shown = (li..<lines.count).first(where: { !isCollapsed(line: $0) })
                ?? (0..<li).last(where: { !isCollapsed(line: $0) })
        else { return 0 }
        return visibleStart(lines[shown])
    }

    /// Moves the cursor to the start of the nearest change after it, or
    /// before it, wrapping around the document, and scrolls it into view.
    func stepChange(forward: Bool) {
        ensureFresh()
        let starts = changes.map(start(of:))
        guard let firstStart = starts.first, let lastStart = starts.last else { return }
        let c = selectedRange().location
        let target = forward ? (starts.first { $0 > c } ?? firstStart) : (starts.last { $0 < c } ?? lastStart)
        window?.makeFirstResponder(self)
        setSelectedRange(NSRange(location: target, length: 0))
        scrollRangeToVisible(NSRange(location: target, length: 0))
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

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        drawObjects(in: dirtyRect)
        drawMarkers(in: dirtyRect)
    }

    /// The images, with their own highlights and selection: the layout
    /// manager draws none on their lines (see `DocLayoutManager`).
    private func drawObjects(in rect: NSRect) {
        guard !stale, !objects.isEmpty else { return }
        let shown = lines(in: rect)
        let sel = selectedRange()
        let focused = window?.isKeyWindow == true && window?.firstResponder === self
        let selection = focused ? NSColor.selectedTextBackgroundColor : .unemphasizedSelectedTextBackgroundColor
        for o in objects where shown.contains(o.line) {
            let look = look(of: o)
            guard let r = objectRect(o, look: look), r.intersects(rect) else { continue }
            let selected = NSIntersectionRange(sel, o.range).length > 0
            // A view without a window, or printing, can't wait for a
            // decoding to be drawn again.
            look.draw(
                in: r, for: o, highlights: objectHighlights, selection: selected ? selection : nil,
                scale: window?.backingScaleFactor ?? 2,
                canWait: window != nil && NSPrintOperation.current == nil
                    && NSGraphicsContext.currentContextDrawingToScreen())
        }
    }

    /// The lines images take, in view coordinates.
    func objectLines() -> [NSRect] {
        guard !stale, let lm = layoutManager else { return [] }
        let len = (string as NSString).length
        return objects.filter { $0.start < len }.map { o in
            lm.lineFragmentRect(forGlyphAt: lm.glyphIndexForCharacter(at: o.start), effectiveRange: nil)
                .offsetBy(dx: textContainerOrigin.x, dy: textContainerOrigin.y)
        }
    }

    /// Where a list item's marker goes: its text's baseline and left edge.
    /// Beside an image, the baseline of a line of text at its top.
    func markerPosition(_ it: ItemInfo) -> (baseline: CGFloat, xText: CGFloat) {
        let font = Theme.font(size: Theme.bodySize)
        let line = lines[Int(it.line)]
        let x = geometry.left + indent(quotes: it.quotes, items: it.items)
        if let i = objectOnLine[Int(it.line)], case let look = look(of: objects[i]), !look.isText,
            let r = objectRect(objects[i], look: look)
        {
            return (r.minY + font.ascender, x)
        }
        let ci = visibleStart(line)
        let frag = fragmentRect(at: ci)
        var baseline = frag.minY + font.ascender
        if let lm = layoutManager, ci < (string as NSString).length {
            let b = frag.minY + lm.location(forGlyphAt: lm.glyphIndexForCharacter(at: ci)).y
            if b > frag.minY { baseline = b }
        }
        return (baseline, x)
    }

    /// A task item's drawn checkbox, from the current layout.
    func checkboxBox(_ it: ItemInfo) -> NSRect {
        let font = Theme.font(size: Theme.bodySize)
        let (baseline, xText) = markerPosition(it)
        let size = (14 * Theme.scale).rounded()
        return NSRect(
            x: xText - size - 8 * Theme.scale, y: (baseline - font.xHeight / 2 - size / 2).rounded(), width: size,
            height: size)
    }

    /// The task whose checkbox is under `p`, as an index for `toggleTask`.
    private func task(at p: NSPoint) -> UInt32? {
        guard !stale, !sourceMode, !lines.isEmpty else { return nil }
        let shown = lines(in: NSRect(x: 0, y: p.y - 20, width: 1, height: 40))
        for it in items where it.task != nil && shown.contains(Int(it.line)) {
            if checkboxBox(it).insetBy(dx: -4, dy: -4).contains(p) { return analysis.taskOnLine(line: it.line) }
        }
        return nil
    }

    private func drawMarkers(in rect: NSRect) {
        guard !stale, !lines.isEmpty, !sourceMode else { return }
        let s = Theme.scale
        let font = Theme.font(size: Theme.bodySize)
        let shown = lines(in: rect)
        for it in items where shown.contains(Int(it.line)) {
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

    /// The Mac convention: the I-beam over text (and the page around it),
    /// the arrow over controls (cards, buttons, checkboxes), and the
    /// pointing hand only over links, here while Command is held.
    private func cursor(at p: NSPoint, modifiers: NSEvent.ModifierFlags) -> NSCursor? {
        if task(at: p) != nil || object(atPoint: p) != nil {
            return .arrow
        }
        if modifiers.contains(.command) && !sourceMode && link(at: p) != nil {
            return .pointingHand
        }
        return nil
    }

    private func updateCursor(_ event: NSEvent) {
        guard let window else { return }
        let p = convert(window.mouseLocationOutsideOfEventStream, from: nil)
        guard visibleRect.contains(p) else { return }
        (cursor(at: p, modifiers: event.modifierFlags) ?? .iBeam).set()
    }

    override func resetCursorRects() {
        super.resetCursorRects()
        removeAllToolTips()
        guard !stale, !sourceMode, !lines.isEmpty, let lm = layoutManager, let tc = textContainer else { return }
        let shown = lines(in: visibleRect)
        for it in items where it.task != nil && shown.contains(Int(it.line)) {
            addCursorRect(checkboxBox(it).insetBy(dx: -4, dy: -4), cursor: .arrow)
        }
        // Images are objects, not text; a broken one says why when hovered.
        for o in objects where shown.contains(o.line) {
            let look = look(of: o)
            guard let r = objectRect(o, look: look) else { continue }
            addCursorRect(r, cursor: .arrow)
            if look.isText { addToolTip(r, owner: self, userData: nil) }
        }
        let first = Int(lines[shown.lowerBound].start)
        let last = Int(lines[shown.upperBound].end)
        let origin = textContainerOrigin
        for r in linkRanges where NSMaxRange(r) >= first && r.location <= last {
            let glyphs = lm.glyphRange(forCharacterRange: r, actualCharacterRange: nil)
            lm.enumerateEnclosingRects(
                forGlyphRange: glyphs, withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0), in: tc
            ) { rect, _ in
                self.addToolTip(rect.offsetBy(dx: origin.x, dy: origin.y), owner: self, userData: nil)
            }
        }
    }

    func view(
        _ view: NSView, stringForToolTip tag: NSView.ToolTipTag, point: NSPoint, userData data: UnsafeMutableRawPointer?
    ) -> String {
        if let o = object(atPoint: point) { return imageToolTip(o) ?? "" }
        return link(at: point).map { "\($0)\n⌘-click to open" } ?? ""
    }

    /// A broken image's path or URL and why it can't be shown.
    func imageToolTip(_ o: ImageObject) -> String? {
        guard case .broken(let b) = look(of: o) else { return nil }
        return "\(o.source.location)\n\(b.reason)"
    }

    override func cursorUpdate(with event: NSEvent) {
        super.cursorUpdate(with: event)
        updateCursor(event)
    }

    override func flagsChanged(with event: NSEvent) {
        super.flagsChanged(with: event)
        updateCursor(event)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        onHighlight?()
    }

    /// The text of a range as shown: hidden syntax and images left out,
    /// newlines inside paragraphs read as spaces.
    func visibleText(_ r: NSRange) -> String {
        guard let storage = textStorage else { return "" }
        let s = storage.string as NSString
        var out = ""
        storage.enumerateAttributes(in: r, options: []) { attrs, sub, _ in
            if attrs[.marginHidden] == nil && attrs[.marginObject] == nil { out += s.substring(with: sub) }
        }
        return out
    }

    // MARK: - Mouse

    override func cancelOperation(_ sender: Any?) {
        onEscape?()
    }

    override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        ensureFresh()
        if let item = task(at: p) {
            apply(analysis.toggleTask(item: item, cursor: UInt32(selectedRange().location)))
            return
        }
        if event.modifierFlags.contains(.command), let url = link(at: p) {
            onOpenLink?(url)
            return
        }
        // A click selects an image; Shift-click extends the selection over
        // it whole.
        if let o = object(atPoint: p) {
            window?.makeFirstResponder(self)
            let sel = selectedRange()
            setSelectedRange(event.modifierFlags.contains(.shift) ? NSUnionRange(sel, o.range) : o.range)
            return
        }
        super.mouseDown(with: event)
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        updateCursor(event)
    }

    /// The destination of the link under a point, if any.
    func link(at p: NSPoint) -> String? {
        guard let lm = layoutManager, let tc = textContainer, !stale else { return nil }
        let origin = textContainerOrigin
        let local = NSPoint(x: p.x - origin.x, y: p.y - origin.y)
        var fraction: CGFloat = 0
        let g = lm.glyphIndex(for: local, in: tc, fractionOfDistanceThroughGlyph: &fraction)
        let bounds = lm.boundingRect(forGlyphRange: NSRange(location: g, length: 1), in: tc)
        guard bounds.contains(local) else { return nil }
        let ci = lm.characterIndexForGlyph(at: g)
        return analysis.linkAt(pos: UInt32(ci))
    }

    // MARK: - Selection

    override func setSelectedRanges(_ ranges: [NSValue], affinity: NSSelectionAffinity, stillSelecting: Bool) {
        let before = selectedRange()
        // Margin edits one range at a time: a multiple selection (from
        // Command-drag or code) becomes its first range.
        super.setSelectedRanges(Array(ranges.prefix(1)), affinity: affinity, stillSelecting: stillSelecting)
        // An image draws its own selection.
        let after = selectedRange()
        for o in objects
        where (NSIntersectionRange(before, o.range).length > 0) != (NSIntersectionRange(after, o.range).length > 0) {
            if let r = objectRect(o, look: look(of: o)) { setNeedsDisplay(r.insetBy(dx: -1, dy: -1)) }
        }
        if !stillSelecting && !hasMarkedText() {
            updateReveal()
            onSelectionChange?()
        }
    }

    var cursor: Int { selectedRange().location + selectedRange().length }

    /// The selection, or nil when it is empty.
    var selection: NSRange? {
        let r = selectedRange()
        return r.length > 0 ? r : nil
    }

    private func isHidden(_ i: Int) -> Bool {
        guard let storage = textStorage, i >= 0, i < storage.length else { return false }
        return storage.attribute(.marginHidden, at: i, effectiveRange: nil) != nil
    }

    /// Whether every character of the line holding `p`, newline included,
    /// is hidden (a collapsed blank line or fence).
    private func inCollapsedLine(_ p: Int) -> Bool {
        guard !lines.isEmpty else { return false }
        let li = Int(analysis.lineIndex(pos: UInt32(p)))
        guard li < lines.count else { return false }
        let l = lines[li]
        let len = (string as NSString).length
        let end = min(Int(l.end) + 1, len)
        if end <= Int(l.start) { return false }
        for i in Int(l.start)..<end where !isHidden(i) { return false }
        return true
    }

    /// Where an arrow key may stop: never inside hidden syntax, never on a
    /// collapsed line, at the left edge of inline syntax only (as in the
    /// GTK editor, whose invisible text the cursor skips), and never on an
    /// image (see `objectZone`).
    private func isStop(_ p: Int) -> Bool {
        if object(at: p) != nil { return objectZone(at: p) == nil }
        if Int(analysis.visualPos(pos: UInt32(p))) != p || tableRowStarts.contains(p) { return false }
        if inCollapsedLine(p) { return false }
        if p == 0 || !isHidden(p - 1) { return true }
        let li = Int(analysis.lineIndex(pos: UInt32(p)))
        return li < lines.count && Int(lines[li].contentStart) == p
    }

    /// The next place the cursor may go after `p`, in text order: a stop,
    /// as an empty range, or an image, which it selects whole.
    private func landing(from p: Int, forward: Bool) -> NSRange {
        let s = string as NSString
        let len = s.length
        var q = p
        repeat {
            if forward {
                guard q < len else { return NSRange(location: len, length: 0) }
                q = NSMaxRange(s.rangeOfComposedCharacterSequence(at: q))
            } else {
                guard q > 0 else { return NSRange(location: 0, length: 0) }
                q = s.rangeOfComposedCharacterSequence(at: q - 1).location
            }
            if let o = objectZone(at: q) { return o.range }
        } while !isStop(q) && q > 0 && q < len
        return NSRange(location: q, length: 0)
    }

    /// Where the moving end of a selection goes from `x`: to the next stop,
    /// or over an image whole.
    private func extendedEnd(from x: Int, forward: Bool) -> Int {
        if let o = object(at: x), x != (forward ? o.end : o.start) { return forward ? o.end : o.start }
        let l = landing(from: x, forward: forward)
        return forward ? NSMaxRange(l) : l.location
    }

    /// `a..<b`, over any image an end falls inside of, whole; empty on an
    /// image, the image.
    private func covering(_ a: Int, _ b: Int) -> NSRange {
        var (a, b) = (a, b)
        if let o = object(at: a), a > o.start, a < o.end { a = o.start }
        if let o = object(at: b), b > o.start, b < o.end { b = o.end }
        if a == b, let o = objectZone(at: a) { return o.range }
        return NSRange(location: a, length: b - a)
    }

    /// Left and right in a table row move from stop to stop in the text:
    /// TextKit's own movement lands on the gaps laid out before cells.
    private var inTableRow: Bool {
        guard !sourceMode, !stale, !lines.isEmpty else { return false }
        let li = Int(analysis.lineIndex(pos: UInt32(cursor)))
        return li < lines.count && lines[li].kind == .table
    }

    /// The fixed end of a selection extended from stop to stop.
    private var stopsAnchor: (anchor: Int, selection: NSRange)?

    /// Whether Left and Right go from stop to stop here, rather than as
    /// TextKit moves: in a table row, from a selected image, and on with a
    /// selection extended that way.
    private var movesByStops: Bool {
        inTableRow || selectedObject != nil || (stopsAnchor.map { $0.selection == selectedRange() } ?? false)
    }

    private func moveByStops(forward: Bool, extending: Bool) {
        let sel = selectedRange()
        var anchor: Int?
        let target: NSRange
        if !extending {
            if let o = selectedObject {
                target = landing(from: forward ? o.end : o.start, forward: forward)
            } else if sel.length > 0 {
                target = NSRange(location: forward ? NSMaxRange(sel) : sel.location, length: 0)
            } else {
                target = landing(from: sel.location, forward: forward)
            }
        } else {
            // A selected image extends from its far side.
            let a =
                stopsAnchor.flatMap { $0.selection == sel ? $0.anchor : nil }
                ?? selectedObject.map { forward ? $0.start : $0.end } ?? sel.location
            let head = extendedEnd(from: a == sel.location ? NSMaxRange(sel) : sel.location, forward: forward)
            target = covering(min(a, head), max(a, head))
            anchor = a
        }
        raw += 1
        setSelectedRange(target)
        raw -= 1
        stopsAnchor = anchor.map { ($0, selectedRange()) }
        scrollRangeToVisible(NSRange(location: cursor, length: 0))
    }

    override func moveLeft(_ sender: Any?) {
        movesByStops ? moveByStops(forward: false, extending: false) : super.moveLeft(sender)
    }

    override func moveRight(_ sender: Any?) {
        movesByStops ? moveByStops(forward: true, extending: false) : super.moveRight(sender)
    }

    override func moveLeftAndModifySelection(_ sender: Any?) {
        movesByStops ? moveByStops(forward: false, extending: true) : super.moveLeftAndModifySelection(sender)
    }

    override func moveRightAndModifySelection(_ sender: Any?) {
        movesByStops ? moveByStops(forward: true, extending: true) : super.moveRightAndModifySelection(sender)
    }

    override func moveUp(_ sender: Any?) { passingImages(down: false) { super.moveUp(sender) } }
    override func moveDown(_ sender: Any?) { passingImages(down: true) { super.moveDown(sender) } }
    override func moveUpAndModifySelection(_ sender: Any?) {
        passingImages(down: false) { super.moveUpAndModifySelection(sender) }
    }
    override func moveDownAndModifySelection(_ sender: Any?) {
        passingImages(down: true) { super.moveDownAndModifySelection(sender) }
    }

    /// Up and Down pass an image keeping the column: a move that lands on
    /// one, or on a line laid out as nothing, moves on, as AppKit moves, so
    /// it keeps its column. An image's hidden source is laid out at the end
    /// of the line above it, so a move onto that line past its text lands
    /// on the image too: the line it went to is the one next to where it
    /// started, and there it stops at the end of the text.
    private func passingImages(down: Bool, _ move: () -> Void) {
        guard !objects.isEmpty, !sourceMode, !stale else { return move() }
        let before = selectedRange()
        func head() -> Int {
            let r = selectedRange()
            return r.location != before.location ? r.location : NSMaxRange(r)
        }
        /// The selection with its moving end at `p`.
        func headAt(_ p: Int) -> NSRange {
            let r = selectedRange()
            guard r.length > 0 else { return NSRange(location: p, length: 0) }
            let anchor = r.location != before.location ? NSMaxRange(r) : r.location
            return NSRange(location: min(anchor, p), length: abs(anchor - p))
        }
        movingVertically = true
        var from = down ? NSMaxRange(before) : before.location
        move()
        var last = selectedRange()
        while true {
            let h = head()
            // The image of `h`'s line, which `h` is on or in the hidden
            // prefix of.
            let li = lines.isEmpty ? 0 : Int(analysis.lineIndex(pos: UInt32(h)))
            if let i = objectOnLine[li], h <= objects[i].end, objectZone(at: max(h, objects[i].start)) != nil {
                let o = objects[i]
                guard let next = lineTop(nextTo: from, down: down), next == lineTop(of: o.start) else {
                    // On the line next to where it started, past its text.
                    last = headAt(landing(from: min(h, o.start), forward: false).location)
                    break
                }
            } else if !inCollapsedLine(h) {
                break
            }
            from = h
            move()
            if selectedRange() == last { break }
            last = selectedRange()
        }
        movingVertically = false
        let fixed = adjusted(last, from: before)
        if fixed != selectedRange() {
            raw += 1
            setSelectedRange(fixed)
            raw -= 1
        }
    }

    /// The top of the line fragment holding character `p`.
    private func lineTop(of p: Int) -> CGFloat? {
        guard let lm = layoutManager, lm.numberOfGlyphs > 0 else { return nil }
        return lm.lineFragmentRect(
            forGlyphAt: min(lm.glyphIndexForCharacter(at: p), lm.numberOfGlyphs - 1), effectiveRange: nil
        ).minY
    }

    /// The top of the shown line fragment below (or above) the one holding
    /// character `p`.
    private func lineTop(nextTo p: Int, down: Bool) -> CGFloat? {
        guard let lm = layoutManager, lm.numberOfGlyphs > 0 else { return nil }
        var range = NSRange()
        _ = lm.lineFragmentRect(
            forGlyphAt: min(lm.glyphIndexForCharacter(at: p), lm.numberOfGlyphs - 1), effectiveRange: &range)
        while true {
            let g = down ? NSMaxRange(range) : range.location - 1
            guard g >= 0, g < lm.numberOfGlyphs else { return nil }
            let rect = lm.lineFragmentRect(forGlyphAt: g, effectiveRange: &range)
            if rect.height > 0 { return rect.minY }
        }
    }

    /// Every selection change passes here: keyboard movement of any kind
    /// (in either writing direction), clicks and drags. Margin edits one
    /// range at a time, so a multiple selection becomes its first range.
    func textView(
        _ textView: NSTextView, willChangeSelectionFromCharacterRanges old: [NSValue], toCharacterRanges new: [NSValue]
    ) -> [NSValue] {
        guard raw == 0, !sourceMode, !stale, !hasMarkedText(), !movingVertically,
            let r = new.first?.rangeValue, let o = old.first?.rangeValue
        else { return new }
        return [NSValue(range: adjusted(r, from: o))]
    }

    /// Where selection `r`, coming from `o`, goes. The cursor never rests
    /// in hidden syntax, on a collapsed line or on an image; a key movement
    /// that lands there continues in its direction, anything else snaps to
    /// where the text starts. Landing on an image, by a key toward it or a
    /// click, selects it, and a selection takes an image whole.
    private func adjusted(_ r: NSRange, from o: NSRange) -> NSRange {
        if r.length == 0 {
            let p = r.location
            if isStop(p) { return r }
            if let obj = objectZone(at: p) { return obj.range }
            let stepped = o.length == 0 && abs(p - o.location) <= 2
            var q =
                stepped || inCollapsedLine(p)
                ? landing(from: p, forward: p > o.location)
                : NSRange(location: Int(analysis.visualPos(pos: UInt32(p))), length: 0)
            // In a table's hidden padding, which the core does not know of.
            if q.length == 0 && !isStop(q.location) { q = landing(from: q.location, forward: true) }
            return q
        }
        // Extending a selection: its moving end skips hidden syntax too.
        var (a, b) = (r.location, NSMaxRange(r))
        if a == o.location, b != NSMaxRange(o), !isStop(b) {
            b = extendedEnd(from: b, forward: b > NSMaxRange(o))
        } else if b == NSMaxRange(o), a != o.location, !isStop(a) {
            a = extendedEnd(from: a, forward: a > o.location)
        }
        return covering(min(a, b), max(a, b))
    }

    // MARK: - Editing through the core

    /// Applies an editing plan as one undoable step.
    func apply(_ plan: EditPlan) {
        if plan.changes.isEmpty {
            setSelection(plan)
            return
        }
        raw += 1
        breakUndoCoalescing()
        undoManager?.beginUndoGrouping()
        var applied = false
        for c in plan.changes.reversed() {
            let r = NSRange(from: c.start, to: c.end)
            if shouldChangeText(in: r, replacementString: c.text) {
                textStorage?.replaceCharacters(in: r, with: c.text)
                applied = true
            }
        }
        if applied { didChangeText() }
        undoManager?.endUndoGrouping()
        raw -= 1
        setSelection(plan)
        scrollRangeToVisible(selectedRange())
    }

    private func setSelection(_ plan: EditPlan) {
        raw += 1
        if let s = plan.selection {
            setSelectedRange(NSRange(s))
        } else {
            setSelectedRange(NSRange(location: Int(plan.cursor), length: 0))
        }
        raw -= 1
        updateReveal()
        onSelectionChange?()
    }

    /// Runs an editing command against the cursor and selection.
    func run(_ command: (Analysis, Int, NSRange) -> EditPlan?) {
        ensureFresh()
        if let plan = command(analysis, cursor, selectedRange()) {
            apply(plan)
        }
    }

    /// Several commands as one undo step.
    private func grouped(_ body: () -> Void) {
        breakUndoCoalescing()
        undoManager?.beginUndoGrouping()
        body()
        undoManager?.endUndoGrouping()
    }

    private func deleteSelectionThroughCore() {
        if let sel = selection {
            run { a, _, _ in a.deleteRange(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel))) }
        }
    }

    /// An input method starts composing: at the place typed text would go.
    /// The composition itself is the input method's; the committed text is
    /// styled and analyzed as usual.
    override func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        if !hasMarkedText() && !sourceMode && replacementRange.location == NSNotFound && selection == nil {
            ensureFresh()
            let p = Int(analysis.insertionPoint(pos: UInt32(cursor)))
            if p != cursor {
                raw += 1
                setSelectedRange(NSRange(location: p, length: 0))
                raw -= 1
            }
        }
        raw += 1
        super.setMarkedText(string, selectedRange: selectedRange, replacementRange: replacementRange)
        raw -= 1
    }

    override func unmarkText() {
        raw += 1
        super.unmarkText()
        raw -= 1
        refresh()
    }

    override func insertText(_ insertString: Any, replacementRange: NSRange) {
        let text = (insertString as? NSAttributedString)?.string ?? (insertString as? String) ?? ""
        if sourceMode || hasMarkedText() || text.isEmpty {
            let composing = hasMarkedText()
            raw += 1
            super.insertText(insertString, replacementRange: replacementRange)
            raw -= 1
            if composing { refresh() }
            return
        }
        if replacementRange.location != NSNotFound && replacementRange != selectedRange() {
            setSelectedRange(replacementRange)
        }
        if text == "\n" || text == "\r" {
            insertNewline(nil)
            return
        }
        if text == "\t" {
            insertTab(nil)
            return
        }
        if let sel = selection {
            run { a, _, _ in a.replaceRange(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel)), text: text) }
            return
        }
        grouped {
            ensureFresh()
            let p = cursor
            let plan = analysis.insert(pos: UInt32(p), text: text)
            let n = (text as NSString).length
            // Plain typing: let the text view insert it, which keeps its
            // native undo coalescing.
            if plan.changes.count == 1, plan.selection == nil,
                Int(plan.changes[0].start) == p, Int(plan.changes[0].end) == p,
                plan.changes[0].text == text, Int(plan.cursor) == p + n
            {
                raw += 1
                super.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
                raw -= 1
            } else {
                apply(plan)
            }
        }
    }

    override func insertNewline(_ sender: Any?) {
        if sourceMode { return super.insertNewline(sender) }
        newline(soft: NSApp.currentEvent?.modifierFlags.contains(.shift) ?? false)
    }

    override func insertLineBreak(_ sender: Any?) {
        if sourceMode { return super.insertLineBreak(sender) }
        newline(soft: true)
    }

    override func insertNewlineIgnoringFieldEditor(_ sender: Any?) {
        if sourceMode { return super.insertNewlineIgnoringFieldEditor(sender) }
        newline(soft: false)
    }

    private func newline(soft: Bool) {
        grouped {
            deleteSelectionThroughCore()
            run { a, c, _ in a.newline(pos: UInt32(c), soft: soft) }
        }
    }

    override func insertTab(_ sender: Any?) {
        if sourceMode { return super.insertTab(sender) }
        indentLines(outdent: false)
    }

    override func insertBacktab(_ sender: Any?) {
        if sourceMode { return super.insertBacktab(sender) }
        indentLines(outdent: true)
    }

    override func deleteBackward(_ sender: Any?) {
        if sourceMode { return super.deleteBackward(sender) }
        if selection != nil { return deleteSelectionThroughCore() }
        ensureFresh()
        let p = cursor
        guard p > 0 else { return }
        let plan = analysis.backspace(pos: UInt32(p))
        let s = string as NSString
        let char = s.rangeOfComposedCharacterSequence(at: p - 1)
        if plan.changes.count == 1, plan.selection == nil, plan.changes[0].text.isEmpty,
            Int(plan.changes[0].start) == char.location, Int(plan.changes[0].end) == p,
            Int(plan.cursor) == char.location
        {
            raw += 1
            super.deleteBackward(sender)
            raw -= 1
        } else {
            apply(plan)
        }
    }

    override func deleteForward(_ sender: Any?) {
        if sourceMode { return super.deleteForward(sender) }
        if selection != nil { return deleteSelectionThroughCore() }
        run { a, c, _ in a.deleteForward(pos: UInt32(c)) }
    }

    /// Any other edit (deleting a word, dragging text, a spelling
    /// correction, Transpose, Writing Tools) arrives here, and goes through
    /// the editing rules too: the change is replaced by the core's.
    /// Edits made directly, not through the core: ours (already planned),
    /// undo and redo, Show Markdown, and input methods composing.
    private var editsAreRaw: Bool {
        raw > 0 || sourceMode || hasMarkedText() || undoManager?.isUndoing == true || undoManager?.isRedoing == true
    }

    override func shouldChangeText(inRanges affectedRanges: [NSValue], replacementStrings: [String]?) -> Bool {
        guard !editsAreRaw, let strings = replacementStrings, strings.count == affectedRanges.count else {
            return super.shouldChangeText(inRanges: affectedRanges, replacementStrings: replacementStrings)
        }
        guard isEditable else { return false }
        let changes = zip(affectedRanges.map(\.rangeValue), strings).sorted { $0.0.location > $1.0.location }
        grouped {
            for (range, text) in changes {
                // A replacement (a spelling correction, Writing Tools) is
                // typing over a selection.
                if range.length > 0, !text.isEmpty {
                    run { a, _, _ in
                        a.replaceRange(start: UInt32(range.location), end: UInt32(NSMaxRange(range)), text: text)
                    }
                    continue
                }
                if range.length > 0 {
                    run { a, _, _ in a.deleteRange(start: UInt32(range.location), end: UInt32(NSMaxRange(range))) }
                } else {
                    raw += 1
                    setSelectedRange(NSRange(location: range.location, length: 0))
                    raw -= 1
                }
                if !text.isEmpty {
                    run { a, c, _ in a.insert(pos: UInt32(c), text: text) }
                }
            }
        }
        return false
    }

    override func shouldChangeText(in affectedCharRange: NSRange, replacementString: String?) -> Bool {
        guard !editsAreRaw, replacementString != nil else {
            return super.shouldChangeText(in: affectedCharRange, replacementString: replacementString)
        }
        return shouldChangeText(
            inRanges: [NSValue(range: affectedCharRange)], replacementStrings: replacementString.map { [$0] })
    }

    // MARK: - Clipboard

    override func copy(_ sender: Any?) {
        guard let sel = selection else { return }
        ensureFresh()
        let text =
            sourceMode
            ? (string as NSString).substring(with: sel)
            : analysis.copySource(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel)))
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
    }

    override func cut(_ sender: Any?) {
        guard selection != nil else { return }
        copy(sender)
        if sourceMode {
            raw += 1
            super.delete(sender)
            raw -= 1
        } else {
            deleteSelectionThroughCore()
        }
    }

    override func paste(_ sender: Any?) {
        guard let text = NSPasteboard.general.string(forType: .string) else { return }
        pasteText(text)
    }

    override func pasteAsPlainText(_ sender: Any?) { paste(sender) }
    override func pasteAsRichText(_ sender: Any?) { paste(sender) }

    func pasteText(_ text: String) {
        let text = text.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n")
        if sourceMode {
            raw += 1
            insertText(text, replacementRange: selectedRange())
            raw -= 1
            return
        }
        run { a, c, sel in
            sel.length > 0
                ? a.replaceRange(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel)), text: text)
                : a.insert(pos: UInt32(c), text: text)
        }
    }

    override func writeSelection(to pboard: NSPasteboard, type: NSPasteboard.PasteboardType) -> Bool {
        guard type == .string, let sel = selection else { return super.writeSelection(to: pboard, type: type) }
        ensureFresh()
        let text =
            sourceMode
            ? (string as NSString).substring(with: sel)
            : analysis.copySource(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel)))
        return pboard.setString(text, forType: .string)
    }

    // MARK: - Formatting commands

    private var formattingAllowed: Bool { !sourceMode }

    private func inline(_ style: InlineStyle) {
        guard formattingAllowed else { return }
        run { a, c, sel in
            let r = sel.length > 0 ? sel : NSRange(location: c, length: 0)
            return a.toggleInline(start: UInt32(r.location), end: UInt32(NSMaxRange(r)), style: style)
        }
    }

    private func block(_ style: BlockStyle) {
        guard formattingAllowed else { return }
        run { a, c, sel in
            let r = sel.length > 0 ? sel : NSRange(location: c, length: 0)
            return a.setBlock(start: UInt32(r.location), end: UInt32(NSMaxRange(r)), style: style)
        }
    }

    func indentLines(outdent: Bool) {
        guard formattingAllowed else { return }
        run { a, c, sel in
            let r = sel.length > 0 ? sel : NSRange(location: c, length: 0)
            return a.indent(start: UInt32(r.location), end: UInt32(NSMaxRange(r)), outdent: outdent)
        }
    }

    @objc func marginBold(_ sender: Any?) { inline(.bold) }
    @objc func marginItalic(_ sender: Any?) { inline(.italic) }
    @objc func marginStrikethrough(_ sender: Any?) { inline(.strikethrough) }
    @objc func marginInlineCode(_ sender: Any?) { inline(.code) }
    @objc func marginNormalText(_ sender: Any?) { block(.paragraph) }
    @objc func marginHeading(_ sender: Any?) {
        let level = UInt8(clamping: (sender as? NSMenuItem)?.tag ?? 1)
        block(.heading(level: level))
    }
    @objc func marginBulletedList(_ sender: Any?) { block(.bulleted) }
    @objc func marginNumberedList(_ sender: Any?) { block(.numbered) }
    @objc func marginChecklist(_ sender: Any?) { block(.checklist) }
    @objc func marginQuote(_ sender: Any?) { block(.quote) }
    @objc func marginCodeBlock(_ sender: Any?) { block(.codeBlock) }
    @objc func marginIndent(_ sender: Any?) { indentLines(outdent: false) }
    @objc func marginOutdent(_ sender: Any?) { indentLines(outdent: true) }

    @objc func marginToggleTask(_ sender: Any?) {
        guard formattingAllowed else { return }
        ensureFresh()
        if let item = analysis.taskOnLineOf(pos: UInt32(cursor)) {
            apply(analysis.toggleTask(item: item, cursor: UInt32(cursor)))
        }
    }

    @objc func marginOpenLink(_ sender: Any?) {
        ensureFresh()
        if let url = analysis.linkAtCursor(pos: UInt32(cursor)) {
            onOpenLink?(url)
        }
    }

    @objc func marginLink(_ sender: Any?) {
        guard formattingAllowed, let window else { return }
        ensureFresh()
        let current = analysis.linkAtCursor(pos: UInt32(cursor))
        let sel = selectedRange()
        let at = cursor
        let alert = NSAlert()
        alert.messageText = current == nil ? "Insert Link" : "Edit Link"
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 300, height: 24))
        field.placeholderString = "https://…"
        field.stringValue = current ?? ""
        alert.accessoryView = field
        alert.addButton(withTitle: "Apply")
        alert.addButton(withTitle: "Cancel")
        if current != nil {
            let remove = alert.addButton(withTitle: "Remove Link")
            remove.hasDestructiveAction = true
        }
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self else { return }
            self.ensureFresh()
            switch response {
            case .alertFirstButtonReturn:
                let url = field.stringValue.trimmingCharacters(in: .whitespaces)
                if !url.isEmpty {
                    let r = sel.length > 0 ? sel : NSRange(location: at, length: 0)
                    self.apply(self.analysis.makeLink(start: UInt32(r.location), end: UInt32(NSMaxRange(r)), url: url))
                }
            case .alertThirdButtonReturn:
                if let plan = self.analysis.removeLink(pos: UInt32(at)) {
                    self.apply(plan)
                }
            default:
                break
            }
            self.window?.makeFirstResponder(self)
        }
    }

    /// The text's right-click menu leads with Comment on Selection, as in
    /// Pages and Preview.
    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = super.menu(for: event) ?? NSMenu()
        menu.insertItem(
            NSMenuItem(
                title: "Comment on Selection", action: #selector(DocumentWindow.marginCommentOnSelection(_:)),
                keyEquivalent: ""), at: 0)
        menu.insertItem(.separator(), at: 1)
        return menu
    }

    override func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(marginBold(_:)), #selector(marginItalic(_:)), #selector(marginStrikethrough(_:)),
            #selector(marginInlineCode(_:)), #selector(marginNormalText(_:)), #selector(marginHeading(_:)),
            #selector(marginBulletedList(_:)), #selector(marginNumberedList(_:)), #selector(marginChecklist(_:)),
            #selector(marginQuote(_:)), #selector(marginCodeBlock(_:)), #selector(marginIndent(_:)),
            #selector(marginOutdent(_:)), #selector(marginToggleTask(_:)), #selector(marginLink(_:)):
            return formattingAllowed
        default:
            return super.validateMenuItem(item)
        }
    }

    /// Cmd+Shift+7, 8 and 9 by physical key, as Google Docs does: as
    /// characters they are Cmd+&, Cmd+* and Cmd+( on a US layout, and other
    /// things elsewhere.
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let mods = event.modifierFlags.intersection([.command, .shift, .option, .control])
        if mods == [.command, .shift], window?.firstResponder === self {
            switch event.keyCode {
            case 26: marginNumberedList(nil); return true
            case 28: marginBulletedList(nil); return true
            case 25: marginChecklist(nil); return true
            default: break
            }
        }
        return super.performKeyEquivalent(with: event)
    }
}

/// Fills backgrounds (selection, comments, find, inline code), with two
/// corrections to TextKit:
///
/// - Where a paragraph starts inside a line fragment, after a newline laid
///   out as a space while reflowing, the rectangles come from glyph
///   locations. TextKit's for such a paragraph's text are wrong: the whole
///   fragment for the text on it, and empty on the fragments after.
/// - It leaves the lines images take to them: the selection and highlights
///   there are drawn by each image itself, over it (see
///   `DocTextView.drawObjects`), not behind it.
nonisolated final class DocLayoutManager: NSLayoutManager {
    weak var view: DocTextView?
    private var origin = NSPoint.zero

    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        self.origin = origin
        super.drawBackground(forGlyphRange: glyphsToShow, at: origin)
    }

    override func fillBackgroundRectArray(
        _ rectArray: UnsafePointer<NSRect>, count rectCount: Int, forCharacterRange charRange: NSRange,
        color: NSColor
    ) {
        guard charRange.length > 0, touchesJoinedParagraph(charRange) else {
            return fill(rectArray, count: rectCount, forCharacterRange: charRange, color: color)
        }
        let glyphs = glyphRange(forCharacterRange: charRange, actualCharacterRange: nil)
        var rects: [NSRect] = []
        enumerateLineFragments(forGlyphRange: glyphs) { frag, used, _, line, _ in
            let start = max(glyphs.location, line.location)
            let end = min(NSMaxRange(glyphs), NSMaxRange(line))
            guard start < end else { return }
            // Glyph locations are from the fragment's origin.
            let x0 = frag.minX + self.location(forGlyphAt: start).x
            let x1 = end < NSMaxRange(line) ? frag.minX + self.location(forGlyphAt: end).x : used.maxX
            guard x1 > x0 else { return }
            rects.append(
                NSRect(x: x0, y: frag.minY, width: x1 - x0, height: frag.height)
                    .offsetBy(dx: self.origin.x, dy: self.origin.y))
        }
        guard !rects.isEmpty else { return }
        rects.withUnsafeBufferPointer {
            fill($0.baseAddress!, count: $0.count, forCharacterRange: charRange, color: color)
        }
    }

    /// Fills `rectArray`, except on the lines images take.
    private func fill(
        _ rectArray: UnsafePointer<NSRect>, count rectCount: Int, forCharacterRange charRange: NSRange,
        color: NSColor
    ) {
        let rects = UnsafeBufferPointer(start: rectArray, count: rectCount)
        let bounds = rects.reduce(NSRect.null) { $0.union($1) }
        // Drawing is on the main thread.
        let view = view
        let lines = MainActor.assumeIsolated { view?.objectLines() ?? [] }.filter { $0.intersects(bounds) }
        guard !lines.isEmpty else {
            return super.fillBackgroundRectArray(
                rectArray, count: rectCount, forCharacterRange: charRange, color: color)
        }
        NSGraphicsContext.saveGraphicsState()
        let clip = NSBezierPath(rect: bounds.insetBy(dx: -1, dy: -1))
        for l in lines { clip.appendRect(l) }
        clip.windingRule = .evenOdd
        clip.addClip()
        super.fillBackgroundRectArray(rectArray, count: rectCount, forCharacterRange: charRange, color: color)
        NSGraphicsContext.restoreGraphicsState()
    }

    /// Whether `range` reaches into a paragraph that starts inside the line
    /// fragment of the newline before it.
    private func touchesJoinedParagraph(_ range: NSRange) -> Bool {
        guard let s = textStorage?.string as NSString? else { return false }
        var p = s.paragraphRange(for: NSRange(location: range.location, length: 0)).location
        while p < NSMaxRange(range) {
            if p > 0 {
                var line = NSRange()
                lineFragmentRect(forGlyphAt: glyphIndexForCharacter(at: p - 1), effectiveRange: &line)
                if NSLocationInRange(glyphIndexForCharacter(at: p), line) { return true }
            }
            let next = NSMaxRange(s.paragraphRange(for: NSRange(location: p, length: 0)))
            if next <= p { break }
            p = next
        }
        return false
    }
}

/// Hides Markdown syntax by laying it out as null glyphs, collapses lines
/// that are entirely hidden (blank lines, fences), and lays out newlines
/// inside paragraphs as spaces while reflowing.
final class HidingLayoutDelegate: NSObject, NSLayoutManagerDelegate {
    weak var view: DocTextView?

    func layoutManager(
        _ layoutManager: NSLayoutManager, shouldGenerateGlyphs glyphs: UnsafePointer<CGGlyph>,
        properties props: UnsafePointer<NSLayoutManager.GlyphProperty>,
        characterIndexes charIndexes: UnsafePointer<Int>, font aFont: NSFont, forGlyphRange glyphRange: NSRange
    ) -> Int {
        guard let storage = layoutManager.textStorage, glyphRange.length > 0 else { return 0 }
        let n = glyphRange.length
        let first = charIndexes[0]
        let last = charIndexes[n - 1]
        var any = false
        let range = NSRange(location: first, length: last - first + 1)
        for key in [NSAttributedString.Key.marginHidden, .marginTableGap, .marginObject] where !any {
            storage.enumerateAttribute(key, in: range, options: []) { v, _, stop in
                if v != nil { any = true; stop.pointee = true }
            }
        }
        if !any { return 0 }
        let s = storage.string as NSString
        var newProps = Array(UnsafeBufferPointer(start: props, count: n))
        for i in 0..<n {
            let ci = charIndexes[i]
            // Newlines stay: they end lines. Lines hidden entirely are
            // collapsed in `shouldSetLineFragmentRect` instead.
            if storage.attribute(.marginHidden, at: ci, effectiveRange: nil) != nil && s.character(at: ci) != 10 {
                newProps[i] = .null
            } else if storage.attribute(.marginTableGap, at: ci, effectiveRange: nil) != nil
                || storage.attribute(.marginObject, at: ci, effectiveRange: nil) != nil
            {
                newProps[i] = .controlCharacter
            }
        }
        newProps.withUnsafeBufferPointer { buf in
            layoutManager.setGlyphs(
                glyphs, properties: buf.baseAddress!, characterIndexes: charIndexes, font: aFont,
                forGlyphRange: glyphRange)
        }
        return n
    }

    func layoutManager(
        _ layoutManager: NSLayoutManager, shouldUse action: NSLayoutManager.ControlCharacterAction,
        forControlCharacterAt charIndex: Int
    ) -> NSLayoutManager.ControlCharacterAction {
        if let v = view, v.reflowsParagraphs, !v.sourceMode, v.softBreaks.contains(charIndex) {
            return .whitespace
        }
        if view?.tableGaps[charIndex] != nil || view?.objectWidth(at: charIndex) != nil {
            return .whitespace
        }
        return action
    }

    /// A newline laid out as whitespace (Reflow Paragraphs) is as wide as a
    /// space; a table cell's gap reaches to where the cell's text starts;
    /// an image's character is as wide as the image.
    func layoutManager(
        _ layoutManager: NSLayoutManager, boundingBoxForControlGlyphAt glyphIndex: Int,
        for textContainer: NSTextContainer, proposedLineFragment proposedRect: NSRect, glyphPosition: NSPoint,
        characterIndex charIndex: Int
    ) -> NSRect {
        if let x = view?.tableGaps[charIndex] {
            return NSRect(x: glyphPosition.x, y: 0, width: max(0, x - glyphPosition.x), height: 0)
        }
        if let w = view?.objectWidth(at: charIndex) {
            return NSRect(x: glyphPosition.x, y: 0, width: w, height: 0)
        }
        let font =
            layoutManager.textStorage?.attribute(.font, at: charIndex, effectiveRange: nil) as? NSFont
            ?? Theme.font(size: Theme.bodySize)
        let width = (" " as NSString).size(withAttributes: [.font: font]).width
        return NSRect(x: glyphPosition.x, y: 0, width: width, height: 0)
    }

    func layoutManager(
        _ layoutManager: NSLayoutManager, shouldSetLineFragmentRect lineFragmentRect: UnsafeMutablePointer<NSRect>,
        lineFragmentUsedRect: UnsafeMutablePointer<NSRect>, baselineOffset: UnsafeMutablePointer<CGFloat>,
        in textContainer: NSTextContainer, forGlyphRange glyphRange: NSRange
    ) -> Bool {
        guard let storage = layoutManager.textStorage, glyphRange.length > 0 else { return false }
        let chars = layoutManager.characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil)
        guard chars.length > 0, NSMaxRange(chars) <= storage.length else { return false }
        // Null glyphs after a newline join the fragment before, so a line's
        // hidden prefix and any hidden blank lines before it end the
        // previous line's fragment, or make one of their own: collapse that.
        var effective = NSRange()
        let hidden =
            storage.attribute(.marginHidden, at: chars.location, longestEffectiveRange: &effective, in: chars) != nil
        if hidden && NSMaxRange(effective) >= NSMaxRange(chars) {
            lineFragmentRect.pointee.size.height = 0
            lineFragmentUsedRect.pointee.size.height = 0
            baselineOffset.pointee = 0
            return true
        }
        // For the same reason a line whose prefix is hidden starts its
        // fragment in the middle of its paragraph, where TextKit leaves out
        // the paragraph's space above: add it here.
        guard let v = view, !v.lines.isEmpty else { return false }
        let li = Int(v.analysis.lineIndex(pos: UInt32(chars.location)))
        guard li < v.lines.count, li < v.styler.metrics.count else { return false }
        let line = v.lines[li]
        let metrics = v.styler.metrics[li]
        var changed = false
        func grow(_ h: CGFloat) {
            lineFragmentRect.pointee.size.height += h
            lineFragmentUsedRect.pointee.size.height += h
            baselineOffset.pointee += h
            changed = true
        }
        if !v.stale, metrics.spaceAbove > 0, chars.location > Int(line.start),
            (Int(line.start)..<chars.location).allSatisfy({
                storage.attribute(.marginHidden, at: $0, effectiveRange: nil) != nil
            })
        {
            grow(metrics.spaceAbove)
        }
        // An image's line is as tall as the image, between the space above
        // it and the line spacing below.
        if let h = v.objectHeight(inFragment: chars) {
            let text = lineFragmentRect.pointee.height - metrics.spaceAbove - metrics.lineSpacing
            grow(h - text)
        }
        return changed
    }
}
