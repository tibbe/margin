import AppKit
import UniformTypeIdentifiers
import margin_ffi

extension UTType {
    static let markdown = UTType("net.daringfireball.markdown") ?? UTType(filenameExtension: "md") ?? .plainText
}

extension Timer {
    /// A timer whose `body` runs on the main actor. It's scheduled on the
    /// current run loop, which on the main actor is the main one, so it
    /// fires on the main thread.
    static func onMain(
        after seconds: TimeInterval, repeats: Bool = false, _ body: @escaping @MainActor () -> Void
    ) -> Timer {
        scheduledTimer(withTimeInterval: seconds, repeats: repeats) { _ in MainActor.assumeIsolated(body) }
    }
}

/// Where untitled documents live until they are saved somewhere.
func draftsDirectory() -> String {
    (dataDir() as NSString).appendingPathComponent("drafts")
}

/// Whether a (canonical) document path is an untitled draft's.
func isDraftPath(_ path: String) -> Bool {
    let dir = (try? canonicalPath(path: draftsDirectory())) ?? draftsDirectory()
    return path.hasPrefix(dir + "/")
}

/// A document window: loading and autosaving the file, following changes
/// other programs (agents) make to it and to its comments, and the
/// window's commands.
final class DocumentWindow: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSMenuItemValidation {
    private(set) var path: String
    private(set) var isDraft: Bool
    let textView = DocTextView.make()
    private(set) lazy var page = PageView(textView: textView)
    let scrollView = NSScrollView()
    private(set) var layer: CommentLayer!
    private(set) var findBar: FindBar!
    let banner = Banner()
    let countLabel = NSTextField(labelWithString: "")
    /// Agents waiting on the document, or working on what was sent.
    private var agents: DocAgents?
    private(set) var agentState = AgentState.none
    private var agentTimer: Timer?
    private(set) var sendItem: NSToolbarItem?
    /// The text and its file: what to save, and outside edits to take in.
    private let sync: FileSync
    private var saveTimer: Timer?
    private var diskTimer: Timer?
    private var storeTimer: Timer?
    private var fileWatcher: FileWatcher?
    private var storeWatcher: FileWatcher?
    /// The text when the window opened, for Revert to Last Opened.
    private var openedText = ""
    /// Sudden termination is off, as a save is due (see `holdTermination`).
    private var holdsTermination = false
    /// The comment store's file as last read, to skip reloads for other
    /// documents' stores.
    private var storeStamp: (Date, Int)?
    /// The person confirmed closing without saving.
    private var closing = false
    var onClose: ((DocumentWindow) -> Void)?

    static let restorationPathKey = "path"

