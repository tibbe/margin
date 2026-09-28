import AppKit

private let clockFormatter: DateFormatter = {
    let f = DateFormatter()
    f.setLocalizedDateFormatFromTemplate("jmm")  // the user's 12- or 24-hour clock
    return f
}()

private let dayFormatter: DateFormatter = {
    let f = DateFormatter()
    f.setLocalizedDateFormatFromTemplate("MMMd")
    return f
}()

private let relativeFormatter: DateFormatter = {
    let f = DateFormatter()
    f.dateStyle = .short
    f.timeStyle = .short
    f.doesRelativeDateFormatting = true
    return f
}()

/// When a message was written: the time today, "Yesterday" and the time,
/// else the date and time, in the user's formats.
func timeLabel(ms: Int64) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(ms) / 1000)
    let cal = Calendar.current
    if cal.isDateInToday(date) { return clockFormatter.string(from: date) }
    if cal.isDateInYesterday(date) { return relativeFormatter.string(from: date) }
    return "\(dayFormatter.string(from: date)), \(clockFormatter.string(from: date))"
}

private func wrappingLabel(_ text: String, font: NSFont, color: NSColor = .labelColor) -> NSTextField {
    let l = NSTextField(wrappingLabelWithString: text)
    l.font = font
    l.textColor = color
    l.isSelectable = true
    l.lineBreakMode = .byWordWrapping
    return l
}

/// The text box of a comment card: grows with its text. Cmd+Return
/// submits; Escape cancels.
final class ComposerTextView: NSTextView {
    var onSubmit: (() -> Void)?
    var onCancel: (() -> Void)?
    var onResize: (() -> Void)?
    var onFocusChange: (() -> Void)?
    var placeholder = "" { didSet { needsDisplay = true } }

    /// As tall as its text; the width comes from the layout.
    override var intrinsicContentSize: NSSize {
        guard let lm = layoutManager, let tc = textContainer else { return NSSize(width: NSView.noIntrinsicMetric, height: 26) }
        lm.ensureLayout(for: tc)
        let h = ceil(lm.usedRect(for: tc).height + 2 * textContainerInset.height)
        return NSSize(width: NSView.noIntrinsicMetric, height: max(26, h))
    }

    override func setFrameSize(_ newSize: NSSize) {
        let widthChanged = newSize.width != frame.width
        super.setFrameSize(newSize)
        if widthChanged { invalidateIntrinsicContentSize() }
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        let mods = event.modifierFlags.intersection([.command, .shift, .option, .control])
        if window?.firstResponder === self, mods == [.command],
           event.keyCode == 36 || event.keyCode == 76 {
            onSubmit?()
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    override func cancelOperation(_ sender: Any?) {
        onCancel?()
    }

    override func didChangeText() {
        super.didChangeText()
        needsDisplay = true
        invalidateIntrinsicContentSize()
        onResize?()
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        if string.isEmpty && !placeholder.isEmpty {
            let attrs: [NSAttributedString.Key: Any] = [
                .font: font ?? NSFont.systemFont(ofSize: NSFont.systemFontSize),
                .foregroundColor: NSColor.placeholderTextColor,
            ]
            (placeholder as NSString).draw(at: NSPoint(x: textContainerInset.width + 1, y: textContainerInset.height), withAttributes: attrs)
        }
    }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        onFocusChange?()
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        onFocusChange?()
        return ok
    }
}

/// A multi-line text box with submit and cancel buttons.
final class Composer: NSView {
    let textView: ComposerTextView
    let submit: NSButton
    let cancel: NSButton
    private let box = NSView()
    var onSubmit: ((String) -> Void)?
    var onCancel: (() -> Void)?
    var onResize: (() -> Void)?
    /// Show the buttons even while the box is empty and unfocused.
    var alwaysShowButtons: Bool

