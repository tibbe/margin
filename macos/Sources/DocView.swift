import AppKit
import margin_ffi

/// The document view. Its text is the Markdown file; what it shows is the
/// core's projection of it (`Projection`): shown paragraphs, with hidden
/// syntax left out, soft line breaks shown as spaces while reflowing, table
/// cells set at their columns and images and diagrams shown as objects. It
/// lays them out with Core Text and maps every position through the
/// projection, so what is on screen and the file agree by construction.
///
/// Positions are the file's, in UTF-16 units. Every edit goes through the
/// core's editing rules, which answer with minimal changes to the source.
final class DocView: NSView, NSTextInputClient, NSViewToolTipOwner, NSMenuItemValidation, NSUserInterfaceValidations {
    /// The source, the model.
    private(set) var text = NSMutableString()
    var string: String { text as String }
    private(set) var analysis = Analysis(text: "")
    private(set) var projection: Projection
    /// The shown paragraphs, from `projection`.
    private(set) var shown: [ShownParagraph] = []
    private(set) var lines: [LineInfo] = []
    private(set) var items: [ItemInfo] = []
    private(set) var quotes: [QuoteInfo] = []
    private(set) var codeBlocks: [CodeBlockInfo] = []
    var tables: [TableGrid] = []
    private(set) var tableInfos: [TableInfo] = []
    private(set) var imageBlocks: [ImageBlockInfo] = []
    private(set) var diagramBlocks: [DiagramBlockInfo] = []
    /// The image blocks shown as images, and the diagrams (none in Show
    /// Markdown), in order.
    private(set) var objects: [DocObject] = []
    private(set) var objectOnLine: [Int: Int] = [:]
    private var objectAtChar: [Int: Int] = [:]
    /// The source, styled as the analysis says (fonts, colors, the
    /// paragraph style giving each line's indent and spacing). Never laid
    /// out: shown paragraphs take their attributes from it.
    let styled = NSTextStorage()
    let styler = Styler()
    /// Syntax the cursor needs to see (a blank line, a code block's fences).
    private(set) var revealed: [NSRange] = []
    /// Links and images, for their tooltips.
    var linkRanges: [NSRange] {
        Span.decode(analysis.spansPacked()).filter {
            ($0.code == .link || $0.code == .image) && object(at: $0.start) == nil
        }.map { NSRange(location: $0.start, length: $0.end - $0.start) }
    }
    /// Always fresh: the analysis follows every edit at once.
    let stale = false

