import AppKit
import UniformTypeIdentifiers
import margin_ffi

/// Recent documents open through Margin's own windows (there are no
/// NSDocument subclasses).
final class MarginDocumentController: NSDocumentController {
    override func openDocument(
        withContentsOf url: URL, display displayDocument: Bool,
        completionHandler: @escaping (NSDocument?, Bool, Error?) -> Void
    ) {
        AppDelegate.shared.open(path: url.path)
        completionHandler(nil, false, nil)
    }
}

/// Restores document windows after a relaunch, by path.
final class DocumentRestoration: NSObject, NSWindowRestoration {
    static func restoreWindow(
        withIdentifier identifier: NSUserInterfaceItemIdentifier, state: NSCoder,
        completionHandler: @escaping (NSWindow?, Error?) -> Void
    ) {
        guard let path = state.decodeObject(of: NSString.self, forKey: DocumentWindow.restorationPathKey) as String?,
            FileManager.default.fileExists(atPath: path),
            let w = AppDelegate.shared.open(path: path, show: false)
        else {
            completionHandler(nil, NSError(domain: NSCocoaErrorDomain, code: NSFileNoSuchFileError))
            return
        }
        completionHandler(w.window, nil)
    }
}

/// The app, started by the app target's `main.swift`.
public enum MarginApp {
    /// Kept alive for the app's lifetime (`NSApplication.delegate` is weak).
    private static let delegate = AppDelegate()

    public static func main() {
        let app = NSApplication.shared
        app.delegate = delegate
        app.run()
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuItemValidation {
    static var shared: AppDelegate { NSApp.delegate as! AppDelegate }
    private(set) var windows: [DocumentWindow] = []
    private var shortcutsWindow: NSWindow?
    /// Windows still to ask about unsaved text while quitting.
    private var quitQueue: [DocumentWindow] = []

    func applicationWillFinishLaunching(_ notification: Notification) {
        _ = MarginDocumentController()
        Notifier.shared.start()
        NSApp.mainMenu = buildMainMenu()
    }

    func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
        true
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls where url.isFileURL {
            open(path: url.path)
        }
    }

    /// Launched (or its Dock icon clicked) with no windows: untitled
    /// documents left by a crash come back; with none, the Open panel.
    func applicationOpenUntitledFile(_ sender: NSApplication) -> Bool {
        if recoverDrafts() == 0 { showOpenPanel() }
        return true
    }

    /// Quitting keeps untitled documents (they reopen at the next launch)
    /// and asks, one window at a time, only about text whose save failed.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        quitQueue = windows.filter { $0.needsAskingBeforeQuit }
        if quitQueue.isEmpty { return .terminateNow }
        askNextBeforeQuit()
        return .terminateLater
    }

    private func askNextBeforeQuit() {
        guard let w = quitQueue.first else {
            NSApp.reply(toApplicationShouldTerminate: true)
            return
        }
        w.window?.makeKeyAndOrderFront(nil)
        w.askUnsaved { [weak self] proceed in
            guard let self else { return }
            if proceed {
                self.quitQueue.removeFirst()
                self.askNextBeforeQuit()
            } else {
                self.quitQueue = []
                NSApp.reply(toApplicationShouldTerminate: false)
            }
        }
    }

    // MARK: - Documents

    /// Opens `path` in its own window, or brings its window forward.
    @discardableResult
    func open(path: String, show: Bool = true) -> DocumentWindow? {
        do {
            let canonical = try canonicalPath(path: path)
            if let w = windows.first(where: { $0.path == canonical }) {
                if show { w.window?.makeKeyAndOrderFront(nil) }
                return w
            }
            let text = try readDocument(path: canonical)
            let w = DocumentWindow(path: canonical, text: text)
            w.onClose = { [weak self] closed in self?.windows.removeAll { $0 === closed } }
            if let last = windows.last?.window {
                w.window?.setFrameTopLeftPoint(
                    last.cascadeTopLeft(from: NSPoint(x: last.frame.minX, y: last.frame.maxY)))
            }
            windows.append(w)
            if show {
                w.showWindow(nil)
            }
            w.window?.makeFirstResponder(w.textView)
            if !w.isDraft {
                NSDocumentController.shared.noteNewRecentDocumentURL(URL(fileURLWithPath: canonical))
            }
            return w
        } catch {
            let alert = NSAlert()
            alert.messageText = "Could not open “\((path as NSString).lastPathComponent)”"
            alert.informativeText = error.localizedDescription
            alert.runModal()
            return nil
        }
    }

