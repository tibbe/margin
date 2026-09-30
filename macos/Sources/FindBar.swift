import AppKit

/// Find and replace over the text as shown: hidden Markdown syntax is
/// skipped, so "bold text" finds `**bold** text`. Replacing goes through the
/// editing rules, so formatting around a match survives.
final class FindBar: NSView, NSSearchFieldDelegate, NSTextFieldDelegate {
    unowned let view: DocTextView
    let search = NSSearchField()
    let replaceField = NSTextField()
    private let count = NSTextField(labelWithString: "")
    private let matchCase = NSButton(title: "Aa", target: nil, action: nil)
    private let steps = NSSegmentedControl()
    private let replaceRow = NSStackView()
    private(set) var matches: [NSRange] = []
    private(set) var current: Int?
    /// Shown, hidden, or matches changed: highlights need redrawing.
    var onChange: (() -> Void)?
    var onClose: (() -> Void)?

    init(view: DocTextView) {
        self.view = view
        // The scroll view's find bar slot sizes it by frame.
        super.init(frame: NSRect(x: 0, y: 0, width: 600, height: 40))
        search.placeholderString = "Find"
        search.sendsWholeSearchString = false
        search.sendsSearchStringImmediately = true
        search.delegate = self
        search.target = self
        search.action = #selector(searchChanged)
        replaceField.placeholderString = "Replace with"
        replaceField.delegate = self
        replaceField.bezelStyle = .roundedBezel
        count.font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        count.textColor = .secondaryLabelColor
        matchCase.setButtonType(.pushOnPushOff)
        matchCase.bezelStyle = .toolbar
        matchCase.toolTip = "Match Case"
        matchCase.target = self
        matchCase.action = #selector(searchChanged)
        steps.segmentCount = 2
        steps.setImage(NSImage(systemSymbolName: "chevron.left", accessibilityDescription: "Previous"), forSegment: 0)
        steps.setImage(NSImage(systemSymbolName: "chevron.right", accessibilityDescription: "Next"), forSegment: 1)
        steps.setToolTip("Find Previous (⇧⌘G)", forSegment: 0)
        steps.setToolTip("Find Next (⌘G)", forSegment: 1)
        steps.trackingMode = .momentary
        steps.target = self
        steps.action = #selector(stepClicked)
        let done = NSButton(title: "Done", target: self, action: #selector(doneClicked))
        done.bezelStyle = .toolbar
        let findRow = NSStackView(views: [search, count, matchCase, steps, done])
        findRow.orientation = .horizontal
        findRow.spacing = 8
        let replaceButton = NSButton(title: "Replace", target: self, action: #selector(replaceClicked))
        replaceButton.bezelStyle = .toolbar
        let allButton = NSButton(title: "All", target: self, action: #selector(replaceAllClicked))
        allButton.bezelStyle = .toolbar
        replaceRow.setViews([replaceField, replaceButton, allButton], in: .leading)
        replaceRow.orientation = .horizontal
        replaceRow.spacing = 8
        let rows = NSStackView(views: [findRow, replaceRow])
        rows.orientation = .vertical
        rows.alignment = .leading
        rows.spacing = 6
        rows.edgeInsets = NSEdgeInsets(top: 6, left: 10, bottom: 6, right: 10)
        rows.translatesAutoresizingMaskIntoConstraints = false
        addSubview(rows)
        let sep = NSBox()
        sep.boxType = .separator
        sep.translatesAutoresizingMaskIntoConstraints = false
        addSubview(sep)
        NSLayoutConstraint.activate([
            rows.topAnchor.constraint(equalTo: topAnchor),
            rows.leadingAnchor.constraint(equalTo: leadingAnchor),
            rows.trailingAnchor.constraint(equalTo: trailingAnchor),
            rows.bottomAnchor.constraint(equalTo: sep.topAnchor),
            sep.leadingAnchor.constraint(equalTo: leadingAnchor),
            sep.trailingAnchor.constraint(equalTo: trailingAnchor),
            sep.bottomAnchor.constraint(equalTo: bottomAnchor),
            search.widthAnchor.constraint(equalToConstant: 260),
            replaceField.widthAnchor.constraint(equalTo: search.widthAnchor),
        ])
        isHidden = true
        replaceRow.isHidden = true
    }

    required init?(coder: NSCoder) { fatalError() }

    var isOpen: Bool { !isHidden }

    /// The system's find pasteboard, shared with other apps' Find.
    private let findPasteboard = NSPasteboard(name: .find)

    private func publishSearch() {
        guard !search.stringValue.isEmpty else { return }
        findPasteboard.clearContents()
        findPasteboard.setString(search.stringValue, forType: .string)
    }

    /// Shows the bar, starting from the selected text if there is some,
    /// else from what was last searched for (in any app).
    func open(showingReplaceField: Bool) {
        if let sel = view.selection {
            let s = view.visibleText(sel)
            if !s.contains("\n") { search.stringValue = s }
        } else if search.stringValue.isEmpty, let s = findPasteboard.string(forType: .string) {
            search.stringValue = s
        }
        isHidden = false
        replaceRow.isHidden = !showingReplaceField
        refresh(goingToMatchAtOrAfterCursor: true)
        window?.makeFirstResponder(showingReplaceField && !search.stringValue.isEmpty ? replaceField : search)
        onChange?()
    }

    func useSelection() {
        if let sel = view.selection {
            search.stringValue = view.visibleText(sel)
            publishSearch()
            if isOpen { refresh(goingToMatchAtOrAfterCursor: false) }
        }
    }

    /// Hides the bar and selects the current match.
    func close() {
        let m = current.flatMap { $0 < matches.count ? matches[$0] : nil }
        isHidden = true
        matches = []
        current = nil
        onChange?()
        window?.makeFirstResponder(view)
        if let m { view.setSelectedRange(m) }
        onClose?()
    }

    /// Recomputes matches after the text or query changes. When
    /// `goingToMatchAtOrAfterCursor` is true, makes the first match at or
    /// after the cursor the current one and scrolls to it (the selection
    /// stays).
    func refresh(goingToMatchAtOrAfterCursor: Bool) {
        guard isOpen else { return }
        view.ensureFresh()
        let needle = search.stringValue
        matches =
            needle.isEmpty
            ? [] : view.analysis.findAll(needle: needle, matchCase: matchCase.state == .on).map { NSRange($0) }
        if matches.isEmpty {
            current = nil
        } else if goingToMatchAtOrAfterCursor || current == nil {
            let c = view.selectedRange().location
            current = matches.firstIndex { $0.location >= c } ?? 0
        } else {
            current = min(current!, matches.count - 1)
        }
        updateCount()
        if goingToMatchAtOrAfterCursor { reveal() }
        onChange?()
    }

    private func updateCount() {
        switch (current, matches.count) {
        case (_, 0): count.stringValue = search.stringValue.isEmpty ? "" : "Not found"
        case (let i?, let n): count.stringValue = "\(i + 1) of \(n)"
        default: count.stringValue = "\(matches.count)"
        }
    }

    /// Highlights for the matches, the current one strongest.
    func highlights() -> [(NSRange, NSColor)] {
        guard isOpen else { return [] }
        return matches.enumerated().map { ($0.element, Theme.findMatch(current: $0.offset == current)) }
    }

    private func reveal() {
        guard let i = current, i < matches.count else { return }
        view.scrollRangeToVisible(matches[i])
        view.showFindIndicator(for: matches[i])
    }

    func step(forward: Bool) {
        if !isOpen {
            if let s = findPasteboard.string(forType: .string), !s.isEmpty, view.selection == nil {
                search.stringValue = s
            }
            open(showingReplaceField: false)
        }
        guard !matches.isEmpty else { return }
        let i = current ?? 0
        current = forward ? (i + 1) % matches.count : (i + matches.count - 1) % matches.count
        updateCount()
        reveal()
        onChange?()
    }

    private func replace(_ r: NSRange, with text: String) {
        view.ensureFresh()
        let a = view.analysis
        if let plan = a.replacePlain(start: UInt32(r.location), end: UInt32(NSMaxRange(r)), with: text) {
            view.apply(plan)
        } else {
            view.apply(a.deleteRange(start: UInt32(r.location), end: UInt32(NSMaxRange(r))))
            if !text.isEmpty {
                view.run { a, c, _ in a.insert(pos: UInt32(c), text: text) }
            }
        }
    }

    func replaceCurrent() {
        guard let i = current, i < matches.count else { return }
        view.undoManager?.beginUndoGrouping()
        replace(matches[i], with: replaceField.stringValue)
        view.undoManager?.endUndoGrouping()
        refresh(goingToMatchAtOrAfterCursor: false)
        reveal()
    }

    func replaceAll() {
        view.ensureFresh()
        if let plan = view.analysis.replaceAll(
            needle: search.stringValue, matchCase: matchCase.state == .on, with: replaceField.stringValue)
        {
            view.apply(plan)
        }
        refresh(goingToMatchAtOrAfterCursor: false)
    }

    @objc private func searchChanged() {
        publishSearch()
        refresh(goingToMatchAtOrAfterCursor: true)
    }
    @objc private func stepClicked() { step(forward: steps.selectedSegment == 1) }
    @objc private func doneClicked() { close() }
    @objc private func replaceClicked() { replaceCurrent() }
    @objc private func replaceAllClicked() { replaceAll() }

    func controlTextDidChange(_ obj: Notification) {
        if (obj.object as? NSSearchField) === search {
            publishSearch()
            refresh(goingToMatchAtOrAfterCursor: true)
        }
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(NSResponder.cancelOperation(_:)):
            close()
            return true
        case #selector(NSResponder.insertNewline(_:)):
            if control === replaceField {
                replaceCurrent()
            } else {
                step(forward: !(NSApp.currentEvent?.modifierFlags.contains(.shift) ?? false))
            }
            return true
        default:
            return false
        }
    }
}
