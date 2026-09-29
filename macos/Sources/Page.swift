import AppKit

/// The scrolling page: the text view (the left margin and the text column)
/// beside the gutter that holds the comment cards. Both scroll together;
/// the page is as tall as the taller of the two, and at least the window.
final class PageView: NSView {
    let textView: DocTextView
    let gutter = GutterView()
    /// How far down the gutter's cards reach, in page coordinates.
    var gutterExtent: CGFloat = 0 {
        didSet { if abs(oldValue - gutterExtent) > 0.5 { needsLayout = true } }
    }
    private var tiling = false

    init(textView: DocTextView) {
        self.textView = textView
        super.init(frame: NSRect(x: 0, y: 0, width: 1200, height: 800))
        textView.autoresizingMask = []
        textView.postsFrameChangedNotifications = true
        addSubview(textView)
        addSubview(gutter)
        NotificationCenter.default.addObserver(self, selector: #selector(contentChanged), name: NSView.frameDidChangeNotification, object: textView)
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
        NotificationCenter.default.addObserver(self, selector: #selector(contentChanged), name: NSView.frameDidChangeNotification, object: textView)
        if let clip = superview as? NSClipView {
            clip.postsFrameChangedNotifications = true
            NotificationCenter.default.addObserver(self, selector: #selector(contentChanged), name: NSView.frameDidChangeNotification, object: clip)
        }
        needsLayout = true
    }

    @objc private func contentChanged() {
        if !tiling { needsLayout = true }
    }

    override func layout() {
        super.layout()
        tile()
    }

    /// Lays out the text and the gutter for the window's width.
    private func tile() {
        guard !tiling else { return }
        tiling = true
        defer { tiling = false }
        let visible = (superview as? NSClipView)?.bounds.size ?? bounds.size
        let g = PageGeometry(width: visible.width, scale: Theme.scale)
        let textWidth = g.gutterX - PageGeometry.gutterGap / 2
        textView.minSize = NSSize(width: 0, height: visible.height)
        if textView.frame.width != textWidth {
            textView.setFrameSize(NSSize(width: textWidth, height: textView.frame.height))
        }
        textView.setPage(g)
        textView.sizeToFit()
        let height = max(textView.frame.height, gutterExtent + 40, visible.height)
        textView.setFrameOrigin(.zero)
        gutter.frame = NSRect(x: textWidth, y: 0, width: max(0, visible.width - textWidth), height: height)
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