    /// The folder image paths are relative to: the document's.
    var imageFolder = NSHomeDirectory() {
        didSet {
            if oldValue != imageFolder {
                updateObjects()
                relayout()
            }
        }
    }
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
        didSet { if oldValue != reflowsParagraphs { reproject() } }
    }

    // MARK: Callbacks, as the document window and comments use them

    /// The text changed.
    var onChange: (() -> Void)?
    /// Each edit to the characters as it is made, for anchors: the range
    /// of what replaced the old text, in the new text, and the change in
    /// length.
    var onEdit: ((NSRange, Int) -> Void)?
    var onSelectionChange: (() -> Void)?
    /// Positions of lines changed: the gutter follows.
    var onLayout: (() -> Void)?
    var onOpenLink: ((String) -> Void)?
    /// Escape: leave a comment, close Find.
    var onEscape: (() -> Void)?
    /// Highlights to recompute: the text or how it shows changed.
    var onHighlight: (() -> Void)?
    /// Asks the page to lay out again (the text size changed).
    var onRetile: (() -> Void)?
    /// An image loaded or failed to.
    var onImagesChanged: (() -> Void)?

    /// Printing lays the text out across the whole page.
    var usesFullWidth = false
    var geometry = PageGeometry(width: 1200, scale: 1, hasCards: false)
    /// Space above and below the text.
    var textContainerInset = NSSize(width: 0, height: 28) {
        didSet { relayout() }
    }
    var backgroundColor: NSColor = .textBackgroundColor {
        didSet { needsDisplay = true }
    }
    /// The view is at least this tall (the window, from the page).
    var minSize = NSSize.zero
    /// Kept for callers of a text view's API.
    var isVerticallyResizable = true
    /// Spelling underlines, as Edit > Spelling and Grammar sets them.
    var isContinuousSpellCheckingEnabled = true {
        didSet { needsDisplay = true }
    }
    /// The spell checker's document: words ignored here are ignored here.
    let spellTag = NSSpellChecker.uniqueSpellDocumentTag()
    /// Misspelled words per typeset paragraph, as ranges of its text.
    var misspellings: [ObjectIdentifier: [NSRange]] = [:]

    /// Comment and find highlights, later ones over earlier ones.
    var highlights: [(NSRange, NSColor)] = [] {
        didSet { needsDisplay = true }
    }
    /// The same highlights for the images, which draw their own.
    var objectHighlights: [(NSRange, NSColor)] = [] {
        didSet { if !objects.isEmpty { needsDisplay = true } }
    }
    /// Find matches in diagrams' labels, by diagram start and label index.
    var labelHighlights: [(start: Int, label: Int, color: NSColor)] = [] {
        didSet { if objects.contains(where: \.isDiagram) { needsDisplay = true } }
    }

    // MARK: Selection

    /// The selection's fixed end and its moving end (the cursor).
    private(set) var anchor = 0
    private(set) var head = 0
    /// The x Up and Down keep.
    var goalX: CGFloat?
    /// Text an input method is composing, shown at `markedAt` but not yet
    /// in the source.
    private(set) var marked: (text: String, selected: NSRange)?
    private(set) var markedAt = 0

    // MARK: Layout (see DocView+Layout)

    var laidOut: [LaidParagraph] = []
    var layoutHeight: CGFloat = 0
    /// Per source line: whether it shows (isn't collapsed).
    var lineShown: [Bool] = []
    /// Tables' grids by their source, for the next layout to reuse.
    var tableCache: [String: (grid: TableGrid, gaps: [Int: CGFloat], start: Int)] = [:]
    /// Paragraphs typeset by the last layouts, for the next to reuse.
    var typesetCache: [TypesetKey: Typeset] = [:]
    /// The last layout's paragraphs and their typesetting (nil where it
    /// can't be reused), for the same page.
    var previous: (context: TypesetContext, shown: [ShownParagraph], typesets: [Typeset?], gaps: [Int: CGFloat]) = (
        TypesetContext(width: 0, appearance: "", generation: 0), [], [], [:]
    )
    /// Where each table cell's text starts, from the text column's left
    /// edge, by the source position of the gap before it.
    var tableGaps: [Int: CGFloat] = [:]
    let caretView = NSTextInsertionIndicator()

    // MARK: Undo

    /// The text edited since the last restyle (in current coordinates),
    /// and each line's style hash as of then.
    private var pendingEdit: NSRange?
    private var styleHashes: [UInt64] = []
    /// The last edit (in current coordinates) and the change in length,
    /// and the lines restyled since (nil: all), for the layout to reuse
    /// what they didn't change.
    var lastEdit: (range: NSRange, delta: Int)?
    var restyled: IndexSet?
    private var editDelta = 0
    /// Typing that continues a step: the step, and where its text ends.
    private var typing: (step: EditStep, end: Int)?

    override init(frame: NSRect) {
        projection = Analysis(text: "").project(reflow: false, sourceMode: false, reveal: [])
        super.init(frame: frame)
        caretView.displayMode = .hidden
        addSubview(caretView)
        NotificationCenter.default.addObserver(
            self, selector: #selector(imageChanged(_:)), name: ImageLibrary.changed, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(imageDecoded(_:)), name: ImageLibrary.decoded, object: nil)
        refresh()
    }

    required init?(coder: NSCoder) { fatalError() }

    static func make() -> DocView {
        DocView(frame: NSRect(x: 0, y: 0, width: 1200, height: 800))
    }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override var isOpaque: Bool { true }

    override func becomeFirstResponder() -> Bool {
        updateCaret()
        needsDisplay = true
        return true
    }

    override func resignFirstResponder() -> Bool {
        caretView.displayMode = .hidden
        needsDisplay = true
        return true
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        updateCaret()
    }

    // MARK: - Contents

    /// Replaces the whole text, e.g. when loading a file. Not undoable.
    func setContents(_ new: String) {
        text = NSMutableString(string: new)
        styled.setAttributedString(NSAttributedString(string: new))
        // Nothing is styled or laid out yet.
        styleHashes = []
        pendingEdit = nil
        lastEdit = nil
        restyled = nil
        typesetCache = [:]
        tableCache = [:]
        previous.shown = []
        undoManager?.removeAllActions(withTarget: self)
        typing = nil
        anchor = 0
        head = 0
        marked = nil
        refresh()
        selectionChanged()
    }

    /// Changes the text to `new` with minimal edits, so the cursor and
    /// comment anchors stay on unchanged text. One undo step ("Undo Outside
    /// Change").
    func applyExternal(_ new: String, actionName: String = "Outside Change") {
        if string == new { return }
        let changes = textChanges(old: string, new: new).map {
            (NSRange(from: $0.start, to: $0.end), $0.text)
        }
        perform(changes, selecting: nil, actionName: actionName)
    }

    /// Re-analyzes the text and shows it again.
    func refresh() {
        let bytes = text.data(using: String.Encoding.utf8.rawValue) ?? Data()
        analysis = Analysis.fromUtf8(bytes: bytes)
        lines = LineInfo.decode(analysis.linesPacked())
        items = analysis.items()
        quotes = analysis.quotes()
        codeBlocks = analysis.codeBlocks()
        tableInfos = analysis.tables()
        imageBlocks = analysis.imageBlocks()
        diagramBlocks = analysis.diagramBlocks()
        changes = committed?.changes(text: analysis) ?? []
        updateObjects()
        revealed = sourceMode ? [] : analysis.revealAt(cursor: UInt32(min(head, text.length))).map(NSRange.init)
        restyle(only: linesToRestyle())
        reproject()
        onHighlight?()
    }

    func ensureFresh() {}

    /// The lines whose styling may have changed since the last restyle: the
    /// edited ones, and those whose spans changed (by their hashes, the
    /// lines after the edit compared with the lines they were).
    private func linesToRestyle() -> IndexSet? {
        let hashes: [UInt64] = analysis.lineStyleHashes().withUnsafeBytes { raw in
            (0..<(raw.count / 8)).map {
                UInt64(littleEndian: raw.loadUnaligned(fromByteOffset: $0 * 8, as: UInt64.self))
            }
        }
        defer {
            styleHashes = hashes
            lastEdit = pendingEdit.map { ($0, editDelta) }
            pendingEdit = nil
            editDelta = 0
        }
        guard !styleHashes.isEmpty, !lines.isEmpty else { return nil }
        let edit = pendingEdit ?? NSRange(location: 0, length: 0)
        let first = Int(analysis.lineIndex(pos: UInt32(min(edit.location, text.length))))
        let last = Int(analysis.lineIndex(pos: UInt32(min(NSMaxRange(edit), text.length))))
        let delta = hashes.count - styleHashes.count
        var out = IndexSet(integersIn: first...max(first, last))
        for i in hashes.indices where i < first || i > last {
            let old = i < first ? i : i - delta
            if old < 0 || old >= styleHashes.count || styleHashes[old] != hashes[i] { out.insert(i) }
        }
        return out
    }

    /// Styles the source as the analysis says: every line with `all`, else
    /// the lines in `only` (all when nil) whose styling changed.
    func restyle(all: Bool = false, only: IndexSet? = nil) {
        let opts = Styler.Options(sourceMode: sourceMode, revealed: revealed, generation: Theme.generation)
        var spans: [Span]
        if !all, let only {
            restyled = only
            guard let first = only.first, let last = only.last, last < lines.count else { return }
            // Only the spans over those lines.
            spans = Span.decode(
                analysis.spansPackedBetween(start: lines[first].start, end: lines[last].end))
        } else {
            restyled = nil
            spans = Span.decode(analysis.spansPacked())
        }
        styler.apply(
            lines: lines, spans: spans, dirty: all ? NSRange(location: 0, length: styled.length) : nil, to: styled,
            options: opts, only: all ? nil : only)
    }

    /// Projects the text as it shows now, and lays it out.
    func reproject() {
        projection = analysis.project(
            reflow: reflowsParagraphs && !sourceMode, sourceMode: sourceMode,
            reveal: revealed.map { TextRange(start: UInt32($0.location), end: UInt32(NSMaxRange($0))) })
        shown = ShownParagraph.decode(projection.paragraphsPacked())
        layOutTables()
        relayout()
    }

    /// Restyles everything, e.g. after the text size changed.
    func forceRestyle() {
        Theme.generation += 1
        styler.invalidate()
        refresh()
        restyle(all: true)
        reproject()
        updateGeometry(force: true)
        onLayout?()
    }

    /// The text of a range as shown: hidden syntax and images left out,
    /// newlines inside paragraphs read as spaces.
    func visibleText(_ r: NSRange) -> String {
        var out = ""
        for (i, p) in shown.enumerated() where Int(p.sourceEnd) >= r.location && Int(p.sourceStart) <= NSMaxRange(r) {
            defer {
                // The line break between shown paragraphs.
                let nl = Int(p.sourceEnd)
                if i + 1 < shown.count, nl >= r.location, nl < NSMaxRange(r) { out += "\n" }
            }
            for piece in p.pieces where piece.kind == .shown || piece.kind == .replaced {
                let s = NSIntersectionRange(
                    NSRange(from: piece.sourceStart, to: piece.sourceEnd), r)
                guard s.length > 0 || (piece.kind == .replaced && NSLocationInRange(Int(piece.sourceStart), r))
                else { continue }
                if piece.kind == .replaced {
                    out += String(Character(Unicode.Scalar(piece.character) ?? " "))
                } else {
                    out += text.substring(with: s)
                }
            }
        }
        return out
    }

    // MARK: - Objects

    /// The image blocks shown as images, and the diagrams: all of them,
    /// but in Show Markdown.
    private func updateObjects() {
        var all: [DocObject] = []
        if !sourceMode {
            all = imageBlocks.enumerated().compactMap { i, b in
                let li = Int(b.line)
                guard li < lines.count else { return nil }
                return DocObject(
                    kind: .image(b), index: i, source: ImageSource.resolve(b.url, from: imageFolder), line: li,
                    lastLine: li, start: Int(b.range.start), sourceEnd: Int(b.range.end), end: Int(lines[li].end))
            }
            let colors = diagramColors()
            all += diagramBlocks.enumerated().compactMap { i, d in
                let first = Int(d.firstLine)
                let last = Int(d.lastLine)
                guard last < lines.count else { return nil }
                return DocObject(
                    kind: .diagram(d), index: i, source: .diagram(d.source, colors), line: first, lastLine: last,
                    start: Int(d.range.start), sourceEnd: Int(d.range.end), end: Int(lines[last].end))
            }
            all.sort { $0.start < $1.start }
        }
        objects = all
        objectOnLine = [:]
        objectAtChar = [:]
        for (i, o) in objects.enumerated() {
            for li in o.line...o.lastLine { objectOnLine[li] = i }
            objectAtChar[o.start] = i
        }
    }

    /// The page's colors, as the diagrams are drawn in them.
    private func diagramColors() -> DiagramColors {
        var colors = DiagramColors(
            background: "#ffffff", node: "#f2f2f2", border: "#d0d0d0", text: "#000000", line: "#808080",
            fontSize: UInt32(Theme.baseBodySize))
        effectiveAppearance.performAsCurrentDrawingAppearance {
            let page = (backgroundColor.usingColorSpace(.sRGB) ?? .white)
            func hex(_ c: NSColor) -> String {
                let c = c.usingColorSpace(.sRGB) ?? c
                let a = c.alphaComponent
                let mix = { (f: CGFloat, b: CGFloat) in Int(((f * a + b * (1 - a)) * 255).rounded()) }
                return String(
                    format: "#%02X%02X%02X", mix(c.redComponent, page.redComponent),
                    mix(c.greenComponent, page.greenComponent), mix(c.blueComponent, page.blueComponent))
            }
            colors = DiagramColors(
                background: hex(page), node: hex(Theme.codeBackground), border: hex(Theme.border),
                text: hex(Theme.text), line: hex(Theme.dim), fontSize: UInt32(Theme.baseBodySize))
        }
        return colors
    }

    /// The object on the line of `p`, if `p` is on it.
    func object(at p: Int) -> DocObject? {
        guard !objects.isEmpty, !lines.isEmpty else { return nil }
        let li = Int(analysis.lineIndex(pos: UInt32(max(0, p))))
        guard let i = objectOnLine[li], objects[i].contains(p) else { return nil }
        return objects[i]
    }

    /// The image the selection is, if it is one.
    var selectedObject: DocObject? {
        let sel = sourceSelection
        guard sel.length > 0, let o = object(at: sel.location), sel.location == o.start,
            NSMaxRange(sel) >= o.sourceEnd, NSMaxRange(sel) <= o.end
        else { return nil }
        return o
    }

    /// The image blocks shown as their alt text, as indices for find.
    var altTextImages: [UInt32] {
        if sourceMode { return imageBlocks.indices.map { UInt32($0) } }
        return objects.compactMap { o in
            guard !o.isDiagram, case .failed = ImageLibrary.shared.state(of: o.source) else { return nil }
            return UInt32(o.index)
        }
    }

    /// The diagrams shown as their source, as indices for find.
    var sourceDiagrams: [UInt32] {
        if sourceMode { return diagramBlocks.indices.map { UInt32($0) } }
        return objects.compactMap { o in
            guard o.isDiagram, case .unrenderable = ImageLibrary.shared.state(of: o.source) else { return nil }
            return UInt32(o.index)
        }
    }

    /// Find matches in the labels diagrams draw.
    func labelMatches(_ needle: String, matchCase: Bool) -> [(start: Int, label: Int, range: NSRange)] {
        guard !needle.isEmpty, !sourceMode else { return [] }
        var out: [(start: Int, label: Int, range: NSRange)] = []
        for o in objects where o.isDiagram {
            guard case .loaded(let d) = ImageLibrary.shared.state(of: o.source) else { continue }
            for (i, l) in d.labels.enumerated() {
                let t = l.text as NSString
                var from = 0
                while from < t.length {
                    let r = t.range(
                        of: needle, options: matchCase ? [] : [.caseInsensitive],
                        range: NSRange(location: from, length: t.length - from))
                    if r.location == NSNotFound { break }
                    out.append((o.start, i, r))
                    from = NSMaxRange(r)
                }
            }
        }
        return out
    }

    /// How image `o` looks now, its size from the text column and the zoom.
    func look(of o: DocObject) -> ObjectLook {
        let l = lines[o.line]
        let room = max(1, geometry.docWidth - indent(quotes: l.quotes, items: l.items))
        switch ImageLibrary.shared.state(of: o.source) {
        case .loaded(let image):
            return .image(image, ObjectLook.size(of: image, zoom: Theme.bodySize / Theme.baseBodySize, room: room))
        case .loading:
            return .placeholder(NSSize(width: room.rounded(), height: (160 * Theme.scale).rounded()))
        case .failed(let reason):
            guard case .image(let info) = o.kind else { return .placeholder(NSSize(width: room, height: 1)) }
            let alt = info.alt.map { r -> (NSRange, String) in (NSRange(r), text.substring(with: NSRange(r))) }
            return .broken(BrokenImage(alt: alt, fileName: o.source.fileName, reason: reason, room: room))
        case .unrenderable(let message, let fault):
            guard case .diagram(let d) = o.kind else { return .placeholder(NSSize(width: room, height: 1)) }
            let s = text
            let ls = d.lineStarts.map { Int($0) }.map { start -> (source: NSRange, text: String) in
                let r = s.lineRange(for: NSRange(location: min(start, max(0, s.length - 1)), length: 0))
                var end = NSMaxRange(r)
                while end > start && (s.character(at: end - 1) == 10 || s.character(at: end - 1) == 13) { end -= 1 }
                let range = NSRange(location: start, length: max(0, end - start))
                return (range, s.substring(with: range))
            }
            return .diagramError(DiagramError(message: message, lines: ls, fault: fault, room: room.rounded()))
        }
    }

    /// Images whose size arrived, or that failed, not yet laid out again.
    private var imagesToLayOut = Set<ImageSource>()
    private(set) var imageLayoutPending = false

    /// An image loaded, or failed to: lay out again, keeping the text being
    /// read where it is. Images arriving close together go together.
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

    func layOutArrivedImages() {
        guard imageLayoutPending else { return }
        imageLayoutPending = false
        let sources = imagesToLayOut
        imagesToLayOut = []
        let changed = objects.filter { sources.contains($0.source) }
        guard !changed.isEmpty else { return }
        keepingReadingPlace(changing: Set(changed.map(\.line))) { relayout() }
        window?.invalidateCursorRects(for: self)
        onLayout?()
        onImagesChanged?()
    }

    @objc private func imageDecoded(_ note: Notification) {
        guard let source = note.userInfo?[ImageLibrary.sourceKey] as? ImageSource else { return }
        for o in objects where o.source == source {
            if let r = objectRect(o, look: look(of: o)) { setNeedsDisplay(r) }
        }
    }

    /// Runs `change`, which changes the heights of `changing` lines, keeping
    /// the first line shown at the top of the window where it is.
    private func keepingReadingPlace(changing: Set<Int>, _ change: () -> Void) {
        guard let clip = enclosingScrollView?.contentView, !lines.isEmpty else { return change() }
        let top = visibleRect.minY
        let anchorLine = lines(in: visibleRect).first { li in
            !changing.contains(li) && !isCollapsed(line: li) && textTop(line: li) >= top
        }
        let before = anchorLine.map { textTop(line: $0) }
        change()
        sizeToFit()
        enclosingScrollView?.documentView?.layoutSubtreeIfNeeded()
        guard let anchorLine, let before else { return }
        let moved = textTop(line: anchorLine) - before
        if moved != 0 {
            clip.scroll(to: NSPoint(x: clip.bounds.minX, y: max(0, clip.bounds.minY + moved)))
            enclosingScrollView?.reflectScrolledClipView(clip)
        }
    }

    /// Runs `done` once every image has loaded or failed to, laid out.
    func whenImagesSettled(_ done: @escaping () -> Void) {
        ImageLibrary.shared.whenSettled(objects.map(\.source)) { [weak self] in
            self?.layOutArrivedImages()
            done()
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        // Diagrams are drawn in the page's colors; text colors resolve anew.
        updateObjects()
        relayout()
        onHighlight?()
    }

    // MARK: - Selection

    /// The selection in the source.
    var sourceSelection: NSRange {
        NSRange(location: min(anchor, head), length: abs(head - anchor))
    }

    /// The selection, as a text view and an input method see it: in the
    /// text being composed while there is some.
    func selectedRange() -> NSRange {
        if let m = marked { return NSRange(location: markedAt + m.selected.location, length: m.selected.length) }
        return sourceSelection
    }

    var selectedRanges: [NSValue] {
        get { [NSValue(range: selectedRange())] }
        set { if let r = newValue.first?.rangeValue { setSelectedRange(r) } }
    }

    /// The selection's moving end: where typing goes.
    var cursor: Int { sourceSelection.location + sourceSelection.length }

    /// The selection, or nil when it is empty.
    var selection: NSRange? {
        let r = sourceSelection
        return r.length > 0 ? r : nil
    }

    func setSelectedRange(_ r: NSRange) {
        select(anchor: r.location, head: NSMaxRange(r))
    }

    /// Selects from `anchor` to `head`, which the arrows move.
    func select(anchor a: Int, head h: Int, keepGoal: Bool = false) {
        let len = text.length
        var (a, h) = (min(max(0, a), len), min(max(0, h), len))
        if marked == nil {
            if a == h, let o = object(at: a), a > o.start, a < o.end {
                (a, h) = (o.start, o.end)
            } else {
                for (i, p) in [a, h].enumerated() {
                    guard let o = object(at: p), p > o.start, p < o.end else { continue }
                    // The end inside an image goes to its side away from
                    // the other end.
                    let other = i == 0 ? h : a
                    let q = other <= o.start ? o.end : o.start
                    if i == 0 { a = q } else { h = q }
                }
            }
        }
        let changed = a != anchor || h != head
        anchor = a
        head = h
        if !keepGoal { goalX = nil }
        if changed { typing = nil }
        selectionChanged()
    }

    private func selectionChanged() {
        let want = sourceMode ? [] : analysis.revealAt(cursor: UInt32(min(head, text.length))).map(NSRange.init)
        if want != revealed {
            // The lines revealed and no longer revealed.
            var lines = IndexSet()
            for r in want + revealed {
                let a = Int(analysis.lineIndex(pos: UInt32(r.location)))
                let b = Int(analysis.lineIndex(pos: UInt32(max(r.location, NSMaxRange(r) - 1))))
                lines.insert(integersIn: a...max(a, b))
            }
            revealed = want
            restyle(only: lines)
            reproject()
            onLayout?()
        }
        updateCaret()
        needsDisplay = true
        if marked == nil { onSelectionChange?() }
    }

    /// Puts the cursor at `p`, or past the image `p` is on.
    func placeCursor(at p: Int) {
        if let o = object(at: p), p > o.start, p < o.end {
            return setSelectedRange(NSRange(location: o.end, length: 0))
        }
        setSelectedRange(NSRange(location: p, length: 0))
    }

    // MARK: - Editing

    /// Applies an editing plan as one undoable step.
    func apply(_ plan: EditPlan) {
        let sel = plan.selection.map(NSRange.init) ?? NSRange(location: Int(plan.cursor), length: 0)
        let changes = plan.changes.map { (NSRange(from: $0.start, to: $0.end), $0.text) }
        if changes.isEmpty {
            setSelectedRange(sel)
            return
        }
        perform(changes, selecting: sel)
        scrollRangeToVisible(selectedRange())
    }

    /// Runs an editing command against the cursor and selection.
    func run(_ command: (Analysis, Int, NSRange) -> EditPlan?) {
        if let plan = command(analysis, cursor, sourceSelection) {
            apply(plan)
        }
    }

    /// Makes `changes` (ranges in the current text, disjoint) as one undo
    /// step, then selects `selecting`; nil keeps the selection on its text.
    func perform(_ changes: [(NSRange, String)], selecting: NSRange?, actionName: String? = nil) {
        let before = sourceSelection
        let step = EditStep(selection: before)
        step.inverse = replace(changes)
        typing = nil
        let after = selecting ?? mapped(before, through: changes)
        step.after = after
        register(step, actionName: actionName)
        refresh()
        setSelectedRange(after)
        onChange?()
    }

    /// Replaces `changes` in the text, last first, telling anchors of each;
    /// returns the changes that undo them, as ranges in the new text.
    @discardableResult
    private func replace(_ changes: [(NSRange, String)]) -> [(NSRange, String)] {
        var inverse: [(NSRange, String)] = []
        let sorted = changes.sorted { $0.0.location > $1.0.location }
        // Each change's place in the new text: shifted by the changes before
        // it, which are applied after it.
        var shift = 0
        var placed: [(NSRange, String)] = []
        for (range, new) in sorted.reversed() {
            let n = (new as NSString).length
            placed.append((NSRange(location: range.location + shift, length: n), text.substring(with: range)))
            shift += n - range.length
        }
        for (range, new) in sorted {
            text.replaceCharacters(in: range, with: new)
            styled.replaceCharacters(in: range, with: new)
            let n = (new as NSString).length
            let edited = NSRange(location: range.location, length: n)
            editDelta += n - range.length
            if let d = pendingEdit {
                let delta = n - range.length
                let end = NSMaxRange(d) >= NSMaxRange(range) ? NSMaxRange(d) + delta : NSMaxRange(edited)
                let a = min(d.location, edited.location)
                pendingEdit = NSRange(location: a, length: max(end, NSMaxRange(edited)) - a)
            } else {
                pendingEdit = edited
            }
            onEdit?(edited, n - range.length)
        }
        inverse = placed.reversed()
        return inverse
    }

    /// Where `r` is after `changes`: an end in replaced text goes to the
    /// end of what replaced it.
    private func mapped(_ r: NSRange, through changes: [(NSRange, String)]) -> NSRange {
        func map(_ p: Int) -> Int {
            var q = p
            for (range, new) in changes {
                let n = (new as NSString).length
                if p >= NSMaxRange(range) && !(range.length == 0 && p == range.location) {
                    q += n - range.length
                } else if p > range.location {
                    q += range.location + n - p
                }
            }
            return q
        }
        let a = map(r.location)
        return NSRange(location: a, length: max(0, map(NSMaxRange(r)) - a))
    }

    private func register(_ step: EditStep, actionName: String?) {
        lastStep = step
        guard let um = undoManager else { return }
        um.beginUndoGrouping()
        um.registerUndo(withTarget: self) { $0.revert(step) }
        if let actionName { um.setActionName(actionName) }
        um.endUndoGrouping()
    }

    /// Undoes (or redoes) a step, registering its reverse.
    private func revert(_ step: EditStep) {
        let back = EditStep(selection: step.after)
        back.inverse = replace(step.inverse)
        back.after = step.selection
        typing = nil
        undoManager?.registerUndo(withTarget: self) { $0.revert(back) }
        refresh()
        setSelectedRange(step.selection)
        scrollRangeToVisible(selectedRange())
        onChange?()
    }

    /// Plain typing: one step for a run of characters typed in a row.
    private func type(_ s: String) {
        let p = cursor
        let plan = analysis.insert(pos: UInt32(p), text: s)
        let n = (s as NSString).length
        let plain =
            plan.changes.count == 1 && plan.selection == nil && Int(plan.changes[0].start) == p
            && Int(plan.changes[0].end) == p && plan.changes[0].text == s && Int(plan.cursor) == p + n
        if plain, let t = typing, t.end == p, let first = t.step.inverse.first {
            replace([(NSRange(location: p, length: 0), s)])
            t.step.inverse = [(NSRange(location: first.0.location, length: first.0.length + n), first.1)]
            t.step.after = NSRange(location: p + n, length: 0)
            refresh()
            setSelectedRange(NSRange(location: p + n, length: 0))
            typing = (t.step, p + n)
            scrollRangeToVisible(NSRange(location: p + n, length: 0))
            onChange?()
            return
        }
        apply(plan)
        if plain, let step = lastStep { typing = (step, p + n) }
    }

    /// The step registered last, for typing to continue.
    private var lastStep: EditStep?

    private func deleteSelectionThroughCore() {
        if let sel = selection {
            run { a, _, _ in a.deleteRange(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel))) }
        }
    }

    // MARK: - Keys

    override func keyDown(with event: NSEvent) {
        interpretKeyEvents([event])
    }

    override func doCommand(by selector: Selector) {
        if responds(to: selector) {
            perform(selector, with: nil)
        }
    }

    override func cancelOperation(_ sender: Any?) {
        onEscape?()
    }

    @objc override func insertNewline(_ sender: Any?) {
        if sourceMode { return insertRaw("\n") }
        newline(soft: NSApp.currentEvent?.modifierFlags.contains(.shift) ?? false)
    }

    @objc override func insertLineBreak(_ sender: Any?) {
        if sourceMode { return insertRaw("\n") }
        newline(soft: true)
    }

    @objc override func insertNewlineIgnoringFieldEditor(_ sender: Any?) {
        if sourceMode { return insertRaw("\n") }
        newline(soft: false)
    }

    private func newline(soft: Bool) {
        grouped {
            deleteSelectionThroughCore()
            run { a, c, _ in a.newline(pos: UInt32(c), soft: soft) }
        }
    }

    @objc override func insertTab(_ sender: Any?) {
        if sourceMode { return insertRaw("\t") }
        indentLines(outdent: false)
    }

    @objc override func insertBacktab(_ sender: Any?) {
        if sourceMode { return }
        indentLines(outdent: true)
    }

    @objc override func deleteBackward(_ sender: Any?) {
        if let sel = selection {
            return sourceMode
                ? perform([(sel, "")], selecting: NSRange(location: sel.location, length: 0))
                : deleteSelectionThroughCore()
        }
        let p = cursor
        guard p > 0 else { return }
        if sourceMode {
            let r = text.rangeOfComposedCharacterSequence(at: p - 1)
            return perform([(r, "")], selecting: NSRange(location: r.location, length: 0))
        }
        apply(analysis.backspace(pos: UInt32(p)))
    }

    @objc override func deleteForward(_ sender: Any?) {
        if let sel = selection {
            return sourceMode
                ? perform([(sel, "")], selecting: NSRange(location: sel.location, length: 0))
                : deleteSelectionThroughCore()
        }
        let p = cursor
        guard p < text.length else { return }
        if sourceMode {
            let r = text.rangeOfComposedCharacterSequence(at: p)
            return perform([(r, "")], selecting: NSRange(location: p, length: 0))
        }
        run { a, c, _ in a.deleteForward(pos: UInt32(c)) }
    }

    @objc override func deleteWordBackward(_ sender: Any?) {
        if selection != nil { return deleteBackward(sender) }
        deleteTo(wordBoundary(from: cursor, forward: false))
    }

    @objc override func deleteWordForward(_ sender: Any?) {
        if selection != nil { return deleteForward(sender) }
        deleteTo(wordBoundary(from: cursor, forward: true))
    }

    @objc override func deleteToBeginningOfLine(_ sender: Any?) {
        if selection != nil { return deleteBackward(sender) }
        deleteTo(lineEdge(from: cursor, end: false))
    }

    @objc override func deleteToEndOfLine(_ sender: Any?) {
        if selection != nil { return deleteForward(sender) }
        deleteTo(lineEdge(from: cursor, end: true))
    }

    @objc override func deleteToEndOfParagraph(_ sender: Any?) {
        deleteToEndOfLine(sender)
    }

    /// Deletes from the cursor to `p` through the editing rules.
    private func deleteTo(_ p: Int) {
        let c = cursor
        guard p != c else { return }
        let r = NSRange(location: min(p, c), length: abs(p - c))
        if sourceMode { return perform([(r, "")], selecting: NSRange(location: r.location, length: 0)) }
        run { a, _, _ in a.deleteRange(start: UInt32(r.location), end: UInt32(NSMaxRange(r))) }
    }

    /// Ctrl+T: swaps the characters around the cursor.
    @objc override func transpose(_ sender: Any?) {
        let p = cursor
        guard selection == nil, p > 0, p < text.length else { return }
        let a = text.rangeOfComposedCharacterSequence(at: p - 1)
        let b = text.rangeOfComposedCharacterSequence(at: p)
        let swapped = text.substring(with: b) + text.substring(with: a)
        let r = NSRange(location: a.location, length: NSMaxRange(b) - a.location)
        run { an, _, _ in an.replaceRange(start: UInt32(r.location), end: UInt32(NSMaxRange(r)), text: swapped) }
    }

    /// The Spelling panel's Change: the correction replaces the selection.
    @objc func changeSpelling(_ sender: Any?) {
        guard let word = (sender as? NSMatrix)?.selectedCell()?.stringValue ?? (sender as? NSControl)?.stringValue,
            let sel = selection
        else { return }
        run { a, _, _ in a.replaceRange(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel)), text: word) }
    }

    /// Several commands as one undo step.
    private func grouped(_ body: () -> Void) {
        undoManager?.beginUndoGrouping()
        body()
        undoManager?.endUndoGrouping()
    }

    /// Show Markdown: text goes in as typed.
    private func insertRaw(_ s: String) {
        let sel = sourceSelection
        perform([(sel, s)], selecting: NSRange(location: sel.location + (s as NSString).length, length: 0))
    }

    @objc override func selectAll(_ sender: Any?) {
        select(anchor: 0, head: text.length)
    }

    // MARK: - NSTextInputClient

    /// The input method's view of the text: the source, with the text being
    /// composed in it at `markedAt`.
    private var clientText: NSString {
        guard let m = marked else { return text }
        let s = NSMutableString(string: text)
        s.insert(m.text, at: markedAt)
        return s
    }

    /// A position in the input method's text, in the source.
    private func sourcePosition(client p: Int) -> Int {
        guard let m = marked else { return p }
        let n = (m.text as NSString).length
        return p <= markedAt ? p : (p <= markedAt + n ? markedAt : p - n)
    }

    func insertText(_ string: Any, replacementRange: NSRange) {
        let s = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        if marked != nil {
            marked = nil
            if !s.isEmpty {
                // As composed, where it was composed.
                let n = (s as NSString).length
                perform(
                    [(NSRange(location: markedAt, length: 0), s)], selecting: NSRange(location: markedAt + n, length: 0)
                )
            } else {
                relayout()
                updateCaret()
            }
            return
        }
        if replacementRange.location != NSNotFound && replacementRange != selectedRange() {
            setSelectedRange(replacementRange)
        }
        if s.isEmpty { return }
        if sourceMode { return insertRaw(s) }
        if s == "\n" || s == "\r" { return insertNewline(nil) }
        if s == "\t" { return insertTab(nil) }
        if let sel = selection {
            run { a, _, _ in a.replaceRange(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel)), text: s) }
            return
        }
        type(s)
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        let s = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        if marked == nil {
            if replacementRange.location != NSNotFound {
                // Reconverting committed text: it is composed again.
                let r = NSRange(
                    location: sourcePosition(client: replacementRange.location),
                    length: sourcePosition(client: NSMaxRange(replacementRange))
                        - sourcePosition(client: replacementRange.location))
                if r.length > 0 { perform([(r, "")], selecting: NSRange(location: r.location, length: 0)) }
            } else if let sel = selection {
                // Composing over a selection replaces it in place, keeping
                // the formatting around it, as in any text view.
                perform([(sel, "")], selecting: NSRange(location: sel.location, length: 0))
                markedAt = sel.location
                marked = s.isEmpty ? nil : (s, selectedRange)
                relayout()
                updateCaret()
                return
            }
            markedAt = sourceMode ? cursor : Int(analysis.insertionPoint(pos: UInt32(cursor)))
        }
        marked = s.isEmpty ? nil : (s, selectedRange)
        relayout()
        updateCaret()
    }

    func unmarkText() {
        guard let m = marked else { return }
        insertText(m.text, replacementRange: NSRange(location: NSNotFound, length: 0))
    }

    func markedRange() -> NSRange {
        guard let m = marked else { return NSRange(location: NSNotFound, length: 0) }
        return NSRange(location: markedAt, length: (m.text as NSString).length)
    }

    func hasMarkedText() -> Bool { marked != nil }

    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? {
        let t = clientText
        let lo = max(0, min(range.location, t.length))
        let hi = max(lo, min(NSMaxRange(range), t.length))
        actualRange?.pointee = NSRange(location: lo, length: hi - lo)
        return NSAttributedString(
            string: t.substring(with: NSRange(location: lo, length: hi - lo)),
            attributes: [.font: Theme.font(size: Theme.bodySize)])
    }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] {
        [.underlineStyle, .markedClauseSegment]
    }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        actualRange?.pointee = range
        let r = clientRects(range).first ?? caretRect()
        guard let window else { return r }
        return window.convertToScreen(convert(r, to: nil))
    }

    func characterIndex(for point: NSPoint) -> Int {
        guard let window else { return NSNotFound }
        let p = convert(window.convertPoint(fromScreen: point), from: nil)
        let s = position(at: p)
        guard let m = marked, s > markedAt else { return s }
        return s + (m.text as NSString).length
    }

    // MARK: - Clipboard

    @objc func copy(_ sender: Any?) {
        guard let sel = selection else { return }
        let t =
            sourceMode
            ? text.substring(with: sel)
            : analysis.copySource(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel)))
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(t, forType: .string)
    }

    @objc func cut(_ sender: Any?) {
        guard selection != nil else { return }
        copy(sender)
        deleteBackward(sender)
    }

    @objc func paste(_ sender: Any?) {
        guard let t = NSPasteboard.general.string(forType: .string) else { return }
        pasteText(t)
    }

    @objc func pasteAsPlainText(_ sender: Any?) { paste(sender) }
    @objc func pasteAsRichText(_ sender: Any?) { paste(sender) }

    func pasteText(_ t: String) {
        let t = t.replacingOccurrences(of: "\r\n", with: "\n").replacingOccurrences(of: "\r", with: "\n")
        if sourceMode { return insertRaw(t) }
        run { a, c, sel in
            sel.length > 0
                ? a.replaceRange(start: UInt32(sel.location), end: UInt32(NSMaxRange(sel)), text: t)
                : a.insert(pos: UInt32(c), text: t)
        }
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
        if let item = analysis.taskOnLineOf(pos: UInt32(cursor)) {
            apply(analysis.toggleTask(item: item, cursor: UInt32(cursor)))
        }
    }

    @objc func marginOpenLink(_ sender: Any?) {
        if let url = analysis.linkAtCursor(pos: UInt32(cursor)) {
            onOpenLink?(url)
        }
    }

    @objc func marginLink(_ sender: Any?) {
        guard formattingAllowed, let window else { return }
        let current = analysis.linkAtCursor(pos: UInt32(cursor))
        let sel = sourceSelection
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
    /// Pages and Preview, then corrections for a misspelled word under the
    /// pointer.
    override func menu(for event: NSEvent) -> NSMenu? {
        let menu = NSMenu()
        menu.addItem(
            NSMenuItem(
                title: "Comment on Selection", action: #selector(DocumentWindow.marginCommentOnSelection(_:)),
                keyEquivalent: ""))
        menu.addItem(.separator())
        if let (word, range) = misspelledWord(at: convert(event.locationInWindow, from: nil)) {
            let guesses =
                NSSpellChecker.shared.guesses(
                    forWordRange: NSRange(location: 0, length: (word as NSString).length), in: word, language: nil,
                    inSpellDocumentWithTag: spellTag) ?? []
            for g in guesses.prefix(6) {
                let item = NSMenuItem(title: g, action: #selector(correctSpelling(_:)), keyEquivalent: "")
                item.representedObject = [range.location, range.length] as [Int]
                item.target = self
                menu.addItem(item)
            }
            if guesses.isEmpty { menu.addItem(NSMenuItem(title: "No Guesses Found", action: nil, keyEquivalent: "")) }
            let ignore = NSMenuItem(title: "Ignore Spelling", action: #selector(ignoreSpelling(_:)), keyEquivalent: "")
            ignore.representedObject = word
            ignore.target = self
            let learn = NSMenuItem(title: "Learn Spelling", action: #selector(learnSpelling(_:)), keyEquivalent: "")
            learn.representedObject = word
            learn.target = self
            menu.addItem(.separator())
            menu.addItem(ignore)
            menu.addItem(learn)
            menu.addItem(.separator())
        }
        menu.addItem(NSMenuItem(title: "Cut", action: #selector(cut(_:)), keyEquivalent: ""))
        menu.addItem(NSMenuItem(title: "Copy", action: #selector(copy(_:)), keyEquivalent: ""))
        menu.addItem(NSMenuItem(title: "Paste", action: #selector(paste(_:)), keyEquivalent: ""))
        return menu
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(toggleContinuousSpellChecking(_:)) {
            item.state = isContinuousSpellCheckingEnabled ? .on : .off
        }
        return validate(item.action)
    }

    // MARK: - Spelling

    @objc func toggleContinuousSpellChecking(_ sender: Any?) {
        isContinuousSpellCheckingEnabled.toggle()
    }

    /// A correction from the menu: it replaces the word through the
    /// editing rules, keeping the formatting around it.
    @objc func correctSpelling(_ sender: NSMenuItem) {
        guard let r = sender.representedObject as? [Int], r.count == 2 else { return }
        run { a, _, _ in a.replaceRange(start: UInt32(r[0]), end: UInt32(r[0] + r[1]), text: sender.title) }
    }

    @objc func ignoreSpelling(_ sender: NSMenuItem) {
        guard let word = sender.representedObject as? String else { return }
        NSSpellChecker.shared.ignoreWord(word, inSpellDocumentWithTag: spellTag)
        misspellings = [:]
        needsDisplay = true
    }

    @objc func learnSpelling(_ sender: NSMenuItem) {
        guard let word = sender.representedObject as? String else { return }
        NSSpellChecker.shared.learnWord(word)
        misspellings = [:]
        needsDisplay = true
    }

    func validateUserInterfaceItem(_ item: any NSValidatedUserInterfaceItem) -> Bool {
        validate(item.action)
    }

    private func validate(_ action: Selector?) -> Bool {
        switch action {
        case #selector(marginBold(_:)), #selector(marginItalic(_:)), #selector(marginStrikethrough(_:)),
            #selector(marginInlineCode(_:)), #selector(marginNormalText(_:)), #selector(marginHeading(_:)),
            #selector(marginBulletedList(_:)), #selector(marginNumberedList(_:)), #selector(marginChecklist(_:)),
            #selector(marginQuote(_:)), #selector(marginCodeBlock(_:)), #selector(marginIndent(_:)),
            #selector(marginOutdent(_:)), #selector(marginToggleTask(_:)), #selector(marginLink(_:)):
            return formattingAllowed
        case #selector(copy(_:)), #selector(cut(_:)):
            return selection != nil
        case #selector(paste(_:)), #selector(pasteAsPlainText(_:)), #selector(pasteAsRichText(_:)):
            return NSPasteboard.general.string(forType: .string) != nil
        default:
            return true
        }
    }

    /// Cmd+Shift+7, 8 and 9 by physical key, as Google Docs does.
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

/// One undoable edit: the changes that undo it, and the selections around
/// it.
final class EditStep {
    var inverse: [(NSRange, String)] = []
    let selection: NSRange
    var after: NSRange

    init(selection: NSRange) {
        self.selection = selection
        after = selection
    }
}