    init(placeholder: String, submitLabel: String, alwaysShowButtons: Bool) {
        self.alwaysShowButtons = alwaysShowButtons
        textView = ComposerTextView(frame: NSRect(x: 0, y: 0, width: 240, height: 22))
        submit = NSButton(title: submitLabel, target: nil, action: nil)
        cancel = NSButton(title: "Cancel", target: nil, action: nil)
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        textView.placeholder = placeholder
        textView.font = NSFont.systemFont(ofSize: NSFont.systemFontSize)
        textView.isRichText = false
        textView.allowsUndo = true
        textView.drawsBackground = false
        textView.textContainerInset = NSSize(width: 4, height: 5)
        textView.textContainer?.lineFragmentPadding = 2
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isVerticallyResizable = false
        textView.translatesAutoresizingMaskIntoConstraints = false
        textView.onSubmit = { [weak self] in self?.submitText() }
        textView.onCancel = { [weak self] in self?.onCancel?() }
        textView.onResize = { [weak self] in
            self?.syncButtons()
            self?.onResize?()
        }
        textView.onFocusChange = { [weak self] in self?.focusChanged() }

        box.wantsLayer = true
        box.translatesAutoresizingMaskIntoConstraints = false
        box.layer?.cornerRadius = 6
        box.layer?.borderWidth = 1
        addSubview(box)

        submit.bezelStyle = .push
        submit.keyEquivalent = ""
        submit.controlSize = .small
        submit.target = self
        submit.action = #selector(submitClicked)
        cancel.bezelStyle = .push
        cancel.controlSize = .small
        cancel.target = self
        cancel.action = #selector(cancelClicked)
        let buttons = NSStackView(views: [NSView(), cancel, submit])
        buttons.orientation = .horizontal
        buttons.spacing = 6
        // A stack view, so hidden buttons take no room.
        let column = NSStackView(views: [textView, buttons])
        column.orientation = .vertical
        column.alignment = .leading
        column.spacing = 6
        column.translatesAutoresizingMaskIntoConstraints = false
        addSubview(column)
        NSLayoutConstraint.activate([
            column.topAnchor.constraint(equalTo: topAnchor),
            column.leadingAnchor.constraint(equalTo: leadingAnchor),
            column.trailingAnchor.constraint(equalTo: trailingAnchor),
            column.bottomAnchor.constraint(equalTo: bottomAnchor),
            textView.widthAnchor.constraint(equalTo: column.widthAnchor),
            buttons.widthAnchor.constraint(equalTo: column.widthAnchor),
            box.topAnchor.constraint(equalTo: textView.topAnchor),
            box.leadingAnchor.constraint(equalTo: textView.leadingAnchor),
            box.trailingAnchor.constraint(equalTo: textView.trailingAnchor),
            box.bottomAnchor.constraint(equalTo: textView.bottomAnchor),
        ])
        buttonsView = buttons
        syncButtons()
    }

    private var buttonsView: NSStackView!

    required init?(coder: NSCoder) { fatalError() }

    var text: String {
        get { textView.string.trimmingCharacters(in: .whitespacesAndNewlines) }
        set {
            textView.string = newValue
            textView.invalidateIntrinsicContentSize()
            syncButtons()
            onResize?()
        }
    }

    func focus() {
        window?.makeFirstResponder(textView)
    }

    var hasFocus: Bool { window?.firstResponder === textView }

    func focusChanged() {
        syncButtons()
        applyColors()
    }

    private func syncButtons() {
        let show = alwaysShowButtons || hasFocus || !textView.string.isEmpty
        if buttonsView.isHidden == show {
            buttonsView.isHidden = !show
            onResize?()
        }
        submit.isEnabled = !text.isEmpty
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyColors()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        applyColors()
    }

    private func applyColors() {
        effectiveAppearance.performAsCurrentDrawingAppearance {
            box.layer?.backgroundColor = NSColor.textBackgroundColor.cgColor
            box.layer?.borderColor = (hasFocus ? NSColor.controlAccentColor : NSColor.separatorColor).cgColor
            box.layer?.borderWidth = hasFocus ? 2 : 1
        }
    }

    private func submitText() {
        let t = text
        if !t.isEmpty { onSubmit?(t) }
    }