    /// A new untitled document with `text`, autosaved in the drafts folder
    /// until it is saved somewhere. `prepare` runs once its file exists
    /// (Duplicate copies comments there).
    @discardableResult
    func newDraft(text: String = "", show: Bool = true, prepare: ((String) throws -> Void)? = nil) -> DocumentWindow? {
        let dir = draftsDirectory()
        do {
            try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
            let f = DateFormatter()
            f.dateFormat = "yyyy-MM-dd HHmmss.SSS"
            let p = (dir as NSString).appendingPathComponent("Untitled \(f.string(from: Date())).md")
            try Data(text.utf8).write(to: URL(fileURLWithPath: p))
            try prepare?(p)
            return open(path: p, show: show)
        } catch {
            NSAlert(error: error).runModal()
            return nil
        }
    }

    @objc func marginNew(_ sender: Any?) {
        newDraft()
    }

    /// Reopens drafts left behind (e.g. by a crash). Returns how many.
    private func recoverDrafts() -> Int {
        let dir = draftsDirectory()
        guard let names = try? FileManager.default.contentsOfDirectory(atPath: dir) else { return 0 }
        var n = 0
        for name in names.sorted() where name.hasSuffix(".md") {
            let p = (dir as NSString).appendingPathComponent(name)
            let size = (try? FileManager.default.attributesOfItem(atPath: p))?[.size] as? Int ?? 0
            if size == 0 {
                try? FileManager.default.removeItem(atPath: p)
                continue
            }
            if open(path: p) != nil { n += 1 }
        }
        return n
    }

    @objc func marginOpen(_ sender: Any?) {
        showOpenPanel()
    }

    private func showOpenPanel() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.markdown, .plainText]
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        if let w = NSApp.keyWindow?.windowController as? DocumentWindow, !w.isDraft {
            panel.directoryURL = URL(fileURLWithPath: (w.path as NSString).deletingLastPathComponent)
        }
        panel.begin { [weak self] response in
            guard response == .OK else { return }
            for url in panel.urls { self?.open(path: url.path) }
        }
    }

    // MARK: - Preferences for every window

    @objc func marginLarger(_ sender: Any?) { zoom(1) }
    @objc func marginSmaller(_ sender: Any?) { zoom(-1) }
    @objc func marginActualSize(_ sender: Any?) { zoom(0) }

    /// Makes the text of every window a step larger (`step > 0`), smaller
    /// (`step < 0`) or its normal size (`0`).
    func zoom(_ step: Int) {
        let now = Prefs.zoom
        let z: Double
        switch step {
        case 0: z = 1
        case 1...: z = Prefs.zoomSteps.first { $0 > now + 0.01 } ?? now
        default: z = Prefs.zoomSteps.last { $0 < now - 0.01 } ?? now
        }
        if z == now { return }
        Prefs.zoom = z
        Theme.invalidateFonts()
        for w in windows {
            w.keepingCursorLineStill { w.textView.forceRestyle() }
        }
    }

    @objc func marginToggleReflow(_ sender: Any?) {
        Prefs.reflowsParagraphs.toggle()
        for w in windows {
            w.keepingCursorLineStill { w.textView.reflowsParagraphs = Prefs.reflowsParagraphs }
        }
    }

    @objc func marginShowShortcuts(_ sender: Any?) {
        if shortcutsWindow == nil {
            shortcutsWindow = makeShortcutsWindow(menu: NSApp.mainMenu!)
        }
        shortcutsWindow?.makeKeyAndOrderFront(nil)
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(marginToggleReflow(_:)):
            item.state = Prefs.reflowsParagraphs ? .on : .off
        case #selector(marginLarger(_:)):
            return Prefs.zoom < Prefs.zoomSteps.last! - 0.01
        case #selector(marginSmaller(_:)):
            return Prefs.zoom > Prefs.zoomSteps.first! + 0.01
        default:
            break
        }
        return true
    }
}

// MARK: - Menus

