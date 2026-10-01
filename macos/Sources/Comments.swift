import AppKit
import margin_ffi

extension AnchorPlace {
    /// Where the text starts, or was.
    var start: Int {
        switch self {
        case .on(let start, _): Int(start)
        case .detached(let at): Int(at)
        }
    }

    /// The text's range; nil for deleted text.
    var range: NSRange? {
        guard case .on(let start, let end) = self, start < end else { return nil }
        return NSRange(location: Int(start), length: Int(end - start))
    }

    var isDetached: Bool { range == nil }
}

extension CommentThread {
    var resolved: Bool { resolvedAtMs != nil }
}

/// A thread in the gutter, anchored to text in the editor. Anchors follow
/// edits as the GTK editor's text marks do: text inserted at the start is
/// excluded, text inserted at the end too.
final class ThreadItem {
    /// As the store last had it.
    var thread: CommentThread
    /// Where its text is now.
    var place: AnchorPlace
    let card: ThreadCard

    init(thread: CommentThread, card: ThreadCard) {
        self.thread = thread
        place = thread.place
        self.card = card
    }

    /// Where its text starts, or was.
    var start: Int { place.start }
    /// Its text's range; nil once the text was deleted.
    var range: NSRange? { place.range }
    var detached: Bool { place.isDetached }
}

private struct Draft {
    var start: Int
    var end: Int
    let card: DraftCard
}

/// Comment threads in the gutter: anchoring them to text, laying out their
/// cards beside the text, keeping focus in sync between highlights and
/// cards, and reading and writing the store the CLI shares.
final class CommentLayer {
    unowned let view: DocTextView
    unowned let page: PageView
    private var gutter: GutterView { page.gutter }
    private(set) var store: CommentStore?
    /// Changes made to the store from here, counted so a reload doesn't
    /// show what one of them replaced.
    private var changesMade = 0
    private(set) var items: [ThreadItem] = []
    /// What has the gutter's focus: a thread, or the draft of a new one,
    /// never both.
    private enum Focus {
        case none
        case thread(UInt64)
        case draft(Draft)
    }
    private var focus = Focus.none
    /// The focused thread.
    var active: UInt64? {
        if case .thread(let id) = focus { id } else { nil }
    }
    private var draft: Draft? {
        if case .draft(let d) = focus { d } else { nil }
    }
    /// The range the draft comments on, if one is open.
    var draftRange: NSRange? { draft.map { NSRange(location: $0.start, length: $0.end - $0.start) } }
    var showsResolved = false {
        didSet {
            if !showsResolved, let a = active, items.contains(where: { $0.thread.id == a && $0.thread.resolved }) {
                focus = .none
            }
            sync()
        }
    }
    /// The counts changed.
    var onChange: (() -> Void)?
    /// Says something at the bottom of the window, with an optional Undo.
    var toast: ((String, (() -> Void)?) -> Void)?
    /// An agent changed threads (after the announcement).
    var onActivity: (([ThreadActivity]) -> Void)?
    /// Runs before a thread is added (the window saves the document).
    var beforeAdd: (() -> Void)?
    /// Find matches, highlighted above comment highlights.
    var extraHighlights: (() -> [(NSRange, NSColor)])?

    static let cardGap: CGFloat = 10

    init(page: PageView) {
        self.page = page
        view = page.textView
        page.gutter.onEmptyClick = { [weak self] in self?.leave() }
        page.placeCards = { [weak self] in self?.placeCards() ?? 0 }
    }

    /// The number of open threads.
    ///
    /// - Complexity: O(n), where n is the number of threads.
    var openCount: Int { items.filter { !$0.thread.resolved }.count }
    /// The number of resolved threads.
    ///
    /// - Complexity: O(n), where n is the number of threads.
    var resolvedCount: Int { items.filter { $0.thread.resolved }.count }

    func visible(_ t: CommentThread) -> Bool {
        !t.resolved || showsResolved
    }

    // MARK: - Anchors