    @objc private func submitClicked() { submitText() }
    @objc private func cancelClicked() { onCancel?() }
}

/// A card in the gutter: a comment thread, or the draft of a new one.
/// Clicks on a card stay with it and never reach the text.
class GutterCard: NSView {
    let stack = NSStackView()
    private var widthConstraint: NSLayoutConstraint!
    var active = false { didSet { if oldValue != active { activeChanged() } } }
    var onClick: (() -> Void)?
    static let pad = NSEdgeInsets(top: 10, left: 12, bottom: 10, right: 12)

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.cornerRadius = 8
        layer?.borderWidth = 1
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        widthConstraint = stack.widthAnchor.constraint(equalToConstant: 276)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: topAnchor, constant: GutterCard.pad.top),
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: GutterCard.pad.left),
            widthConstraint,
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }
    override var wantsUpdateLayer: Bool { true }

    override func updateLayer() {
        layer?.backgroundColor = Theme.cardBackground.cgColor
        layer?.borderColor = (active ? Theme.accent : Theme.border).cgColor
        layer?.borderWidth = active ? 2 : 1
        layer?.shadowOpacity = 0
    }

    func activeChanged() {
        needsDisplay = true
    }

    /// The height the card needs at `width`.
    func height(forWidth width: CGFloat) -> CGFloat {
        let inner = width - GutterCard.pad.left - GutterCard.pad.right
        if widthConstraint.constant != inner {
            widthConstraint.constant = inner
            setLabelWidths(inner)
        }
        stack.layoutSubtreeIfNeeded()
        return ceil(stack.fittingSize.height + GutterCard.pad.top + GutterCard.pad.bottom)
    }

    func setLabelWidths(_ w: CGFloat) {
        for v in stack.arrangedSubviews {
            if let l = v as? NSTextField { l.preferredMaxLayoutWidth = w }
            if let s = v as? NSStackView {
                for sub in s.arrangedSubviews { (sub as? NSTextField)?.preferredMaxLayoutWidth = w }
            }
        }
    }

    override func mouseDown(with event: NSEvent) {
        onClick?()
    }
}

/// A comment thread's card: the comment, its replies, and a reply box.
final class ThreadCard: GutterCard {
    let id: UInt64
    let composer = Composer(placeholder: "Reply…", submitLabel: "Reply", alwaysShowButtons: false)
    private var resolved = false
    var onResolve: ((Bool) -> Void)?
    var onDelete: (() -> Void)?
    var onReply: ((String) -> Void)?
    var onResize: (() -> Void)?
    /// Leaving the reply box: focus goes back to the text.
    var onLeave: (() -> Void)?
    private(set) var resolveButton: NSButton?

    init(thread: CommentThread) {
        id = thread.id
        super.init(frame: .zero)
        composer.onSubmit = { [weak self] body in
            self?.onReply?(body)
            self?.composer.text = ""
        }
        composer.onCancel = { [weak self] in
            self?.composer.text = ""
            self?.onLeave?()
        }
        composer.onResize = { [weak self] in self?.onResize?() }
        update(thread)
    }

    required init?(coder: NSCoder) { fatalError() }

