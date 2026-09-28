import AppKit

/// A short message at the bottom of the window that goes away by itself,
/// with an Undo button when there is something to undo.
final class Banner: NSVisualEffectView {
    private let label = NSTextField(labelWithString: "")
    private let undo = NSButton(title: "Undo", target: nil, action: nil)
    private var undoAction: (() -> Void)?
    private var hideTimer: Timer?

    init() {
        super.init(frame: .zero)
        material = .hudWindow
        blendingMode = .withinWindow
        state = .active
        // Rounded the visual-effect way: a mask image, not a layer radius.
        maskImage = NSImage(size: NSSize(width: 21, height: 21), flipped: false) { rect in
            NSColor.black.setFill()
            NSBezierPath(roundedRect: rect, xRadius: 10, yRadius: 10).fill()
            return true
        }
        maskImage?.capInsets = NSEdgeInsets(top: 10, left: 10, bottom: 10, right: 10)
        maskImage?.resizingMode = .stretch
        translatesAutoresizingMaskIntoConstraints = false
        label.font = NSFont.systemFont(ofSize: NSFont.systemFontSize)
        label.lineBreakMode = .byTruncatingTail
        undo.bezelStyle = .push
        undo.controlSize = .small
        undo.target = self
        undo.action = #selector(undoClicked)
        let row = NSStackView(views: [label, undo])
        row.orientation = .horizontal
        row.spacing = 12
        row.edgeInsets = NSEdgeInsets(top: 8, left: 14, bottom: 8, right: 14)
        row.translatesAutoresizingMaskIntoConstraints = false
        addSubview(row)
        NSLayoutConstraint.activate([
            row.topAnchor.constraint(equalTo: topAnchor),
            row.bottomAnchor.constraint(equalTo: bottomAnchor),
            row.leadingAnchor.constraint(equalTo: leadingAnchor),
            row.trailingAnchor.constraint(equalTo: trailingAnchor),
        ])
        isHidden = true
    }

    required init?(coder: NSCoder) { fatalError() }

    var message: String? { isHidden ? nil : label.stringValue }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .arrow)
    }

    func show(_ text: String, undo action: (() -> Void)?) {
        label.stringValue = text
        undoAction = action
        undo.isHidden = action == nil
        isHidden = false
        alphaValue = 1
        hideTimer?.invalidate()
        hideTimer = Timer.scheduledTimer(withTimeInterval: action == nil ? 3 : 6, repeats: false) { [weak self] _ in
            self?.dismiss()
        }
        NSAccessibility.post(element: self, notification: .announcementRequested,
                             userInfo: [.announcement: text, .priority: NSAccessibilityPriorityLevel.medium.rawValue])
    }

    func dismiss() {
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.2
            animator().alphaValue = 0
        }, completionHandler: { [weak self] in
            self?.isHidden = true
        })
    }

    @objc private func undoClicked() {
        let a = undoAction
        undoAction = nil
        dismiss()
        a?()
    }
}