    /// Moves anchors across an edit: `range` is the edited range in the new
    /// text, `delta` the change in length.
    func textEdited(range: NSRange, delta: Int) {
        let loc = range.location
        let oldEnd = loc + range.length - delta
        let newLen = range.length
        func map(_ p: Int, stickRight: Bool) -> Int {
            if p < loc { return p }
            if p > oldEnd { return p + delta }
            // Inside the replaced text, or at its edges: as a deletion
            // followed by an insertion at `loc`.
            if p == loc && !stickRight { return loc }
            return stickRight ? loc + newLen : loc
        }
        for it in items {
            guard let r = it.range else {
                it.place = .detached(at: UInt32(map(it.start, stickRight: true)))
                continue
            }
            let (s, e) = (map(r.location, stickRight: true), map(NSMaxRange(r), stickRight: false))
            // All of its text deleted: detached where it was.
            it.place = s < e ? .on(start: UInt32(s), end: UInt32(e)) : .detached(at: UInt32(s))
        }
        if case .draft(var d) = focus {
            d.start = map(d.start, stickRight: true)
            d.end = map(d.end, stickRight: false)
            focus = .draft(d)
        }
    }

    private func anchors() -> [ThreadAnchor] {
        items.map { ThreadAnchor(id: $0.thread.id, place: $0.place) }
    }

    /// Open threads, anchored where their text is now.
    func openThreads() -> [CommentThread] {
        items.filter { !$0.thread.resolved }.map { it in
            var t = it.thread
            t.place = it.place
            return t
        }
    }

    // MARK: - Store

    func attach(_ store: CommentStore?) {
        self.store = store
        // What is there when the document opens is not news.
        if let store {
            do { merge(try store.load(text: view.string), announce: false) } catch {
                toast?("Could not read comments: \(error.localizedDescription)", nil)
            }
        } else {
            merge([], announce: false)
        }
    }