private func item(
    _ title: String, _ action: Selector?, _ key: String = "", _ mods: NSEvent.ModifierFlags = [.command], tag: Int = 0
) -> NSMenuItem {
    let i = NSMenuItem(title: title, action: action, keyEquivalent: key)
    i.keyEquivalentModifierMask = key.isEmpty ? [] : mods
    i.tag = tag
    return i
}

private func submenu(_ title: String, _ items: [NSMenuItem]) -> NSMenuItem {
    let m = NSMenu(title: title)
    for i in items { m.addItem(i) }
    let i = NSMenuItem(title: title, action: nil, keyEquivalent: "")
    i.submenu = m
    return i
}

private let down = String(UnicodeScalar(NSDownArrowFunctionKey)!)
private let up = String(UnicodeScalar(NSUpArrowFunctionKey)!)
private let ret = "\r"

func buildMainMenu() -> NSMenu {
    let main = NSMenu()
    let services = NSMenu(title: "Services")
    NSApp.servicesMenu = services
    let servicesItem = NSMenuItem(title: "Services", action: nil, keyEquivalent: "")
    servicesItem.submenu = services
    main.addItem(
        submenu(
            "Margin",
            [
                item("About Margin", #selector(NSApplication.orderFrontStandardAboutPanel(_:))),
                .separator(),
                servicesItem,
                .separator(),
                item("Hide Margin", #selector(NSApplication.hide(_:)), "h"),
                item("Hide Others", #selector(NSApplication.hideOtherApplications(_:)), "h", [.command, .option]),
                item("Show All", #selector(NSApplication.unhideAllApplications(_:))),
                .separator(),
                item("Quit Margin", #selector(NSApplication.terminate(_:)), "q"),
            ]))

    let recent = NSMenu(title: "Open Recent")
    recent.addItem(item("Clear Menu", #selector(NSDocumentController.clearRecentDocuments(_:))))
    let recentItem = NSMenuItem(title: "Open Recent", action: nil, keyEquivalent: "")
    recentItem.submenu = recent
    main.addItem(
        submenu(
            "File",
            [
                item("New", #selector(AppDelegate.marginNew(_:)), "n"),
                item("Open…", #selector(AppDelegate.marginOpen(_:)), "o"),
                recentItem,
                .separator(),
                item("Close", #selector(NSWindow.performClose(_:)), "w"),
                item("Save", #selector(DocumentWindow.saveDocument(_:)), "s"),
                item("Save As…", #selector(DocumentWindow.saveDocumentAs(_:)), "s", [.command, .shift]),
                item("Duplicate", #selector(DocumentWindow.marginDuplicate(_:))),
                item("Rename…", #selector(DocumentWindow.marginRename(_:))),
                item("Move To…", #selector(DocumentWindow.marginMoveTo(_:))),
                item("Revert to Last Opened", #selector(DocumentWindow.marginRevertToLastOpened(_:))),
                .separator(),
                item("Page Setup…", #selector(NSApplication.runPageLayout(_:)), "p", [.command, .shift]),
                item("Print…", #selector(DocumentWindow.printDocument(_:)), "p"),
            ]))

    main.addItem(
        submenu(
            "Edit",
            [
                item("Undo", Selector(("undo:")), "z"),
                item("Redo", Selector(("redo:")), "z", [.command, .shift]),
                .separator(),
                item("Cut", #selector(NSText.cut(_:)), "x"),
                item("Copy", #selector(NSText.copy(_:)), "c"),
                item("Paste", #selector(NSText.paste(_:)), "v"),
                item("Select All", #selector(NSText.selectAll(_:)), "a"),
                .separator(),
                submenu(
                    "Find",
                    [
                        item("Find…", #selector(DocumentWindow.marginFind(_:)), "f"),
                        item(
                            "Find and Replace…", #selector(DocumentWindow.marginFindAndReplace(_:)), "f",
                            [.command, .option]),
                        item("Find Next", #selector(DocumentWindow.marginFindNext(_:)), "g"),
                        item(
                            "Find Previous", #selector(DocumentWindow.marginFindPrevious(_:)), "g", [.command, .shift]),
                        item("Use Selection for Find", #selector(DocumentWindow.marginUseSelectionForFind(_:)), "e"),
                    ]),
                submenu(
                    "Spelling",
                    [
                        item("Show Spelling and Grammar", #selector(NSText.showGuessPanel(_:)), ":"),
                        item("Check Document Now", #selector(NSText.checkSpelling(_:)), ";"),
                        item("Check Spelling While Typing", #selector(NSTextView.toggleContinuousSpellChecking(_:))),
                    ]),
            ]))

    var headings = [item("Normal Text", #selector(DocTextView.marginNormalText(_:)), "0", [.command, .option])]
    for n in 1...6 {
        headings.append(
            item("Heading \(n)", #selector(DocTextView.marginHeading(_:)), "\(n)", [.command, .option], tag: n))
    }
    main.addItem(
        submenu(
            "Format",
            headings + [
                .separator(),
                // Matched by physical key in DocTextView; shown here.
                item("Bulleted List", #selector(DocTextView.marginBulletedList(_:)), "8", [.command, .shift]),
                item("Numbered List", #selector(DocTextView.marginNumberedList(_:)), "7", [.command, .shift]),
                item("Checklist", #selector(DocTextView.marginChecklist(_:)), "9", [.command, .shift]),
                item("Quote", #selector(DocTextView.marginQuote(_:)), "q", [.command, .option]),
                item("Code Block", #selector(DocTextView.marginCodeBlock(_:)), "c", [.command, .option]),
                .separator(),
                item("Bold", #selector(DocTextView.marginBold(_:)), "b"),
                item("Italic", #selector(DocTextView.marginItalic(_:)), "i"),
                item("Strikethrough", #selector(DocTextView.marginStrikethrough(_:)), "x", [.command, .shift]),
                item("Inline Code", #selector(DocTextView.marginInlineCode(_:)), "e", [.command, .shift]),
                item("Link…", #selector(DocTextView.marginLink(_:)), "k"),
                .separator(),
                item("Indent", #selector(DocTextView.marginIndent(_:)), "]"),
                item("Outdent", #selector(DocTextView.marginOutdent(_:)), "["),
                item("Toggle Task", #selector(DocTextView.marginToggleTask(_:)), ret),
                item("Open Link", #selector(DocTextView.marginOpenLink(_:)), ret, [.command, .option]),
            ]))

    main.addItem(
        submenu(
            "Comments",
            [
                item(
                    "Comment on Selection", #selector(DocumentWindow.marginCommentOnSelection(_:)), "m",
                    [.command, .option]),
                item("Reply", #selector(DocumentWindow.marginReply(_:)), "r", [.command, .option]),
                item("Next Comment", #selector(DocumentWindow.marginNextComment(_:)), down, [.command, .option]),
                item("Previous Comment", #selector(DocumentWindow.marginPreviousComment(_:)), up, [.command, .option]),
                .separator(),
                item("Resolve", #selector(DocumentWindow.marginResolveComment(_:))),
                item("Edit", #selector(DocumentWindow.marginEditComment(_:))),
                item("Delete", #selector(DocumentWindow.marginDeleteComment(_:))),
                .separator(),
                item(
                    "Copy Open Comments", #selector(DocumentWindow.marginCopyOpenComments(_:)), "c", [.command, .shift]),
                item("Send to Agent", #selector(DocumentWindow.marginSendToAgent(_:)), ret, [.command, .shift]),
                item("Resolve All", #selector(DocumentWindow.marginResolveAll(_:))),
                item("Show Resolved", #selector(DocumentWindow.marginToggleShowResolved(_:))),
            ]))

    let largerAlt = item("Zoom In", #selector(AppDelegate.marginLarger(_:)), "=")
    largerAlt.isHidden = true
    largerAlt.allowsKeyEquivalentWhenHidden = true
    main.addItem(
        submenu(
            "View",
            [
                item("Show Markdown", #selector(DocumentWindow.marginToggleShowMarkdown(_:)), "/"),
                item("Reflow Paragraphs", #selector(AppDelegate.marginToggleReflow(_:)), "z", [.command, .option]),
                .separator(),
                item(
                    "Next Change", #selector(DocumentWindow.marginNextChange(_:)), down, [.command, .option, .shift]),
                item(
                    "Previous Change", #selector(DocumentWindow.marginPreviousChange(_:)), up,
                    [.command, .option, .shift]),
                .separator(),
                item("Zoom In", #selector(AppDelegate.marginLarger(_:)), "+"),
                largerAlt,
                item("Zoom Out", #selector(AppDelegate.marginSmaller(_:)), "-"),
                item("Actual Size", #selector(AppDelegate.marginActualSize(_:)), "0"),
                .separator(),
                item("Enter Full Screen", #selector(NSWindow.toggleFullScreen(_:)), "f", [.command, .control]),
            ]))

    let window = submenu(
        "Window",
        [
            item("Minimize", #selector(NSWindow.performMiniaturize(_:)), "m"),
            item("Zoom", #selector(NSWindow.performZoom(_:))),
            .separator(),
            item("Bring All to Front", #selector(NSApplication.arrangeInFront(_:))),
        ])
    NSApp.windowsMenu = window.submenu
    main.addItem(window)

    let help = submenu(
        "Help",
        [
            item("Keyboard Shortcuts", #selector(AppDelegate.marginShowShortcuts(_:)))
        ])
    NSApp.helpMenu = help.submenu
    main.addItem(help)
    return main
}

/// Every shortcut, read from the menus so the list never goes stale, plus
/// the keys that aren't menu commands.
private func makeShortcutsWindow(menu: NSMenu) -> NSWindow {
    func keys(_ i: NSMenuItem) -> String {
        var s = ""
        let m = i.keyEquivalentModifierMask
        if m.contains(.control) { s += "⌃" }
        if m.contains(.option) { s += "⌥" }
        if m.contains(.shift) { s += "⇧" }
        if m.contains(.command) { s += "⌘" }
        switch i.keyEquivalent {
        case "\r": s += "↩"
        case down: s += "↓"
        case up: s += "↑"
        default: s += i.keyEquivalent.uppercased()
        }
        return s
    }
    let out = NSMutableAttributedString()
    let head: [NSAttributedString.Key: Any] = [
        .font: NSFont.boldSystemFont(ofSize: 13), .foregroundColor: NSColor.labelColor,
    ]
    let body: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: 13), .foregroundColor: NSColor.labelColor,
    ]
    let para = NSMutableParagraphStyle()
    para.tabStops = [NSTextTab(textAlignment: .left, location: 230)]
    func section(_ title: String, _ rows: [(String, String)]) {
        guard !rows.isEmpty else { return }
        out.append(NSAttributedString(string: (out.length > 0 ? "\n" : "") + title + "\n", attributes: head))
        for (t, k) in rows {
            var a = body
            a[.paragraphStyle] = para
            out.append(NSAttributedString(string: "\(t)\t\(k)\n", attributes: a))
        }
    }
    func collect(_ m: NSMenu) -> [(String, String)] {
        var rows: [(String, String)] = []
        for i in m.items where !i.isHidden {
            if let sub = i.submenu {
                rows += collect(sub)
            } else if !i.keyEquivalent.isEmpty {
                rows.append((i.title.replacingOccurrences(of: "…", with: ""), keys(i)))
            }
        }
        return rows
    }
    for top in menu.items {
        guard let sub = top.submenu, top.title != "Margin", top.title != "Window" else { continue }
        section(top.title, collect(sub))
    }
    section(
        "Editing",
        [
            ("Line break in paragraph", "⇧↩"),
            ("Indent, outdent list item", "⇥, ⇧⇥"),
            ("Open link under the pointer", "⌘-click"),
            ("Leave comment, close Find", "Esc"),
            ("Post comment or reply", "⌘↩"),
        ])
    let tv = NSTextView(frame: NSRect(x: 0, y: 0, width: 420, height: 600))
    tv.isEditable = false
    tv.textContainerInset = NSSize(width: 16, height: 16)
    tv.textStorage?.setAttributedString(out)
    let scroll = NSScrollView(frame: tv.frame)
    scroll.documentView = tv
    scroll.hasVerticalScroller = true
    tv.autoresizingMask = [.width]
    let w = NSPanel(
        contentRect: NSRect(x: 0, y: 0, width: 420, height: 600),
        styleMask: [.titled, .closable, .resizable, .utilityWindow], backing: .buffered, defer: false)
    w.title = "Keyboard Shortcuts"
    w.contentView = scroll
    w.isReleasedWhenClosed = false
    w.center()
    return w
}