    init(path: String, text: LoadedText) {
        self.path = path
        isDraft = isDraftPath(path)
        sync = FileSync(opened: text)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1240, height: 860),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered, defer: false)
        window.minSize = NSSize(width: 520, height: 320)
        window.toolbarStyle = .unified
        window.titlebarSeparatorStyle = .automatic
        // The controller owns the window.
        window.isReleasedWhenClosed = false
        window.identifier = NSUserInterfaceItemIdentifier("document")
        window.restorationClass = DocumentRestoration.self
        window.tabbingIdentifier = "document"
        super.init(window: window)
        window.delegate = self
        openedText = text.text
        buildContent()
        textView.reflowsParagraphs = Prefs.reflowsParagraphs
        textView.setContents(text.text)
        layer.attach(try? CommentStore(document: path))
        watch()
        updateTitle()
        window.center()
    }

    required init?(coder: NSCoder) { fatalError() }

    private func buildContent() {
        guard let window else { return }
        layer = CommentLayer(page: page)
        findBar = FindBar(view: textView)
        scrollView.documentView = page
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.drawsBackground = true
        scrollView.backgroundColor = .textBackgroundColor
        scrollView.translatesAutoresizingMaskIntoConstraints = false
        scrollView.contentView.postsBoundsChangedNotifications = true
        scrollView.findBarPosition = .aboveContent
        scrollView.findBarView = findBar
        let content = NSView()
        content.addSubview(scrollView)
        content.addSubview(banner)
        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: content.safeAreaLayoutGuide.topAnchor),
            scrollView.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            banner.centerXAnchor.constraint(equalTo: content.centerXAnchor),
            banner.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -20),
            banner.widthAnchor.constraint(lessThanOrEqualTo: content.widthAnchor, constant: -40),
        ])
        window.contentView = content

        let toolbar = NSToolbar(identifier: "document")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false
        window.toolbar = toolbar

        textView.onRetile = { [weak self] in
            self?.page.retile()
            self?.page.layoutSubtreeIfNeeded()
        }
        textView.onChange = { [weak self] in self?.textChanged() }
        textView.onEdit = { [weak self] r, d in self?.layer.textEdited(range: r, delta: d) }
        textView.onSelectionChange = { [weak self] in self?.layer.cursorMoved() }
        textView.onLayout = { [weak self] in self?.layer.queueRelayout() }
        textView.onHighlight = { [weak self] in self?.layer.refreshHighlights() }
        textView.onOpenLink = { [weak self] url in self?.follow(link: url) }
        textView.onEscape = { [weak self] in
            guard let self else { return }
            if !self.layer.escape() && self.findBar.isOpen { self.findBar.close() }
        }
        layer.onChange = { [weak self] in self?.updateTitle() }
        layer.toast = { [weak self] text, undo in self?.banner.show(text, undo: undo) }
        layer.onActivity = { [weak self] activity in
            guard let self else { return }
            self.agents?.activity(nowMs: DocumentWindow.nowMs())
            Notifier.shared.post(activity, in: self)
        }
        layer.beforeAdd = { [weak self] in self?.save() }
        layer.extraHighlights = { [weak self] in self?.findBar.highlights() ?? [] }
        findBar.onChange = { [weak self] in
            guard let self else { return }
            if self.scrollView.isFindBarVisible != self.findBar.isOpen || self.findBar.isOpen {
                self.findBar.setFrameSize(
                    NSSize(width: self.scrollView.frame.width, height: self.findBar.fittingSize.height))
                self.scrollView.isFindBarVisible = self.findBar.isOpen
                self.scrollView.tile()
            }
            self.layer.refreshHighlights()
        }
        followAgents()
    }

    // MARK: - Toolbar

    private static let countItem = NSToolbarItem.Identifier("count")
    private static let commentItem = NSToolbarItem.Identifier("comment")
    private static let sendItem = NSToolbarItem.Identifier("send")

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.flexibleSpace, DocumentWindow.countItem, DocumentWindow.sendItem, DocumentWindow.commentItem]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    func toolbar(
        _ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool
    ) -> NSToolbarItem? {
        switch id {
        case DocumentWindow.countItem:
            let item = NSToolbarItem(itemIdentifier: id)
            countLabel.textColor = .secondaryLabelColor
            item.view = countLabel
            item.label = "Comments"
            return item
        case DocumentWindow.commentItem:
            let item = NSToolbarItem(itemIdentifier: id)
            item.image = NSImage(systemSymbolName: "plus.bubble", accessibilityDescription: "Comment on Selection")
            item.label = "Comment"
            item.toolTip = "Comment on Selection (⌥⌘M)"
            item.isBordered = true
            item.target = self
            item.action = #selector(marginCommentOnSelection(_:))
            return item
        case DocumentWindow.sendItem:
            let item = NSToolbarItem(itemIdentifier: id)
            item.image = NSImage(systemSymbolName: "paperplane", accessibilityDescription: "Send to Agent")
            item.label = "Send to Agent"
            item.isBordered = true
            item.autovalidates = false
            item.target = self
            item.action = #selector(marginSendToAgent(_:))
            sendItem = item
            showAgent()
            return item
        default:
            return nil
        }
    }

    // MARK: - Title

    var displayName: String {
        isDraft ? "Untitled" : (path as NSString).lastPathComponent
    }

    private func homeRelative(_ p: String) -> String {
        let home = NSHomeDirectory()
        return p.hasPrefix(home + "/") ? "~" + p.dropFirst(home.count) : (p == home ? "~" : p)
    }

    func updateTitle() {
        guard let window else { return }
        window.title = displayName
        window.subtitle = isDraft ? "Not saved yet" : homeRelative((path as NSString).deletingLastPathComponent)
        window.representedURL = isDraft ? nil : URL(fileURLWithPath: path)
        // Documents save themselves: they are only "edited" when that
        // failed, or while untitled.
        window.isDocumentEdited = isDraft ? !textView.string.isEmpty : sync.saveFailed()
        showAgent()
    }

    // MARK: - Agents

    /// Milliseconds on a clock that only goes forward.
    static func nowMs() -> Int64 { Int64(ProcessInfo.processInfo.systemUptime * 1000) }

    /// Follows the agents on the document, which drafts have none of.
    private func followAgents() {
        agents = isDraft ? nil : DocAgents(document: path)
        if agentTimer == nil {
            agentTimer = Timer.onMain(after: 1, repeats: true) { [weak self] in self?.updateAgent() }
        }
        updateAgent()
    }

    private var canSend: Bool { agentState == .waiting && layer.openCount > 0 }

    /// Looks at the document's agents again, and shows what they are doing.
    func updateAgent() {
        agentState = agents?.poll(nowMs: DocumentWindow.nowMs()) ?? .none
        showAgent()
    }

    /// The comment count, and what the agent is doing, in the toolbar.
    private func showAgent() {
        let open = layer.openCount
        let resolved = layer.resolvedCount
        var parts = [
            open == 0
                ? (resolved == 0 ? "" : "\(resolved) resolved")
                : (open == 1 ? "1 open comment" : "\(open) open comments")
        ]
        if agentState == .working { parts.append("Agent working") }
        countLabel.stringValue = parts.filter { !$0.isEmpty }.joined(separator: " · ")
        countLabel.sizeToFit()
        guard let item = sendItem else { return }
        item.isEnabled = canSend
        switch agentState {
        case .waiting:
            item.toolTip =
                layer.openCount > 0
                ? "Send Open Comments to the Agent (⇧⌘↩)"
                : "An agent is waiting, but there are no open comments to send."
        case .working:
            item.toolTip = "The agent is working on the comments you sent."
        case .none:
            item.toolTip = "No agent is waiting on this document. Ask your agent to run “margin wait” on it."
        }
    }

    // MARK: - Saving

    private func textChanged() {
        sync.edited()
        updateTitle()
        scheduleSave()
        if findBar.isOpen { findBar.refresh(goingToMatchAtOrAfterCursor: false) }
    }

    /// Keeps the system from killing the app unasked while `hold`: while a
    /// save is due.
    private func holdTermination(_ hold: Bool) {
        if hold == holdsTermination { return }
        holdsTermination = hold
        if hold {
            ProcessInfo.processInfo.disableSuddenTermination()
        } else {
            ProcessInfo.processInfo.enableSuddenTermination()
        }
    }

    private func scheduleSave() {
        holdTermination(sync.needsSave())
        saveTimer?.invalidate()
        saveTimer = Timer.onMain(after: 0.7) { [weak self] in self?.save() }
    }

    /// Replaces the file's contents with `text` (in the file's newlines),
    /// coordinated with other file presenters (sync clients, other
    /// editors). Replacing the item keeps its metadata: permissions, Finder
    /// tags, extended attributes.
    private func write(_ text: String, to path: String) throws {
        let data = Data(text.utf8)
        let url = URL(fileURLWithPath: path)
        let fm = FileManager.default
        var coordinationError: NSError?
        var writeError: Error?
        NSFileCoordinator(filePresenter: nil).coordinate(
            writingItemAt: url, options: .forReplacing, error: &coordinationError
        ) { url in
            do {
                guard fm.fileExists(atPath: url.path) else {
                    try data.write(to: url, options: .atomic)
                    return
                }
                let dir = try fm.url(
                    for: .itemReplacementDirectory, in: .userDomainMask, appropriateFor: url, create: true)
                defer { try? fm.removeItem(at: dir) }
                let temp = dir.appendingPathComponent(url.lastPathComponent)
                try data.write(to: temp)
                _ = try fm.replaceItemAt(url, withItemAt: temp)
            } catch {
                writeError = error
            }
        }
        if let e = coordinationError ?? writeError { throw e }
    }

    /// Writes the document if it changed, then records comment anchors
    /// against the saved text.
    @discardableResult
    func save() -> Bool {
        saveTimer?.invalidate()
        saveTimer = nil
        defer {
            holdTermination(sync.needsSave())
            updateTitle()
        }
        // The file is read first: the file watcher may not have delivered
        // an outside edit yet. One retry lets taking it in finish saving
        // during this call.
        for _ in 0..<2 {
            let text = textView.string
            do {
                switch try sync.save(ours: text, path: path) {
                case .done:
                    return true
                case .blocked:
                    return false
                case .changed(let change):
                    if !takeIn(change) { return false }
                case .write(let out):
                    try write(out, to: path)
                    sync.wrote(ours: text)
                    layer.persistAnchors()
                    return true
                }
            } catch {
                sync.failed()
                banner.show("Could not save: \(error.localizedDescription)", undo: nil)
                return false
            }
        }
        return false
    }

    // MARK: - Following outside changes

    private func watch() {
        fileWatcher = FileWatcher(path: path) { [weak self] in
            self?.debounce(\.diskTimer, 0.12) { $0.checkDisk() }
        }
        if let store = layer.store {
            let p = store.path()
            try? FileManager.default.createDirectory(
                atPath: (p as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
            storeStamp = stamp(p)
            // The folder holds every document's store: reload only when
            // this one changed.
            storeWatcher = FileWatcher(path: p) { [weak self] in
                self?.debounce(\.storeTimer, 0.08) { w in
                    let now = w.stamp(p)
                    if now.map({ [$0.0.timeIntervalSince1970, Double($0.1)] })
                        != w.storeStamp.map({ [$0.0.timeIntervalSince1970, Double($0.1)] })
                    {
                        w.storeStamp = now
                        w.layer.reload()
                    }
                }
            }
        }
    }

    private func stamp(_ p: String) -> (Date, Int)? {
        guard let a = try? FileManager.default.attributesOfItem(atPath: p),
            let d = a[.modificationDate] as? Date, let n = a[.size] as? Int
        else { return nil }
        return (d, n)
    }

    private func debounce(
        _ slot: ReferenceWritableKeyPath<DocumentWindow, Timer?>, _ seconds: TimeInterval,
        _ f: @escaping (DocumentWindow) -> Void
    ) {
        self[keyPath: slot]?.invalidate()
        self[keyPath: slot] = Timer.onMain(after: seconds) { [weak self] in
            guard let self else { return }
            self[keyPath: slot] = nil
            f(self)
        }
    }

    /// Picks up edits made to the file by someone else.
    private func checkDisk() {
        guard let change = try? sync.diskChanged(ours: textView.string, path: path) else { return }
        takeIn(change)
        holdTermination(sync.needsSave())
    }

    /// Shows what taking in a change to the file takes; false for a
    /// conflict, which waits for the person's answer.
    @discardableResult
    private func takeIn(_ change: FileChange) -> Bool {
        switch change {
        case .caughtUp:
            updateTitle()
        case .load(let text):
            textView.applyExternal(text)
            afterExternalChange()
            banner.show("Updated from disk", undo: nil)
        case .merge(let text):
            textView.applyExternal(text)
            afterExternalChange()
            scheduleSave()
            banner.show("Merged changes from disk", undo: nil)
        case .conflict:
            askConflict()
            return false
        }
        return true
    }

    private func afterExternalChange() {
        updateTitle()
        layer.persistAnchors()
        layer.queueRelayout()
    }

    private func askConflict() {
        guard let window, sync.inConflict() else { return }
        if window.attachedSheet != nil {
            // Asked once the sheet showing now (Rename, Save As…) is gone.
            NotificationCenter.default.addObserver(
                self, selector: #selector(sheetEnded), name: NSWindow.didEndSheetNotification, object: window)
            return
        }
        let alert = NSAlert()
        alert.messageText = "Document Changed on Disk"
        alert.informativeText =
            "\(displayName) was changed by another program while you had unsaved edits, and the changes overlap."
        alert.addButton(withTitle: "Keep My Version")
        let load = alert.addButton(withTitle: "Load Disk Version")
        load.hasDestructiveAction = true
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self else { return }
            if response == .alertSecondButtonReturn {
                guard let theirs = self.sync.loadTheirs() else { return }
                self.textView.applyExternal(theirs)
                self.afterExternalChange()
                self.checkDisk()
            } else if self.sync.keepMine() {
                self.save()
            }
        }
    }

    @objc private func sheetEnded(_ note: Notification) {
        NotificationCenter.default.removeObserver(self, name: NSWindow.didEndSheetNotification, object: window)
        // After the sheet is gone.
        DispatchQueue.main.async { [weak self] in self?.askConflict() }
    }

    // MARK: - Links

    /// Opens a link: Markdown files in Margin, other files in their
    /// default app, anything else (web, mail) with the system.
    private func follow(link: String) {
        let base = URL(fileURLWithPath: path)
        guard !link.hasPrefix("#"),
            let url = URL(string: link, relativeTo: base)?.absoluteURL
                ?? URL(
                    string: link.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? "", relativeTo: base)?
                .absoluteURL
        else { return }
        guard url.isFileURL else {
            NSWorkspace.shared.open(url)
            return
        }
        let ext = url.pathExtension.lowercased()
        if ext == "md" || ext == "markdown" {
            AppDelegate.shared.open(path: url.path)
        } else {
            NSWorkspace.shared.open(url)
        }
    }

    // MARK: - Save As, Rename, Move To, Duplicate, Revert

    @objc func saveDocument(_ sender: Any?) {
        if isDraft { saveDocumentAs(sender) } else { save() }
    }

    @objc func saveDocumentAs(_ sender: Any?) {
        askForPath(title: nil) { [weak self] path in
            guard let self else { return }
            guard let path else {
                // Cancelled: closing waits for another answer.
                self.closing = false
                self.pendingClose?(false)
                return
            }
            do {
                try self.relocate(to: path, moving: false)
                if self.closing { self.window?.close() }
                self.pendingClose?(true)
            } catch {
                self.closing = false
                self.pendingClose?(false)
                self.banner.show("Could not save: \(error.localizedDescription)", undo: nil)
            }
        }
    }

    /// Rename: the file, and its comments, get a new name in the same folder.
    @objc func marginRename(_ sender: Any?) {
        guard !isDraft, let window, window.attachedSheet == nil else { return saveDocumentAs(sender) }
        let alert = NSAlert()
        alert.messageText = "Rename “\(displayName)”"
        let field = NSTextField(string: displayName)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        alert.accessoryView = field
        alert.addButton(withTitle: "Rename")
        alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .alertFirstButtonReturn else { return }
            let name = field.stringValue.trimmingCharacters(in: .whitespaces)
            guard !name.isEmpty, name != self.displayName else { return }
            let dir = (self.path as NSString).deletingLastPathComponent
            self.relocateReporting((dir as NSString).appendingPathComponent(name), moving: true)
        }
        // Select the name without the extension, as Finder does.
        let stem = (displayName as NSString).deletingPathExtension as NSString
        field.currentEditor()?.selectedRange = NSRange(location: 0, length: stem.length)
    }

    /// Move To: the file, and its comments, go to another folder.
    @objc func marginMoveTo(_ sender: Any?) {
        guard !isDraft, let window else { return saveDocumentAs(sender) }
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.prompt = "Move"
        panel.message = "Move “\(displayName)” to:"
        panel.directoryURL = URL(fileURLWithPath: (path as NSString).deletingLastPathComponent)
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .OK, let dir = panel.url else { return }
            self.relocateReporting(dir.appendingPathComponent(self.displayName).path, moving: true)
        }
    }

    /// Duplicate: an untitled copy, with the comments.
    @objc func marginDuplicate(_ sender: Any?) {
        save()
        let text = textView.string
        let source = path
        AppDelegate.shared.newDraft(text: text) { draft in
            let store = try CommentStore(document: source)
            // A second handle, so this window's store stays put.
            try store.copyTo(newDocument: draft, text: text)
        }
    }

    /// Revert to Last Opened: the text as it was when the window opened,
    /// as one undoable step.
    @objc func marginRevertToLastOpened(_ sender: Any?) {
        guard textView.string != openedText else { return }
        textView.applyExternal(openedText, actionName: "Revert")
        scheduleSave()
    }

    /// "+" in the tab bar: a new untitled document in a tab.
    @objc override func newWindowForTab(_ sender: Any?) {
        guard let w = AppDelegate.shared.newDraft(show: false)?.window else { return }
        window?.addTabbedWindow(w, ordered: .above)
        w.makeKeyAndOrderFront(nil)
    }

    private func askForPath(title: String?, then: @escaping (String?) -> Void) {
        guard let window else { return then(nil) }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.markdown]
        panel.allowsOtherFileTypes = true
        panel.isExtensionHidden = false
        panel.canCreateDirectories = true
        if isDraft {
            panel.nameFieldStringValue = "Untitled.md"
        } else {
            panel.directoryURL = URL(fileURLWithPath: (path as NSString).deletingLastPathComponent)
            panel.nameFieldStringValue = (path as NSString).lastPathComponent
        }
        panel.beginSheetModal(for: window) { response in
            then(response == .OK ? panel.url?.path : nil)
        }
    }

    private func relocateReporting(_ path: String, moving: Bool) {
        do {
            try relocate(to: path, moving: moving)
        } catch {
            banner.show("Could not \(moving ? "move" : "save"): \(error.localizedDescription)", undo: nil)
        }
    }

    /// Continues the document at a new path, with its comments. Save As
    /// (`moving` false) writes a new file and leaves the old one; Rename
    /// and Move To move the file itself, keeping its metadata. A draft's
    /// own file is removed either way.
    func relocate(to newPath: String, moving: Bool) throws {
        guard save() else {
            throw NSError(
                domain: "Margin", code: 1, userInfo: [NSLocalizedDescriptionKey: "Could not save the document first"])
        }
        let target = (newPath as NSString).pathExtension.isEmpty ? newPath + ".md" : newPath
        if try canonicalPath(path: target) == path { return }
        let text = textView.string
        let wasDraft = isDraft
        let old = path
        let fm = FileManager.default
        if moving && !wasDraft {
            if fm.fileExists(atPath: target) {
                throw NSError(
                    domain: NSCocoaErrorDomain, code: NSFileWriteFileExistsError,
                    userInfo: [NSLocalizedDescriptionKey: "“\((target as NSString).lastPathComponent)” already exists."]
                )
            }
            var coordinationError: NSError?
            var moveError: Error?
            NSFileCoordinator(filePresenter: nil).coordinate(
                writingItemAt: URL(fileURLWithPath: old), options: .forMoving,
                writingItemAt: URL(fileURLWithPath: target), options: .forReplacing,
                error: &coordinationError
            ) { from, to in
                do { try fm.moveItem(at: from, to: to) } catch { moveError = error }
            }
            if let e = coordinationError ?? moveError { throw e }
        } else {
            try write(sync.fileText(ours: text), to: target)
        }
        let canonical = try canonicalPath(path: target)
        try layer.store?.moveTo(newDocument: canonical, text: text, removeOld: wasDraft || moving)
        if wasDraft { try? fm.removeItem(atPath: old) }
        fileWatcher?.cancel()
        storeWatcher?.cancel()
        path = canonical
        isDraft = false
        sync.wrote(ours: text)
        layer.attach(try? CommentStore(document: canonical))
        watch()
        followAgents()
        updateTitle()
        window?.invalidateRestorableState()
        NSDocumentController.shared.noteNewRecentDocumentURL(URL(fileURLWithPath: canonical))
    }

    // MARK: - Closing and restoration

    private func discardDraft() {
        try? layer.store?.remove()
        try? FileManager.default.removeItem(atPath: path)
        // Nothing left to save.
        sync.wrote(ours: textView.string)
    }

    /// Whether closing now would lose text that is in no file.
    var needsAskingBeforeClose: Bool {
        if closing { return false }
        if isDraft { return !textView.string.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        return !save()
    }

    /// Whether quitting would lose text: only a save that fails. Untitled
    /// documents are kept and reopen at the next launch.
    var needsAskingBeforeQuit: Bool {
        !closing && !isDraft && !save()
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if closing { return true }
        if isDraft && textView.string.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            discardDraft()
            return true
        }
        if needsAskingBeforeClose {
            askUnsaved()
            return false
        }
        return true
    }

    /// Answers a quit waiting on this window: true to go on quitting.
    private var pendingClose: ((Bool) -> Void)?

    /// Asks what to do with text that is in no file yet: an untitled
    /// document, or one whose own file could not be written. `done` hears
    /// whether the window may go (quitting waits on it).
    func askUnsaved(done: ((Bool) -> Void)? = nil) {
        guard let window, window.attachedSheet == nil else { done?(false); return }
        let alert = NSAlert()
        alert.messageText = "Do you want to save the changes made to “\(displayName)”?"
        alert.informativeText = "Your changes will be lost if you don’t save them."
        alert.addButton(withTitle: "Save…")
        alert.addButton(withTitle: "Cancel")
        let discard = alert.addButton(withTitle: "Don’t Save")
        discard.hasDestructiveAction = true
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self else { return }
            switch response {
            case .alertFirstButtonReturn:
                self.closing = done == nil
                self.pendingClose = { ok in
                    self.pendingClose = nil
                    done?(ok)
                }
                DispatchQueue.main.async { self.saveDocumentAs(nil) }
            case .alertThirdButtonReturn:
                if self.isDraft { self.discardDraft() }
                self.closing = true
                if done == nil { self.window?.close() }
                done?(true)
            default:
                done?(false)
            }
        }
    }

    func window(_ window: NSWindow, willEncodeRestorableState state: NSCoder) {
        state.encode(path as NSString, forKey: DocumentWindow.restorationPathKey)
    }

    func windowDidBecomeKey(_ notification: Notification) {
        Notifier.shared.clear(path: path)
    }

    func windowDidResignKey(_ notification: Notification) {
        save()
    }

    func windowWillClose(_ notification: Notification) {
        save()
        fileWatcher?.cancel()
        storeWatcher?.cancel()
        saveTimer?.invalidate()
        agentTimer?.invalidate()
        // Waiters stop once no window shows the document.
        agents = nil
        holdTermination(false)
        onClose?(self)
    }

    // MARK: - Commands

    @objc func marginFind(_ sender: Any?) { findBar.open(showingReplaceField: false) }
    @objc func marginFindAndReplace(_ sender: Any?) { findBar.open(showingReplaceField: true) }
    @objc func marginFindNext(_ sender: Any?) { findBar.step(forward: true) }
    @objc func marginFindPrevious(_ sender: Any?) { findBar.step(forward: false) }
    @objc func marginUseSelectionForFind(_ sender: Any?) { findBar.useSelection() }

    @objc func marginCommentOnSelection(_ sender: Any?) {
        if !layer.hasFocus { layer.beginDraft() }
    }

    @objc func marginReply(_ sender: Any?) { layer.focusReply() }
    @objc func marginResolveComment(_ sender: Any?) { layer.toggleResolvedFocused() }
    @objc func marginEditComment(_ sender: Any?) { layer.editFocused() }
    @objc func marginDeleteComment(_ sender: Any?) { if let a = layer.active { layer.delete(a) } }
    @objc func marginNextComment(_ sender: Any?) { layer.step(forward: true) }
    @objc func marginPreviousComment(_ sender: Any?) { layer.step(forward: false) }
    @objc func marginResolveAll(_ sender: Any?) { layer.resolveAll() }
    @objc func marginToggleShowResolved(_ sender: Any?) { layer.showsResolved.toggle() }

    @objc func marginCopyOpenComments(_ sender: Any?) {
        let threads = layer.openThreads()
        if threads.isEmpty {
            banner.show("No open comments", undo: nil)
            return
        }
        let text = commentsForAgent(document: path, text: textView.string, threads: threads)
        let pb = NSPasteboard.general
        pb.clearContents()
        pb.setString(text, forType: .string)
        banner.show(threads.count == 1 ? "Copied 1 open comment" : "Copied \(threads.count) open comments", undo: nil)
    }

    @objc func marginSendToAgent(_ sender: Any?) {
        updateAgent()
        guard canSend, let agents else {
            banner.show(layer.openCount == 0 ? "No open comments" : "No agent is waiting", undo: nil)
            return
        }
        save()
        do {
            let n = try agents.send(nowMs: DocumentWindow.nowMs())
            let open = layer.openCount
            let what = open == 1 ? "1 open comment" : "\(open) open comments"
            banner.show(
                n == 0 ? "No agent is waiting" : "Sent \(what) to \(n == 1 ? "the agent" : "\(n) agents")", undo: nil)
        } catch {
            banner.show("Could not send: \(error.localizedDescription)", undo: nil)
        }
        updateAgent()
    }

    @objc func marginToggleShowMarkdown(_ sender: Any?) {
        keepingCursorLineStill { textView.sourceMode.toggle() }
    }

    /// Runs `f`, which changes line heights, keeping the cursor's line
    /// where it is in the window when it is in view.
    func keepingCursorLineStill(_ f: () -> Void) {
        let clip = scrollView.contentView
        let before = textView.location(of: textView.cursor).minY - clip.bounds.minY
        let inView = before >= 0 && before <= clip.bounds.height
        f()
        textView.layoutManager?.ensureLayout(for: textView.textContainer!)
        if inView {
            let after = textView.location(of: textView.cursor).minY
            clip.scroll(to: NSPoint(x: 0, y: max(0, after - before)))
            scrollView.reflectScrolledClipView(clip)
        }
        layer.queueRelayout()
    }

    @objc func printDocument(_ sender: Any?) {
        guard let window else { return }
        printMarkdown(text: textView.string, title: displayName, window: window)
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(marginToggleShowResolved(_:)):
            item.state = layer.showsResolved ? .on : .off
        case #selector(marginToggleShowMarkdown(_:)):
            item.state = textView.sourceMode ? .on : .off
        case #selector(marginFindNext(_:)), #selector(marginFindPrevious(_:)):
            return !findBar.search.stringValue.isEmpty || findBar.isOpen
        case #selector(marginCommentOnSelection(_:)):
            return layer.store != nil && !layer.hasFocus
        case #selector(marginReply(_:)):
            return layer.active != nil
        case #selector(marginResolveComment(_:)):
            item.title = layer.focusedThread?.resolved == true ? "Reopen" : "Resolve"
            return layer.active != nil
        case #selector(marginEditComment(_:)), #selector(marginDeleteComment(_:)):
            return layer.active != nil
        case #selector(marginSendToAgent(_:)):
            updateAgent()
            return canSend
        case #selector(marginRevertToLastOpened(_:)):
            return textView.string != openedText
        case #selector(marginRename(_:)), #selector(marginMoveTo(_:)):
            return !isDraft
        default:
            break
        }
        return true
    }
}