    /// Re-reads the store, e.g. after an agent replied from the CLI.
    /// Reading waits for the store's lock, which the CLI may hold, so it
    /// happens off the main thread; the result is shown if neither the text
    /// nor the threads (from here) changed meanwhile, else it reads again.
    func reload() {
        guard let store else { return }
        let text = view.string
        let changes = changesMade
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let result = Result { try store.load(text: text) }
            DispatchQueue.main.async {
                guard let self, self.store === store else { return }
                switch result {
                case .success(let threads):
                    // Read before a change made here since, it would put
                    // back what that change replaced.
                    if self.view.string == text && self.changesMade == changes {
                        self.merge(threads, announce: true)
                    } else {
                        self.reload()
                    }
                case .failure(let error):
                    self.toast?("Could not read comments: \(error.localizedDescription)", nil)
                }
            }
        }
    }

    /// Records the editor's anchors, then makes `change`, under the store's
    /// lock; shows the result.
    @discardableResult
    private func update(_ change: CommentChange) -> CommentState? {
        guard let store else { return nil }
        do {
            let state = try store.update(text: view.string, anchors: anchors(), change: change)
            changesMade += 1
            merge(state.threads, announce: false)
            return state
        } catch {
            toast?("Could not save comments: \(error.localizedDescription)", nil)
            return nil
        }
    }

    /// Writes anchors against the saved text, after the document was saved.
    func persistAnchors() {
        if !items.isEmpty { update(.anchors) }
    }

    /// Brings the gutter in line with `threads`.
    private func merge(_ threads: [CommentThread], announce: Bool) {
        // Our own changes reach `items` before the store's file watcher
        // fires, so whatever is new here came from someone else (an agent).
        let activity = announce ? threadActivity(old: items.map(\.thread), new: threads) : []
        var keep = Set<UInt64>()
        for t in threads {
            keep.insert(t.id)
            if let it = items.first(where: { $0.thread.id == t.id }) {
                let changed =
                    it.thread.resolved != t.resolved || it.thread.messages != t.messages
                    || it.thread.place.isDetached != t.place.isDetached
                if changed {
                    let keepAnchor = !t.place.isDetached && !it.thread.place.isDetached
                    it.thread = t
                    if !keepAnchor { it.place = t.place }
                    it.card.update(t)
                }
            } else {
                items.append(makeItem(t))
            }
        }
        for it in items where !keep.contains(it.thread.id) {
            it.card.removeFromSuperview()
        }
        items.removeAll { !keep.contains($0.thread.id) }
        if let a = active, !items.contains(where: { $0.thread.id == a && visible($0.thread) }) {
            focus = .none
        }
        sync()
        onChange?()
        if !activity.isEmpty {
            toast?(activitySummary(activity: activity), nil)
            onActivity?(activity)
        }
    }

    private func makeItem(_ t: CommentThread) -> ThreadItem {
        let card = ThreadCard(thread: t)
        let id = t.id
        card.onClick = { [weak self] in self?.activate(id, scroll: false, focusCard: true) }
        card.onResolve = { [weak self] r in self?.setResolved(forThread: id, to: r) }
        card.onDeleteMessage = { [weak self] i in self?.deleteMessage(at: i, inThread: id) }
        card.onEdit = { [weak self] i, body in self?.editMessage(at: i, inThread: id, to: body) }
        card.onFocusThread = { [weak self] in
            if self?.active != id { self?.activate(id, scroll: false) }
        }
        card.onReply = { [weak self] body in self?.addReply(toThread: id, body: body) }
        card.onResize = { [weak self] in self?.queueRelayout() }
        card.onLeave = { [weak self] in self?.leave() }
        gutter.addSubview(card)
        return ThreadItem(thread: t, card: card)
    }

    // MARK: - Commands

    /// Starts a comment on the selection, or the word at the cursor.
    func beginDraft() {
        if let d = draft {
            d.card.composer.focus()
            return
        }
        guard store != nil else { return }
        view.ensureFresh()
        let a = view.analysis
        let sel =
            view.selection.map { TextRange(start: UInt32($0.location), end: UInt32(NSMaxRange($0))) }
            ?? a.wordAt(pos: UInt32(view.cursor))
        guard let s = sel, let range = a.trimSegment(start: s.start, end: s.end) else {
            toast?("Select the text you want to comment on", nil)
            return
        }
        let card = DraftCard()
        card.composer.onSubmit = { [weak self] body in self?.postDraft(body) }
        card.composer.onCancel = { [weak self] in self?.cancelDraft() }
        card.composer.onResize = { [weak self] in self?.queueRelayout() }
        card.onClick = { [weak card] in card?.composer.focus() }
        gutter.addSubview(card)
        focus = .draft(Draft(start: Int(range.start), end: Int(range.end), card: card))
        // The highlight marks what is being commented on; a selection
        // would hide it.
        view.setSelectedRange(NSRange(location: Int(range.end), length: 0))
        sync()
        // Placed before it takes the keyboard, which scrolls to it.
        page.layoutSubtreeIfNeeded()
        card.composer.focus()
    }

    func cancelDraft() {
        removeDraft()
        sync()
        view.window?.makeFirstResponder(view)
    }

    private func removeDraft() {
        guard let d = draft else { return }
        d.card.removeFromSuperview()
        focus = .none
    }

    private func postDraft(_ body: String) {
        beforeAdd?()
        guard let d = draft else { return }
        // Its text was deleted meanwhile, leaving nothing to comment on;
        // the draft stays, so its words aren't lost.
        guard d.start < d.end else {
            toast?("The text you were commenting on was deleted", nil)
            return
        }
        removeDraft()
        let state = update(.add(start: UInt32(d.start), end: UInt32(max(d.start, d.end)), body: body))
        if let id = state?.added {
            activate(id, scroll: false)
        } else {
            sync()
        }
        view.window?.makeFirstResponder(view)
    }

    func addReply(toThread id: UInt64, body: String) {
        update(.reply(id: id, body: body))
    }

    /// Comment changes are undoable with ⌘Z like text edits; the banner's
    /// Undo does the same while it is still the latest step.
    private func undoable(_ name: String, banner: String, undo: @escaping (CommentLayer) -> Void) {
        guard let um = view.undoManager else { return }
        um.registerUndo(withTarget: self) { undo($0) }
        um.setActionName(name)
        toast?(banner) { [weak um] in
            guard let um, um.canUndo, um.undoActionName == name else { return }
            um.undo()
        }
    }

    func setResolved(forThread id: UInt64, to resolved: Bool) {
        guard update(.setResolved(ids: [id], resolved: resolved)) != nil else { return }
        if resolved && active == id && !showsResolved { activate(nil, scroll: false) }
        if resolved {
            undoable("Resolve Comment", banner: "Comment resolved") { $0.setResolved(forThread: id, to: false) }
        } else {
            // Reopening (or undoing a resolve): redo resolves again.
            view.undoManager?.registerUndo(withTarget: self) { $0.setResolved(forThread: id, to: true) }
            view.undoManager?.setActionName("Reopen Comment")
        }
    }

    /// Resolves every open thread; undo reopens them.
    func resolveAll() {
        let ids = items.filter { !$0.thread.resolved }.map { $0.thread.id }
        if ids.isEmpty {
            toast?("No open comments", nil)
            return
        }
        resolve(ids, true)
    }

    private func resolve(_ ids: [UInt64], _ resolved: Bool) {
        guard update(.setResolved(ids: ids, resolved: resolved)) != nil else { return }
        if resolved {
            if !showsResolved { activate(nil, scroll: false) }
            let banner = ids.count == 1 ? "Resolved 1 comment" : "Resolved \(ids.count) comments"
            undoable("Resolve All", banner: banner) { $0.resolve(ids, false) }
        } else {
            view.undoManager?.registerUndo(withTarget: self) { $0.resolve(ids, true) }
            view.undoManager?.setActionName("Reopen Comments")
        }
    }

    func delete(_ id: UInt64) {
        guard let it = items.first(where: { $0.thread.id == id }) else { return }
        // Undo puts the thread back against the text as it is then.
        var old = it.thread
        old.place = it.place
        guard update(.delete(id: id)) != nil else { return }
        if active == id { focus = .none }
        undoable("Delete Comment", banner: "Comment deleted") { $0.restore(old) }
        view.window?.makeFirstResponder(view)
    }

    private func restore(_ thread: CommentThread) {
        update(.restore(thread: thread))
        view.undoManager?.registerUndo(withTarget: self) { $0.delete(thread.id) }
        view.undoManager?.setActionName("Delete Comment")
        showUndone(thread.id)
    }

    /// Deletes message `index` of a thread; the comment (0) takes the thread.
    func deleteMessage(at index: Int, inThread id: UInt64) {
        if index == 0 { return delete(id) }
        guard let it = items.first(where: { $0.thread.id == id }), index < it.thread.messages.count else { return }
        let old = it.thread.messages[index]
        guard update(.deleteMessage(id: id, index: UInt32(index))) != nil else { return }
        undoable("Delete Reply", banner: "Reply deleted") { $0.restoreMessage(id, index, old) }
    }

    private func restoreMessage(_ id: UInt64, _ index: Int, _ message: ThreadMessage) {
        update(.insertMessage(id: id, index: UInt32(index), message: message))
        view.undoManager?.registerUndo(withTarget: self) { $0.deleteMessage(at: index, inThread: id) }
        view.undoManager?.setActionName("Delete Reply")
        showUndone(id)
    }

    /// Replaces message `index`'s text; undo puts the old text back.
    func editMessage(at index: Int, inThread id: UInt64, to body: String) {
        guard let it = items.first(where: { $0.thread.id == id }), index < it.thread.messages.count else { return }
        let old = it.thread.messages[index].body
        guard update(.edit(id: id, index: UInt32(index), body: body)) != nil else { return }
        let undoing = view.undoManager?.isUndoing ?? false
        view.undoManager?.registerUndo(withTarget: self) { $0.editMessage(at: index, inThread: id, to: old) }
        view.undoManager?.setActionName("Edit")
        if undoing || view.undoManager?.isRedoing == true { showUndone(id) }
    }

    /// After an undo or redo: the thread it changed, focused and in view.
    private func showUndone(_ id: UInt64) {
        guard items.contains(where: { $0.thread.id == id && visible($0.thread) }) else { return }
        activate(id, scroll: true)
    }

    /// The focused thread.
    ///
    /// - Complexity: O(n), where n is the number of threads.
    var focusedThread: CommentThread? {
        active.flatMap { a in items.first { $0.thread.id == a }?.thread }
    }

    /// Resolves the focused thread, or reopens it (the Comments menu).
    func toggleResolvedFocused() {
        if let t = focusedThread { setResolved(forThread: t.id, to: !t.resolved) }
    }

    /// Starts editing the focused thread's comment (the Comments menu).
    func editFocused() {
        guard let a = active, let it = items.first(where: { $0.thread.id == a }) else { return }
        it.card.beginEdit(0)
    }

    /// Focuses a thread, which drops a draft: its card moves beside its
    /// text and its highlight darkens. With `scroll`, its text is scrolled
    /// into view. Focusing no thread leaves a draft as it is.
    func activate(_ id: UInt64?, scroll: Bool, focusCard: Bool = false) {
        if let id {
            removeDraft()
            focus = .thread(id)
        } else if active != nil {
            focus = .none
        }
        sync()
        guard let id, let it = items.first(where: { $0.thread.id == id }) else { return }
        if scroll {
            let r = it.range ?? NSRange(location: it.start, length: 0)
            view.scrollRangeToVisible(r)
            // Moving the cursor may focus another thread's text.
            view.setSelectedRange(NSRange(location: NSMaxRange(r), length: 0))
            focus = .thread(id)
            sync()
        }
        if focusCard {
            // Moved beside its text, the clicked card may have left the view;
            // its top comes back into it.
            page.layoutSubtreeIfNeeded()
            let shown = page.enclosingScrollView?.contentView.bounds.height ?? it.card.bounds.height
            it.card.scrollToVisible(
                NSRect(x: 0, y: 0, width: it.card.bounds.width, height: min(it.card.bounds.height, shown)))
            if !it.thread.resolved { it.card.composer.focus() }
        }
    }

    /// Focuses thread `id` and scrolls to it, if it is shown.
    func reveal(_ id: UInt64) {
        if items.contains(where: { $0.thread.id == id && visible($0.thread) }) { activate(id, scroll: true) }
    }

    /// The next (or previous) visible thread in text order.
    func step(forward: Bool) {
        let order = items.filter { visible($0.thread) }.sorted { ($0.start, $0.thread.id) < ($1.start, $1.thread.id) }
        guard !order.isEmpty else { return }
        let target: ThreadItem
        if let a = active, let i = order.firstIndex(where: { $0.thread.id == a }) {
            target = order[(i + (forward ? 1 : order.count - 1)) % order.count]
        } else {
            let c = view.cursor
            target =
                forward
                ? (order.first { $0.start > c } ?? order[0]) : (order.last { $0.start < c } ?? order[order.count - 1])
        }
        activate(target.thread.id, scroll: true)
    }

    /// Starts a reply on the focused thread.
    func focusReply() {
        if let a = active, let it = items.first(where: { $0.thread.id == a }), !it.thread.resolved {
            it.card.composer.focus()
        }
    }

    /// Escape: leave the focused thread, or drop the draft.
    func escape() -> Bool {
        if draft != nil {
            cancelDraft()
            return true
        }
        if active != nil {
            activate(nil, scroll: false)
            view.window?.makeFirstResponder(view)
            return true
        }
        return false
    }

    /// Leaves the focused thread (a click beside the cards, Cancel on a
    /// card): the keyboard goes back to the text, with the cursor where it
    /// was. A draft keeps its card and text, as when the text is clicked.
    func leave() {
        if active != nil { activate(nil, scroll: false) }
        view.window?.makeFirstResponder(view)
    }

    /// Whether keyboard focus is in the gutter.
    var hasFocus: Bool {
        guard let r = view.window?.firstResponder as? NSView else { return false }
        return items.contains { r.isDescendant(of: $0.card) } || (draft.map { r.isDescendant(of: $0.card) } ?? false)
    }

    func cursorMoved() {
        if view.selection != nil || draft != nil { return }
        let c = view.cursor
        let hit =
            items
            .compactMap { it in it.range.map { (it, $0) } }
            .filter { it, r in visible(it.thread) && r.location <= c && c <= NSMaxRange(r) }
            .min { ($0.1.length, $0.0.thread.id) < ($1.1.length, $1.0.thread.id) }?.0
        if let hit {
            if active != hit.thread.id { activate(hit.thread.id, scroll: false) }
        } else if let a = active, !(items.first { $0.thread.id == a }?.card.hasFocus ?? false) {
            activate(nil, scroll: false)
        }
    }

    // MARK: - Presentation

    /// Shows, hides and marks cards, and redraws highlights.
    func sync() {
        for it in items {
            it.card.isHidden = !visible(it.thread)
            it.card.active = it.thread.id == active
        }
        refreshHighlights()
        queueRelayout()
    }

    func refreshHighlights() {
        guard let lm = view.layoutManager else { return }
        let len = (view.string as NSString).length
        let all = NSRange(location: 0, length: len)
        lm.removeTemporaryAttribute(.backgroundColor, forCharacterRange: all)
        let dark = view.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        // A highlight replaces the text's own background, so inline code
        // gets its fill drawn over the highlight instead. Hidden syntax
        // gets none: a fill starting on it also covers the character
        // before, which a see-through highlight then shows doubled.
        let appearance = view.effectiveAppearance
        func mark(_ a: Int, _ b: Int, _ color: NSColor) {
            let s = max(0, min(a, len))
            let e = max(0, min(b, len))
            guard s < e, let storage = view.textStorage else { return }
            storage.enumerateAttributes(in: NSRange(location: s, length: e - s)) { attrs, r, _ in
                if attrs[.marginHidden] != nil { return }
                let c =
                    (attrs[.backgroundColor] as? NSColor).map { Theme.composite($0, over: color, in: appearance) }
                    ?? color
                lm.addTemporaryAttribute(.backgroundColor, value: c, forCharacterRange: r)
            }
        }
        for it in items where visible(it.thread) && it.thread.id != active {
            if let r = it.range {
                mark(r.location, NSMaxRange(r), Theme.commentHighlightColor(active: false, dark: dark))
            }
        }
        if let a = active, let r = items.first(where: { $0.thread.id == a })?.range {
            mark(r.location, NSMaxRange(r), Theme.commentHighlightColor(active: true, dark: dark))
        }
        if let d = draft {
            mark(d.start, d.end, Theme.commentHighlightColor(active: true, dark: dark))
        }
        for (r, c) in extraHighlights?() ?? [] {
            mark(r.location, NSMaxRange(r), c)
        }
    }

    /// Places the cards again in the page's next layout pass, which comes
    /// before the window next draws.
    func queueRelayout() {
        page.hasCards = draft != nil || items.contains { visible($0.thread) }
        page.cardsChanged()
    }

    /// Where a character's line is, in the gutter's coordinates.
    func lineTop(_ ci: Int) -> CGFloat {
        let len = (view.string as NSString).length
        return view.convert(NSPoint(x: 0, y: view.location(of: min(ci, len)).minY), to: gutter).y
    }

    /// Places cards beside their text, in the page's layout pass once the
    /// text is laid out. The focused card (or the draft) sits exactly beside
    /// its anchor; the others stack above and below it without overlapping,
    /// those above past the top of the page if need be, where the gutter
    /// clips them. Without one, cards stack down from the top. Returns how
    /// far down the cards reach, in page coordinates.
    private func placeCards() -> CGFloat {
        let g = view.geometry
        struct Entry { var y: CGFloat; var order: Int; var h: CGFloat; var card: GutterCard; var focused: Bool }
        var entries: [Entry] = []
        let width = g.cardWidth
        func top(_ ci: Int) -> CGFloat { lineTop(ci) }
        for it in items where visible(it.thread) {
            entries.append(
                Entry(
                    y: top(it.start), order: it.start, h: it.card.height(forWidth: width), card: it.card,
                    focused: it.thread.id == active))
        }
        if let d = draft {
            entries.append(
                Entry(y: top(d.start), order: d.start, h: d.card.height(forWidth: width), card: d.card, focused: true))
        }
        entries.sort { ($0.y, $0.order) < ($1.y, $1.order) }
        let n = entries.count
        var ys = entries.map { $0.y }
        // Down from the focused card, or from the top without one.
        let a = entries.firstIndex { $0.focused }
        let first = a ?? 0
        if a == nil && n > 0 { ys[0] = max(ys[0], view.textContainerInset.height) }
        if first + 1 < n {
            for i in (first + 1)..<n { ys[i] = max(ys[i], ys[i - 1] + entries[i - 1].h + CommentLayer.cardGap) }
        }
        if first > 0 {
            for i in stride(from: first - 1, through: 0, by: -1) {
                ys[i] = min(ys[i], ys[i + 1] - CommentLayer.cardGap - entries[i].h)
            }
        }
        let bottom = n > 0 ? ys[n - 1] + entries[n - 1].h : 0
        let x = gutter.convert(NSPoint(x: g.gutterX, y: 0), from: page).x
        for (i, e) in entries.enumerated() {
            e.card.frame = NSRect(x: x, y: ys[i], width: width, height: e.h)
        }
        return gutter.convert(NSPoint(x: 0, y: max(bottom, 0)), to: page).y
    }
}
