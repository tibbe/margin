import AppKit

/// The scrolling page: the text view (the left margin and the text column)
/// beside the gutter that holds the comment cards. Both scroll together;
/// the page is as tall as the taller of the two, and at least the window.
///
/// One layout pass lays out the text, then places the cards beside it.
/// AppKit lays out before it draws, so a card is never drawn where its
/// text was.
final class PageView: NSView {
    let textView: DocTextView
    let gutter = GutterView()
    /// Places the cards beside the laid-out text; returns how far down they
    /// reach, in page coordinates.
    var placeCards: (() -> CGFloat)?
    /// Whether the gutter shows cards; without, the text is centered alone.
    var hasCards = false {
        didSet { if oldValue != hasCards { retile() } }
    }
    /// The text is to be laid out for the window again in the next pass.
    private var needsTile = true
    /// How far down the cards reach, in page coordinates.
    private var gutterExtent: CGFloat = 0
    private var laying = false

    init(textView: DocTextView) {
        self.textView = textView
        super.init(frame: NSRect(x: 0, y: 0, width: 1200, height: 800))
        textView.autoresizingMask = []
        textView.postsFrameChangedNotifications = true
        addSubview(textView)
        addSubview(gutter)
        NotificationCenter.default.addObserver(
            self, selector: #selector(contentChanged), name: NSView.frameDidChangeNotification, object: textView)
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }
    override var isOpaque: Bool { true }

    /// One page: the gutter is the page's margin, on the text's background.
    override func draw(_ dirtyRect: NSRect) {
        NSColor.textBackgroundColor.setFill()
        dirtyRect.fill()
    }

    override func viewDidMoveToSuperview() {
        super.viewDidMoveToSuperview()
        NotificationCenter.default.removeObserver(self, name: NSView.frameDidChangeNotification, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(contentChanged), name: NSView.frameDidChangeNotification, object: textView)
        if let clip = superview as? NSClipView {
            clip.postsFrameChangedNotifications = true
            NotificationCenter.default.addObserver(
                self, selector: #selector(contentChanged), name: NSView.frameDidChangeNotification, object: clip)
        }
        retile()
    }

    @objc private func contentChanged() {
        if !laying { retile() }
    }

    /// Lays out the text for the window again (its width or text size
    /// changed), and the cards with it, in the next layout pass.
    func retile() {
        needsTile = true
        needsLayout = true
    }

    /// Places the cards again (they changed, or the text moved under
    /// them), in the next layout pass.
    func cardsChanged() {
        if !laying { needsLayout = true }
    }

    override func layout() {
        super.layout()
        guard !laying else { return }
        laying = true
        defer { laying = false }
        if needsTile {
            needsTile = false
            tile()
        }
        gutterExtent = placeCards?() ?? 0
        fitHeight()
    }

    private var visibleSize: NSSize { (superview as? NSClipView)?.bounds.size ?? bounds.size }

    /// Lays out the text and the gutter for the window's width.
    private func tile() {
        let visible = visibleSize
        let g = PageGeometry(width: visible.width, scale: Theme.scale, hasCards: hasCards)
        let textWidth = g.gutterX - PageGeometry.gutterGap / 2
        textView.minSize = NSSize(width: 0, height: visible.height)
        if textView.frame.width != textWidth {
            textView.setFrameSize(NSSize(width: textWidth, height: textView.frame.height))
        }
        textView.setPage(g)
        textView.sizeToFit()
        textView.setFrameOrigin(.zero)
        gutter.frame = NSRect(x: textWidth, y: 0, width: max(0, visible.width - textWidth), height: gutter.frame.height)
    }

    /// As tall as the taller of the text and the cards, and at least the
    /// window.
    private func fitHeight() {
        let visible = visibleSize
        let height = max(textView.frame.height, gutterExtent + 40, visible.height)
        if gutter.frame.height != height { gutter.frame.size.height = height }
        if frame.size != NSSize(width: visible.width, height: height) {
            setFrameSize(NSSize(width: visible.width, height: height))
        }
    }
}

/// The comment gutter. An ordinary view: the arrow cursor, and clicks that
/// reach the cards and never the text.
final class GutterView: NSView {
    /// A click on empty gutter space.
    var onEmptyClick: (() -> Void)?

    override var isFlipped: Bool { true }

    override func mouseDown(with event: NSEvent) {
        // Leaves the focused card, but never moves the text cursor.
        onEmptyClick?()
    }
}