    /// Rebuilds the card's contents, keeping any reply being written.
    func update(_ thread: CommentThread) {
        resolved = thread.resolved
        for v in stack.arrangedSubviews {
            stack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        let small = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        let header = NSStackView()
        header.orientation = .horizontal
        header.spacing = 2
        let time = NSTextField(labelWithString: thread.messages.first.map { timeLabel(ms: $0.atMs) } ?? "")
        time.font = small
        time.textColor = .secondaryLabelColor
        header.addArrangedSubview(time)
        header.addArrangedSubview(NSView())
        let resolve = NSButton(image: NSImage(systemSymbolName: thread.resolved ? "arrow.uturn.backward.circle" : "checkmark.circle",
                                              accessibilityDescription: thread.resolved ? "Reopen" : "Resolve")!,
                               target: self, action: #selector(resolveClicked))
        resolve.isBordered = false
        resolve.toolTip = thread.resolved ? "Reopen" : "Resolve"
        resolve.contentTintColor = .secondaryLabelColor
        resolveButton = resolve
        let more = NSButton(image: NSImage(systemSymbolName: "ellipsis.circle", accessibilityDescription: "More")!,
                            target: self, action: #selector(moreClicked(_:)))
        more.isBordered = false
        more.toolTip = "More"
        more.contentTintColor = .secondaryLabelColor
        header.addArrangedSubview(resolve)
        header.addArrangedSubview(more)
        stack.addArrangedSubview(header)
        header.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true

        if thread.detached {
            let q = wrappingLabel("“\(thread.quote.trimmingCharacters(in: .whitespacesAndNewlines))”",
                                  font: NSFontManager.shared.convert(NSFont.systemFont(ofSize: NSFont.smallSystemFontSize), toHaveTrait: .italicFontMask),
                                  color: .secondaryLabelColor)
            q.attributedStringValue = NSAttributedString(string: q.stringValue, attributes: [
                .font: q.font!, .foregroundColor: NSColor.secondaryLabelColor,
                .strikethroughStyle: NSUnderlineStyle.single.rawValue,
            ])
            q.toolTip = "The commented text was deleted"
            stack.addArrangedSubview(q)
        }
        let bodyFont = NSFont.systemFont(ofSize: NSFont.systemFontSize)
        for (i, m) in thread.messages.enumerated() {
            if i > 0 {
                let sep = NSBox()
                sep.boxType = .separator
                stack.addArrangedSubview(sep)
                sep.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
                let t = NSTextField(labelWithString: timeLabel(ms: m.atMs))
                t.font = small
                t.textColor = .secondaryLabelColor
                stack.addArrangedSubview(t)
            }
            stack.addArrangedSubview(wrappingLabel(m.body, font: bodyFont))
        }
        if thread.resolved {
            let when = thread.resolvedAtMs.map { timeLabel(ms: $0) } ?? ""
            let l = NSTextField(labelWithString: "Resolved \(when)")
            l.font = small
            l.textColor = .secondaryLabelColor
            stack.addArrangedSubview(l)
        }
        stack.addArrangedSubview(composer)
        composer.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        alphaValue = thread.resolved ? 0.7 : 1
        syncComposer()
        onResize?()
    }

    override func activeChanged() {
        super.activeChanged()
        syncComposer()
    }

    private func syncComposer() {
        let show = (active && !resolved) || !composer.textView.string.isEmpty
        composer.isHidden = !show
    }

    var hasFocus: Bool {
        guard let r = window?.firstResponder as? NSView else { return false }
        return r.isDescendant(of: self)
    }

    @objc private func resolveClicked() {
        onResolve?(!resolved)
    }

    /// Right-click (or Control-click): the card's commands.
    override func menu(for event: NSEvent) -> NSMenu? {
        onClick?()
        let menu = NSMenu()
        if !resolved {
            let reply = NSMenuItem(title: "Reply", action: #selector(replyClicked), keyEquivalent: "")
            reply.target = self
            menu.addItem(reply)
        }
        let resolve = NSMenuItem(title: resolved ? "Reopen" : "Resolve", action: #selector(resolveClicked), keyEquivalent: "")
        resolve.target = self
        menu.addItem(resolve)
        menu.addItem(.separator())
        let delete = NSMenuItem(title: "Delete Thread", action: #selector(deleteClicked), keyEquivalent: "")
        delete.target = self
        menu.addItem(delete)
        return menu
    }

    @objc private func replyClicked() {
        composer.isHidden = false
        composer.focus()
    }

    @objc private func moreClicked(_ sender: NSButton) {
        let menu = NSMenu()
        let item = NSMenuItem(title: "Delete Thread", action: #selector(deleteClicked), keyEquivalent: "")
        item.target = self
        menu.addItem(item)
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: sender.bounds.height + 4), in: sender)
    }

    @objc private func deleteClicked() {
        onDelete?()
    }
}

/// The card for a comment being written.
final class DraftCard: GutterCard {
    let composer = Composer(placeholder: "Comment for the agent…", submitLabel: "Comment", alwaysShowButtons: true)

    override init(frame: NSRect) {
        super.init(frame: frame)
        stack.addArrangedSubview(composer)
        composer.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
        active = true
    }

    required init?(coder: NSCoder) { fatalError() }
}
