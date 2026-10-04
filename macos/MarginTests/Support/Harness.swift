import AppKit
import XCTest
import margin_ffi

@testable import MarginKit

/// The app delegate the tests share, as AppKit would have it: installed
/// once, with the main menu. Launch itself (restoring windows, recovering
/// drafts) never runs; tests open documents themselves.
@MainActor
enum TestApp {
    static let delegate: AppDelegate = {
        _ = NSApplication.shared
        let d = AppDelegate()
        NSApp.delegate = d
        NSApp.mainMenu = buildMainMenu()
        return d
    }()
}

/// Notifications, recorded instead of posted.
@MainActor
final class RecordedNotifications: NotificationOutlet {
    private(set) var log: [String] = []
    private var posted: Set<String> = []

    func askOnce() {}

    func post(_ activity: [ThreadActivity], title: String, path: String) {
        posted.insert(path)
        log += activity.map { "notification \(title) | \($0.headline) | \($0.message ?? "-")" }
    }

    func clear(path: String) {
        if posted.remove(path) != nil {
            log.append("notifications cleared for \((path as NSString).lastPathComponent)")
        }
    }
}

/// A document open in an editor window, in a folder and data directory of
/// its own, with ways to act on it as a person does: key presses, clicks and
/// menu commands go through AppKit's own event handling. The windows aren't
/// shown, so a test run never takes the keyboard.
@MainActor
final class Harness {
    let dir: URL
    let doc: DocumentWindow
    let notifications = RecordedNotifications()
    /// Whether the person is looking at the document (for notifications).
    var looking = true
    private let defaultsSuite = "MarginTests.\(UUID().uuidString)"

    var path: String { doc.path }
    var win: NSWindow { doc.window! }
    var view: DocTextView { doc.textView }
    var layer: CommentLayer { doc.layer }

    init(_ text: String = "") throws {
        let app = TestApp.delegate
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("MarginTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        setenv("MARGIN_DATA_DIR", dir.appendingPathComponent(".data").path, 1)
        Prefs.defaults = UserDefaults(suiteName: defaultsSuite)!
        Notifier.shared.outlet = notifications
        let docPath = dir.appendingPathComponent("doc.md").path
        try text.write(toFile: docPath, atomically: true, encoding: .utf8)
        doc = try XCTUnwrap(app.open(path: docPath, show: false))
        Notifier.shared.isLooking = { [unowned self] _ in looking }
    }

    /// Closes every window and forgets the preferences and appearance used.
    func close() {
        for w in TestApp.delegate.windows { w.window?.close() }
        ImageLibrary.shared.fetch = ImageLibrary.read
        NSApp.appearance = nil
        UserDefaults.standard.removePersistentDomain(forName: defaultsSuite)
        Prefs.defaults = .standard
        try? FileManager.default.removeItem(at: dir)
    }

    // MARK: - The document

    var text: String { view.string }

    /// Replaces the text (not undoably), with the cursor at the end.
    func reset(_ text: String) {
        view.setContents(text)
        win.makeFirstResponder(view)
        view.setSelectedRange(NSRange(location: (view.string as NSString).length, length: 0))
    }

    func save() { doc.save() }

    /// An agent editing the file behind the editor's back.
    func external(_ text: String) {
        try? text.write(toFile: path, atomically: true, encoding: .utf8)
    }

    var fileText: String { (try? String(contentsOfFile: path, encoding: .utf8)) ?? "" }

    var selection: String {
        let r = view.selectedRange()
        return "\(r.location) \(r.length)"
    }

    /// The cursor after `s`.
    func find(_ s: String) { place(s) { NSRange(location: NSMaxRange($0), length: 0) } }
    /// The cursor before `s`.
    func findBefore(_ s: String) { place(s) { NSRange(location: $0.location, length: 0) } }
    func select(_ s: String) { place(s) { $0 } }

    /// A multiple selection, as Command-drag makes.
    func select(_ texts: [String]) {
        let s = view.string as NSString
        win.makeFirstResponder(view)
        view.selectedRanges = texts.map { NSValue(range: s.range(of: $0)) }
    }

    private func place(_ s: String, _ f: (NSRange) -> NSRange) {
        let r = (view.string as NSString).range(of: s)
        XCTAssertNotEqual(r.location, NSNotFound, "\(s) not found")
        win.makeFirstResponder(view)
        view.setSelectedRange(f(r))
    }

    // MARK: - Keys

    private static let named: [String: (String, UInt16)] = [
        "enter": ("\r", 36), "return": ("\r", 36),
        "backspace": ("\u{7f}", 51),
        "delete": (String(UnicodeScalar(NSDeleteFunctionKey)!), 117),
        "tab": ("\t", 48),
        "escape": ("\u{1b}", 53),
        "left": (String(UnicodeScalar(NSLeftArrowFunctionKey)!), 123),
        "right": (String(UnicodeScalar(NSRightArrowFunctionKey)!), 124),
        "down": (String(UnicodeScalar(NSDownArrowFunctionKey)!), 125),
        "up": (String(UnicodeScalar(NSUpArrowFunctionKey)!), 126),
        "home": (String(UnicodeScalar(NSHomeFunctionKey)!), 115),
        "end": (String(UnicodeScalar(NSEndFunctionKey)!), 119),
    ]

    /// ANSI key codes, for chords matched by physical key.
    private static let codes: [String: UInt16] = [
        "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12,
        "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23,
        "=": 24, "9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31, "u": 32, "[": 33, "i": 34,
        "p": 35, "l": 37, "j": 38, "k": 40, "n": 45, "m": 46, "/": 44,
    ]

    private static let shifted: [String: String] = ["7": "&", "8": "*", "9": "(", "=": "+", "/": "?"]

    /// Types `text` one key at a time; `\n` is Return and `\t` Tab.
    func type(_ text: String) {
        for ch in text {
            switch ch {
            case "\n": key("enter")
            case "\t": key("tab")
            default: press(String(ch), ignoring: String(ch), code: 0, mods: [])
            }
        }
    }

    /// Types into a comment box, where Return is a character.
    func compose(_ text: String) {
        for ch in text { press(String(ch), ignoring: String(ch), code: 0, mods: []) }
    }

    /// Presses keys by name: `enter`, `backspace`, `left`, or chords like
    /// `cmd-b`, `cmd-shift-z`, `shift-enter`, `opt-backspace`.
    func key(_ names: String...) {
        for name in names {
            var parts = name.split(separator: "-").map(String.init)
            let base = parts.removeLast()
            var mods: NSEvent.ModifierFlags = []
            for p in parts {
                switch p {
                case "cmd": mods.insert(.command)
                case "shift": mods.insert(.shift)
                case "opt", "alt": mods.insert(.option)
                case "ctrl": mods.insert(.control)
                default: XCTFail("unknown modifier \(p)")
                }
            }
            if let (chars, code) = Self.named[base] {
                let c = base == "tab" && mods.contains(.shift) ? "\u{19}" : chars
                press(c, ignoring: c, code: code, mods: mods)
            } else {
                let ignoring = mods.contains(.shift) ? (Self.shifted[base] ?? base.uppercased()) : base
                press(ignoring, ignoring: ignoring, code: Self.codes[base] ?? 0, mods: mods)
            }
        }
    }

    /// The window a key press goes to: a sheet on it, if one shows.
    private var keyWindow: NSWindow { win.attachedSheet ?? win }

    private func press(_ chars: String, ignoring: String, code: UInt16, mods: NSEvent.ModifierFlags) {
        let target = keyWindow
        let down = NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: target.windowNumber, context: nil, characters: chars, charactersIgnoringModifiers: ignoring,
            isARepeat: false, keyCode: code)!
        let isShortcut = mods.contains(.command) || (mods.contains(.control) && mods.contains(.option))
        if isShortcut {
            let base = Self.named.first { $0.value.0 == chars }?.key ?? ignoring
            let key = base == "enter" ? "\r" : (base.count == 1 ? base : chars)
            if shortcut(down, key: key, mods: mods.intersection([.command, .shift, .option, .control]), in: target) {
                return
            }
        }
        deliver(down, to: target)
    }

    /// Delivers an event as AppKit does for the key window. Taken off the
    /// event queue first, so `NSApp.currentEvent` is it. Sent to the window
    /// itself, which AppKit's docs say only it should do: a window that isn't
    /// shown is never key, even after `makeKey()`, so `NSApp.sendEvent` and
    /// the menus' own key equivalents and validation would drop the event.
    private func deliver(_ e: NSEvent, to target: NSWindow) {
        NSApp.postEvent(e, atStart: true)
        let ev = NSApp.nextEvent(matching: .any, until: .distantPast, inMode: .default, dequeue: true) ?? e
        target.sendEvent(ev)
    }

    /// A shortcut, as AppKit handles one for the key window: its views get
    /// it first, then the menu item with that key equivalent, sent up the
    /// window's responder chain. False if nothing takes it.
    private func shortcut(_ e: NSEvent, key: String, mods: NSEvent.ModifierFlags, in target: NSWindow) -> Bool {
        if target.contentView?.performKeyEquivalent(with: e) == true { return true }
        guard let item = Self.menuItem(key: key, mods: mods, in: NSApp.mainMenu!), let action = item.action else {
            return false
        }
        if validate(item), let t = self.target(for: action) { NSApp.sendAction(action, to: t, from: item) }
        return true
    }

    private static func menuItem(key: String, mods: NSEvent.ModifierFlags, in menu: NSMenu) -> NSMenuItem? {
        for i in menu.items {
            if let sub = i.submenu, let f = menuItem(key: key, mods: mods, in: sub) { return f }
            let m = i.keyEquivalentModifierMask.intersection([.command, .shift, .option, .control])
            if !i.keyEquivalent.isEmpty, i.keyEquivalent.lowercased() == key.lowercased(), m == mods { return i }
        }
        return nil
    }

    // MARK: - Commands and menus

    /// The object an action goes to: up the window's responder chain, as
    /// AppKit sends it for the key window.
    func target(for action: Selector) -> AnyObject? {
        var r: NSResponder? = win.firstResponder
        while let x = r {
            if x.responds(to: action) { return x }
            r = x.nextResponder
        }
        if let d = win.delegate, d.responds(to: action) { return d }
        if NSApp.responds(to: action) { return NSApp }
        if let d = NSApp.delegate, d.responds(to: action) { return d }
        return nil
    }

    /// Sends an action as a menu item would.
    func action(_ action: Selector) {
        NSApp.sendAction(action, to: target(for: action), from: nil)
    }

    /// Whether the item's target enables it, as AppKit validates menus.
    func validate(_ item: NSMenuItem) -> Bool {
        guard let action = item.action, let t = target(for: action) else { return false }
        if let v = t as? NSMenuItemValidation { return v.validateMenuItem(item) }
        return true
    }

    /// A main menu item's state, found by title or by a path like
    /// `Comments > Edit`: "enabled", "disabled", with ", checked".
    func menu(_ path: String) -> String {
        let titles = path.components(separatedBy: " > ")
        func find(_ m: NSMenu, _ title: String) -> NSMenuItem? {
            for i in m.items {
                if i.title == title { return i }
                if let sub = i.submenu, let f = find(sub, title) { return f }
            }
            return nil
        }
        var menu: NSMenu? = NSApp.mainMenu
        for t in titles.dropLast() { menu = menu.flatMap { find($0, t) }?.submenu }
        guard let item = menu.flatMap({ find($0, titles.last!) }) else { return "not found" }
        return (validate(item) ? "enabled" : "disabled") + (item.state == .on ? ", checked" : "")
    }

    /// The text's right-click menu at the selection: its first two items;
    /// with `choose`, the item chosen as AppKit would.
    @discardableResult
    func contextMenu(choose: String? = nil) -> [String] {
        let at = view.firstRect(forCharacterRange: view.selectedRange(), actualRange: nil)
        let p = win.convertPoint(fromScreen: NSPoint(x: at.midX, y: at.midY))
        guard let e = mouseEvent(.rightMouseDown, at: p), let menu = view.menu(for: e) else { return [] }
        let items = menu.items.prefix(2).map {
            $0.isSeparatorItem ? "—" : $0.title + (validate($0) ? "" : " (disabled)")
        }
        if let choose, let item = menu.items.first(where: { $0.title == choose }), let action = item.action {
            NSApp.sendAction(action, to: target(for: action), from: item)
        }
        return items
    }

    // MARK: - The mouse

    private func mouseEvent(
        _ type: NSEvent.EventType, at p: NSPoint, mods: NSEvent.ModifierFlags = [], after dt: Double = 0
    )
        -> NSEvent?
    {
        NSEvent.mouseEvent(
            with: type, location: p, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime + dt,
            windowNumber: win.windowNumber, context: nil, eventNumber: 1, clickCount: 1,
            pressure: type == .leftMouseUp ? 0 : 1)
    }

    /// Lays out the window, as AppKit does before it draws; the windows
    /// aren't shown, so it never draws.
    func layOut() {
        win.contentView?.superview?.layoutSubtreeIfNeeded()
    }

    /// A click at `p` in `v`: the view AppKit would hit-test to gets it,
    /// with the release queued first, since controls and text views track
    /// the mouse in their own event loop until it arrives.
    func click(_ v: NSView, at p: NSPoint, mods: NSEvent.ModifierFlags = []) {
        layOut()
        let inWindow = v.convert(p, to: nil)
        if let up = mouseEvent(.leftMouseUp, at: inWindow, mods: mods, after: 0.05) {
            NSApp.postEvent(up, atStart: false)
        }
        guard let down = mouseEvent(.leftMouseDown, at: inWindow, mods: mods),
            let frame = win.contentView?.superview, let hit = frame.hitTest(frame.convert(inWindow, from: nil))
        else { return }
        hit.mouseDown(with: down)
        discardMouseEvents()
    }

    /// Drops the queued releases a view didn't track the mouse for, which
    /// no run loop dispatches in a test; the next click would get them.
    private func discardMouseEvents() {
        NSApp.discardEvents(matching: [.leftMouseUp, .leftMouseDragged], before: nil)
    }

    /// A press at `a` in `v`, a drag to `b` and a release there.
    func drag(_ v: NSView, from a: NSPoint, to b: NSPoint) {
        layOut()
        let (pa, pb) = (v.convert(a, to: nil), v.convert(b, to: nil))
        for e in [mouseEvent(.leftMouseDragged, at: pb, after: 0.05), mouseEvent(.leftMouseUp, at: pb, after: 0.1)] {
            if let e { NSApp.postEvent(e, atStart: false) }
        }
        guard let down = mouseEvent(.leftMouseDown, at: pa), let frame = win.contentView?.superview,
            let hit = frame.hitTest(frame.convert(pa, from: nil))
        else { return }
        hit.mouseDown(with: down)
        discardMouseEvents()
    }

    /// A click in the middle of `s` in the text.
    func click(text s: String, mods: NSEvent.ModifierFlags = []) {
        let r = (view.string as NSString).range(of: s)
        XCTAssertNotEqual(r.location, NSNotFound, "\(s) not found")
        let lm = view.layoutManager!
        lm.ensureLayout(for: view.textContainer!)
        let g = lm.glyphRange(forCharacterRange: r, actualCharacterRange: nil)
        let rect = lm.boundingRect(forGlyphRange: g, in: view.textContainer!)
            .offsetBy(dx: view.textContainerOrigin.x, dy: view.textContainerOrigin.y)
        click(view, at: NSPoint(x: rect.midX, y: rect.midY), mods: mods)
    }

    /// A click at a point of the text view.
    func click(x: CGFloat, y: CGFloat) { click(view, at: NSPoint(x: x, y: y)) }

    // MARK: - Comment cards

    func item(_ id: UInt64) -> ThreadItem? { layer.items.first { $0.thread.id == id } }

    func card(_ id: UInt64) throws -> ThreadCard { try XCTUnwrap(item(id)?.card, "no card #\(id)") }

    /// A click on card `id`'s padding.
    func click(card id: UInt64) throws {
        let c = try card(id)
        click(c, at: NSPoint(x: c.bounds.width - 30, y: c.bounds.height - 12))
    }

    /// Card `id`'s shown button titled `title`, if any.
    func button(_ title: String, onCard id: UInt64) throws -> NSButton? {
        layOut()
        func find(_ v: NSView) -> NSButton? {
            if let b = v as? NSButton, b.title == title, !b.isHiddenOrHasHiddenAncestor { return b }
            for s in v.subviews { if let f = find(s) { return f } }
            return nil
        }
        return find(try card(id))
    }

    func click(button title: String, onCard id: UInt64) throws {
        let b = try XCTUnwrap(try button(title, onCard: id), "no button \(title) on card #\(id)")
        click(b, at: NSPoint(x: b.bounds.midX, y: b.bounds.midY))
    }

    func clickResolve(card id: UInt64) throws {
        let b = try XCTUnwrap(try card(id).resolveButton)
        click(b, at: NSPoint(x: b.bounds.midX, y: b.bounds.midY))
    }

    private func texts(in v: NSView) -> [CardText] {
        v.subviews.flatMap { ($0 as? CardText).map { [$0] } ?? texts(in: $0) }
    }

    /// Clicks, or drags across, the message text containing `s` on card `id`.
    func click(cardText s: String, onCard id: UInt64, drag dragging: Bool = false) throws {
        layOut()
        let t = try XCTUnwrap(texts(in: try card(id)).first { $0.stringValue.contains(s) })
        let b = t.bounds
        if dragging {
            drag(t, from: NSPoint(x: b.minX + 2, y: b.midY), to: NSPoint(x: b.maxX - 2, y: b.midY))
        } else {
            click(t, at: NSPoint(x: b.midX, y: b.midY))
        }
        wait(0.2)
    }

    /// The quote card `id` shows for deleted text, if it shows one.
    func deletedQuote(onCard id: UInt64) throws -> String? {
        texts(in: try card(id)).first { $0.toolTip == "The commented text was deleted" }?.stringValue
    }

    /// Each message's author on card `id`, as shown (✦ for the agent's
    /// symbol).
    func authors(onCard id: UInt64) throws -> [String] {
        try card(id).bylines.map {
            String($0.stringValue.replacingOccurrences(of: "\u{FFFC} ", with: "✦ ").split(separator: " · ")[0])
        }
    }

    /// Each message's text on card `id`: "short", "collapsed", "expanded"
    /// or "editing".
    func messageStates(onCard id: UInt64) throws -> [String] {
        try card(id).texts.map { t in
            t.map { $0.long ? ($0.expanded ? "expanded" : "collapsed") : "short" } ?? "editing"
        }
    }

    /// The pointer onto (or off) card `id`; whether its buttons show.
    func hover(card id: UInt64, on: Bool = true) throws -> Bool {
        let c = try card(id)
        let e = NSEvent.enterExitEvent(
            with: on ? .mouseEntered : .mouseExited, location: .zero, modifierFlags: [], timestamp: 0,
            windowNumber: win.windowNumber, context: nil, eventNumber: 0, trackingNumber: 0, userData: nil)!
        if on { c.mouseEntered(with: e) } else { c.mouseExited(with: e) }
        return c.showsButtons
    }

    /// Message `n`'s "…" menu on card `id` (or its right-click menu): the
    /// items; with `choose`, the item chosen as AppKit would.
    @discardableResult
    func menu(ofMessage n: Int, onCard id: UInt64, context: Bool = false, choose: String? = nil) throws -> [String] {
        let c = try card(id)
        let r = try XCTUnwrap(c.messageRect(n))
        let menu: NSMenu?
        if context {
            menu = mouseEvent(.rightMouseDown, at: c.convert(NSPoint(x: r.midX, y: r.midY), to: nil)).flatMap {
                c.menu(for: $0)
            }
        } else {
            menu = c.messageMenu(n)
        }
        let m = try XCTUnwrap(menu)
        if let choose, let item = m.items.first(where: { $0.title == choose }), let action = item.action {
            NSApp.sendAction(action, to: item.target, from: item)
        }
        return m.items.map { $0.isSeparatorItem ? "—" : $0.title }
    }

    /// The threads, the draft and the focused thread, one per line:
    /// `#1 open [0,5) ["Why?"]`, `draft [0,5)`, `active 1`.
    var comments: String {
        var out = layer.items.map { it in
            let t = it.thread
            let end = it.range.map(NSMaxRange) ?? it.start
            let bodies = "[" + t.messages.map { $0.body.debugDescription }.joined(separator: ", ") + "]"
            return
                "#\(t.id) \(t.resolved ? "resolved" : "open")\(it.detached ? " detached" : "") [\(it.start),\(end)) \(bodies)"
        }
        if let d = layer.draftRange { out.append("draft [\(d.location),\(NSMaxRange(d)))") }
        out.append("active \(layer.active.map(String.init) ?? "none")")
        return out.joined(separator: "\n")
    }

    /// How far each shown card is from beside its text, once the page is
    /// laid out as it is before the window draws.
    var cardOffsets: [UInt64: CGFloat] {
        layOut()
        var out: [UInt64: CGFloat] = [:]
        for it in layer.items where layer.visible(it.thread) {
            out[it.thread.id] = it.card.frame.minY - layer.lineTop(it.start)
        }
        return out
    }

    // MARK: - The window

    var banner: String? { doc.banner.message }

    /// The type of what has the keyboard.
    var focus: String { win.firstResponder.map { String(describing: Swift.type(of: $0)) } ?? "nil" }

    func size(_ width: CGFloat, _ height: CGFloat) {
        win.setContentSize(NSSize(width: width, height: height))
        wait(0.1)
    }

    /// The page's layout: the text column and the cards' width.
    var page: String {
        doc.page.layoutSubtreeIfNeeded()
        let g = view.geometry
        return "left \(Int(g.left)) text \(Int(g.docWidth)) cards \(doc.page.hasCards ? "\(Int(g.cardWidth))" : "none")"
    }

    var title: String { "\(win.title) | \(win.subtitle.isEmpty ? "-" : "subtitle") | edited \(win.isDocumentEdited)" }

    /// The buttons of the sheet on the window, if one shows.
    var sheet: [String]? {
        guard let content = win.attachedSheet?.contentView else { return nil }
        func buttons(_ v: NSView) -> [NSButton] {
            v.subviews.flatMap { s in (s as? NSButton).map { [$0] } ?? buttons(s) }
        }
        return buttons(content).filter { !$0.isHiddenOrHasHiddenAncestor }.map(\.title)
    }

    func press(sheetButton title: String) throws {
        func buttons(_ v: NSView) -> [NSButton] {
            v.subviews.flatMap { s in (s as? NSButton).map { [$0] } ?? buttons(s) }
        }
        let content = try XCTUnwrap(win.attachedSheet?.contentView, "no sheet")
        try XCTUnwrap(buttons(content).first { $0.title == title }, "no button \(title)").performClick(nil)
    }

    /// The document windows open, by name.
    var windows: [String] {
        TestApp.delegate.windows.map { $0.isDraft ? "Untitled" : ($0.path as NSString).lastPathComponent }
    }

    var storedThreads: Int { (try? CommentStore(document: path).load(text: view.string).count) ?? -1 }

    var undoName: String { view.undoManager?.undoMenuItemTitle ?? "none" }

    // MARK: - Agents

    /// The `margin` CLI the build made, run as an agent does, in the
    /// document's folder; its output.
    @discardableResult
    func margin(_ args: String...) -> String {
        sh(([Harness.cli] + args.map(Harness.quoted)).joined(separator: " "))
    }

    /// A shell command in the document's folder, with the `margin` CLI on
    /// the PATH; its output.
    @discardableResult
    func sh(_ command: String) -> String {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", command]
        p.currentDirectoryURL = dir
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = (Harness.cli as NSString).deletingLastPathComponent + ":" + (env["PATH"] ?? "/usr/bin:/bin")
        p.environment = env
        let out = Pipe()
        p.standardOutput = out
        try? p.run()
        p.waitUntilExit()
        wait(0.3)
        return String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    }

    /// The CLI next to the Rust library the build linked (in
    /// macos/build, two folders up from this file).
    static let cli = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("build/rust/Debug/margin").path

    private static func quoted(_ s: String) -> String { "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'" }

    // MARK: - Time

    /// Lets timers, file watchers and queued work run for `seconds`.
    func wait(_ seconds: Double) {
        RunLoop.main.run(until: Date(timeIntervalSinceNow: seconds))
    }

    /// Waits until `condition` holds, or fails after `timeout` seconds.
    func wait(
        until condition: () -> Bool, timeout: Double = 5, _ what: String = "condition", file: StaticString = #filePath,
        line: UInt = #line
    ) {
        let deadline = Date(timeIntervalSinceNow: timeout)
        while !condition() {
            if Date() > deadline { return XCTFail("timed out waiting for \(what)", file: file, line: line) }
            wait(0.05)
        }
    }

    func wait(forBanner s: String, file: StaticString = #filePath, line: UInt = #line) {
        wait(until: { banner?.contains(s) ?? false }, "banner \(s)", file: file, line: line)
    }

    func wait(forText s: String, file: StaticString = #filePath, line: UInt = #line) {
        wait(until: { text.contains(s) }, "text \(s)", file: file, line: line)
    }
}
