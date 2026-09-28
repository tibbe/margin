import AppKit

#if SCRIPTING

/// A scripted driver for testing the editor without a person at the
/// keyboard: `MARGIN_SCRIPT=steps.txt Margin.app/Contents/MacOS/Margin doc.md`,
/// in a test build (`build.sh debug`, which compiles with `SCRIPTING`).
/// Keys and clicks are real events sent through AppKit's event routing
/// (menus, key equivalents, hit-testing), not calls into the editor. Steps:
///
/// ```text
/// type Hello **world**      type text, one key event per character
/// wait-banner TEXT | wait-text TEXT   wait until the banner or document says TEXT
/// change-spelling WORD      the Spelling panel's Change, with WORD
/// select-ranges A|B         a multiple selection
/// bench-type TEXT           type, redrawing after each key; print the time per key
/// key NAME                  enter shift-enter backspace delete tab shift-tab
///                           left right up down home end escape, or a chord
///                           like cmd-b, cmd-shift-8, cmd-opt-m, cmd-enter
/// find TEXT | find-before TEXT | select TEXT
///                           put the cursor after (before) TEXT, or select it
/// click-text TEXT           click in the middle of TEXT (cmd-click-text too)
/// click-card ID | click-resolve ID | click-add | click-checkbox N
/// click X Y                 click at view coordinates of the document view
/// action SELECTOR           send an action up the responder chain
/// compose TEXT              type into the focused comment box
/// ime-mark TEXT | ime-commit TEXT | hold-accent TEXT | marked
///                           input method calls: compose, commit, press-and-hold
/// replace-all FIND|WITH    Find and Replace, then Replace All
/// reset TEXT              replace the document (not undoable), cursor at end
/// save | external TEXT | rename NAME   save; change the file as an agent; rename
/// path | title | stored | windows      print file, title, stored threads, windows
/// menu TITLE               validate a menu item; print enabled and checked
/// size W H | wait MS | shot PATH | dump | comments | banner | focus
/// selection | sh CMD | quit
///                           sh runs CMD in the document's folder
/// ```
///
/// Text steps accept `\n` and `\t` escapes.
enum ScriptDriver {
    private static var steps: [String] = []
    private static weak var window: DocumentWindow?

    static func start(window w: DocumentWindow) {
        guard let path = ProcessInfo.processInfo.environment["MARGIN_SCRIPT"],
              let text = try? String(contentsOfFile: path, encoding: .utf8) else { return }
        steps = text.components(separatedBy: "\n")
        // Output goes out line by line: with sudden termination, quitting
        // ends the process without flushing buffers.
        setvbuf(stdout, nil, _IOLBF, 0)
        window = w
        // Menu shortcuts go to the key window, which only an active app has.
        // macOS may refuse to take focus from the app you are using; then
        // shortcuts are dispatched as AppKit would (see `dispatchShortcut`).
        NSApp.activate()
        w.window?.makeKeyAndOrderFront(nil)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { run(0) }
    }

    private static func run(_ i: Int) {
        guard i < steps.count else {
            NSApp.terminate(nil)
            return
        }
        let line = steps[i].trimmingCharacters(in: .whitespaces)
        if let (what, text) = waitStep(line) {
            poll(what, text, deadline: Date().addingTimeInterval(5)) { run(i + 1) }
            return
        }
        var delay = 0.03
        if !line.isEmpty && !line.hasPrefix("#") {
            delay = step(line) ?? delay
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { run(i + 1) }
    }

    /// `wait-banner TEXT` and `wait-text TEXT` wait (up to 5 s) until the
    /// banner says TEXT or the document contains it.
    private static func waitStep(_ line: String) -> (String, String)? {
        for w in ["wait-banner", "wait-text"] where line.hasPrefix(w + " ") {
            return (w, unescape(String(line.dropFirst(w.count + 1))))
        }
        return nil
    }

    private static func poll(_ what: String, _ text: String, deadline: Date, then: @escaping () -> Void) {
        guard let w = window else { return then() }
        let done = what == "wait-banner" ? (w.banner.message?.contains(text) ?? false) : w.textView.string.contains(text)
        if done || Date() > deadline {
            if !done { print("script: timed out waiting for \(text)") }
            return then()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { poll(what, text, deadline: deadline, then: then) }
    }

    private static func unescape(_ s: String) -> String {
        s.replacingOccurrences(of: "\\n", with: "\n").replacingOccurrences(of: "\\t", with: "\t")
    }

    /// Runs one step; returns how long to wait before the next.
    private static func step(_ line: String) -> Double? {
        guard let w = window, let win = w.window else { return nil }
        let view = w.textView
        let (cmd, arg) = { () -> (String, String) in
            if let sp = line.firstIndex(of: " ") { return (String(line[..<sp]), String(line[line.index(after: sp)...])) }
            return (line, "")
        }()
        switch cmd {
        case "type":
            let text = unescape(arg)
            for ch in text {
                if ch == "\n" { key("enter", win) } else if ch == "\t" { key("tab", win) } else { typeChar(String(ch), win) }
            }
            return 0.05 + 0.004 * Double(text.count)
        case "bench-type":
            // Each key and the redraw it causes, as a person typing sees it.
            let chars = Array(unescape(arg))
            let t0 = CFAbsoluteTimeGetCurrent()
            var worst = 0.0
            for ch in chars {
                let t = CFAbsoluteTimeGetCurrent()
                send(keyDown: String(ch), ignoring: String(ch), code: 0, mods: [], win, now: true)
                win.displayIfNeeded()
                worst = max(worst, CFAbsoluteTimeGetCurrent() - t)
            }
            let avg = (CFAbsoluteTimeGetCurrent() - t0) / Double(max(chars.count, 1))
            print(String(format: "bench-type %d keys: %.1f ms per key, worst %.1f ms", chars.count, avg * 1000, worst * 1000))
        case "ime-mark":
            // What an input method does while composing, through the same
            // NSTextInputClient calls.
            let t = unescape(arg)
            view.setMarkedText(t, selectedRange: NSRange(location: (t as NSString).length, length: 0),
                               replacementRange: NSRange(location: NSNotFound, length: 0))
        case "ime-mark-attributed":
            // As input methods send it: underlined clauses.
            let t = unescape(arg)
            let a = NSAttributedString(string: t, attributes: [
                .underlineStyle: NSUnderlineStyle.single.rawValue,
                .markedClauseSegment: 0,
            ])
            view.setMarkedText(a, selectedRange: NSRange(location: (t as NSString).length, length: 0),
                               replacementRange: NSRange(location: NSNotFound, length: 0))
        case "os-key":
            // A keyboard event with only a key code, posted to ourselves, so
            // macOS translates it through the keyboard layout (dead keys
            // included): `os-key 14 opt` is Option-E.
            let parts = arg.split(separator: " ")
            let code = CGKeyCode(parts.first.flatMap { UInt16($0) } ?? 0)
            var flags: CGEventFlags = []
            if parts.contains("opt") { flags.insert(.maskAlternate) }
            if parts.contains("shift") { flags.insert(.maskShift) }
            if parts.contains("cmd") { flags.insert(.maskCommand) }
            if flags.contains(.maskCommand) && !NSApp.isActive,
               let e = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true), let ev = NSEvent(cgEvent: e) {
                // Shortcuts need the key window; see `dispatchShortcut`.
                e.flags = flags
                let mods = NSEvent(cgEvent: e)?.modifierFlags.intersection([.command, .shift, .option, .control]) ?? []
                _ = dispatchShortcut(NSEvent(cgEvent: e) ?? ev, base: (NSEvent(cgEvent: e)?.charactersIgnoringModifiers ?? ""), mods: mods, win)
                return 0.2
            }
            for down in [true, false] {
                if let e = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down) {
                    e.flags = flags
                    e.postToPid(getpid())
                }
            }
            return 0.4
        case "ime-commit":
            view.insertText(unescape(arg), replacementRange: NSRange(location: NSNotFound, length: 0))
        case "hold-accent":
            // Press and hold: the accented letter replaces the one typed.
            let c = view.selectedRange().location
            view.insertText(unescape(arg), replacementRange: NSRange(location: max(0, c - 1), length: min(1, c)))
        case "marked":
            let r = view.markedRange()
            print(view.hasMarkedText() ? "marked \(r.location) \(r.length)" : "marked none")
        case "key":
            let keys = arg.split(separator: " ")
            for k in keys { key(String(k), win) }
            return 0.05 + 0.004 * Double(keys.count)
        case "find", "find-before", "select":
            let s = view.string as NSString
            let r = s.range(of: unescape(arg))
            if r.location == NSNotFound { print("script: \(arg) not found") } else {
                win.makeFirstResponder(view)
                switch cmd {
                case "find": view.setSelectedRange(NSRange(location: NSMaxRange(r), length: 0))
                case "find-before": view.setSelectedRange(NSRange(location: r.location, length: 0))
                default: view.setSelectedRange(r)
                }
            }
        case "click-text", "cmd-click-text":
            let s = view.string as NSString
            let r = s.range(of: unescape(arg))
            if r.location == NSNotFound { print("script: \(arg) not found"); break }
            view.layoutManager?.ensureLayout(for: view.textContainer!)
            let g = view.layoutManager!.glyphRange(forCharacterRange: r, actualCharacterRange: nil)
            var rect = view.layoutManager!.boundingRect(forGlyphRange: g, in: view.textContainer!)
            rect = rect.offsetBy(dx: view.textContainerOrigin.x, dy: view.textContainerOrigin.y)
            click(view, at: NSPoint(x: rect.midX, y: rect.midY), mods: cmd == "cmd-click-text" ? [.command] : [])
        case "click":
            let p = arg.split(separator: " ").compactMap { Double($0) }
            if p.count == 2 { click(view, at: NSPoint(x: p[0], y: p[1])) }
        case "click-card":
            if let it = w.layer.items.first(where: { $0.thread.id == UInt64(arg) ?? 0 }) {
                click(it.card, at: NSPoint(x: it.card.bounds.width - 30, y: it.card.bounds.height - 12))
            }
        case "click-resolve":
            if let it = w.layer.items.first(where: { $0.thread.id == UInt64(arg) ?? 0 }), let b = it.card.resolveButton {
                click(b, at: NSPoint(x: b.bounds.midX, y: b.bounds.midY))
            }
        case "context-menu":
            // The text's right-click menu at the selection: prints its first two
            // items, and with an argument chooses that item as AppKit would.
            let at = view.firstRect(forCharacterRange: view.selectedRange(), actualRange: nil)
            let p = win.convertPoint(fromScreen: NSPoint(x: at.midX, y: at.midY))
            guard let e = NSEvent.mouseEvent(with: .rightMouseDown, location: p, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                             windowNumber: win.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1),
                  let menu = view.menu(for: e) else { print("script: no context menu"); break }
            print("context menu: \(menu.items.prefix(2).map { $0.isSeparatorItem ? "—" : "\($0.title)\(validate($0, in: win) ? "" : " (disabled)")" }.joined(separator: ", "))")
            if !arg.isEmpty, let item = menu.items.first(where: { $0.title == arg }), let action = item.action {
                NSApp.sendAction(action, to: NSApp.isActive ? nil : target(for: action, in: win), from: item)
            }
        case "click-checkbox":
            let n = Int(arg) ?? 0
            view.display()
            if let r = view.checkboxRect(n) { click(view, at: NSPoint(x: r.midX, y: r.midY)) } else { print("script: no checkbox \(n)") }
        case "action":
            let action = Selector(arg)
            NSApp.sendAction(action, to: NSApp.isActive ? nil : target(for: action, in: win), from: nil)
        case "compose":
            let text = unescape(arg)
            for ch in text { typeChar(String(ch), win) }
            return 0.05 + 0.004 * Double(text.count)
        case "change-spelling":
            // What the Spelling panel's Change button sends.
            let m = NSMatrix(frame: .zero, mode: .radioModeMatrix, cellClass: NSCell.self, numberOfRows: 1, numberOfColumns: 1)
            m.cells.first?.stringValue = unescape(arg)
            m.selectCell(atRow: 0, column: 0)
            NSApp.sendAction(#selector(NSTextView.changeSpelling(_:)), to: view, from: m)
        case "select-ranges":
            // A multiple selection (as Command-drag makes): each TEXT, `|`-separated.
            let s = view.string as NSString
            let ranges = unescape(arg).components(separatedBy: "|").map { s.range(of: $0) }.filter { $0.location != NSNotFound }
            win.makeFirstResponder(view)
            view.selectedRanges = ranges.map { NSValue(range: $0) }
        case "size":
            let p = arg.split(separator: " ").compactMap { Double($0) }
            if p.count == 2 { win.setContentSize(NSSize(width: p[0], height: p[1])) }
            return 0.3
        case "wait":
            return (Double(arg) ?? 200) / 1000
        case "shot":
            shot(win, path: arg)
        case "replace-all":
            let parts = unescape(arg).components(separatedBy: "|")
            w.findBar.open(replace: true)
            w.findBar.search.stringValue = parts[0]
            w.findBar.replaceField.stringValue = parts.count > 1 ? parts[1] : ""
            w.findBar.refresh(jump: true)
            w.findBar.replaceAll()
        case "save":
            w.save()
        case "external":
            // An agent editing the file behind the editor's back.
            try? unescape(arg).write(toFile: w.path, atomically: true, encoding: .utf8)
        case "rename":
            do { try w.relocate(to: ((w.path as NSString).deletingLastPathComponent as NSString).appendingPathComponent(arg), moving: true) } catch {
                print("script: rename failed: \(error.localizedDescription)")
            }
        case "path":
            let exists = FileManager.default.fileExists(atPath: w.path)
            print("path \((w.path as NSString).lastPathComponent)\(exists ? "" : " (missing)")")
        case "title":
            print("title \(win.title) | \(win.subtitle.isEmpty ? "-" : "subtitle") | edited \(win.isDocumentEdited)")
        case "stored":
            let n = (try? CommentStore(document: w.path).load(text: view.string).count) ?? -1
            print("stored \(n) threads")
        case "windows":
            let names = AppDelegate.shared.windows.map { $0.isDraft ? "Untitled" : ($0.path as NSString).lastPathComponent }
            print("windows \(names.joined(separator: ", "))")
        case "reset":
            view.setContents(unescape(arg))
            win.makeFirstResponder(view)
            view.setSelectedRange(NSRange(location: (view.string as NSString).length, length: 0))
        case "dump":
            print(view.string, terminator: "")
            print("--- end")
        case "attrs":
            let s = view.string as NSString
            let r = s.range(of: unescape(arg))
            if r.location != NSNotFound, let st = view.textStorage {
                let ps = st.attribute(.paragraphStyle, at: r.location, effectiveRange: nil) as? NSParagraphStyle
                let f = view.fragmentRect(at: r.location)
                print("attrs \(arg): before=\(ps?.paragraphSpacingBefore ?? -1) spacing=\(ps?.lineSpacing ?? -1) indent=\(ps?.headIndent ?? -1) frag=\(f)")
            }
        case "frags":
            let lm = view.layoutManager!
            lm.ensureLayout(for: view.textContainer!)
            let s = view.string as NSString
            var g = 0
            var n = 0
            while g < lm.numberOfGlyphs && n < (Int(arg) ?? 12) {
                var r = NSRange()
                let rect = lm.lineFragmentRect(forGlyphAt: g, effectiveRange: &r)
                let chars = lm.characterRange(forGlyphRange: r, actualGlyphRange: nil)
                print("frag y=\(rect.minY) h=\(rect.height) chars=\(chars) \(s.substring(with: chars).debugDescription)")
                g = NSMaxRange(r)
                n += 1
            }
        case "markers":
            // Whether each list item's marker sits on its own text: on the
            // baseline of the item's last character.
            let lm = view.layoutManager!
            lm.ensureLayout(for: view.textContainer!)
            for it in view.items {
                let line = view.lines[Int(it.line)]
                let last = max(Int(line.contentStart), Int(line.end) - 1)
                let text = view.fragmentRect(at: last).minY + lm.location(forGlyphAt: lm.glyphIndexForCharacter(at: last)).y
                let off = view.markerPosition(it).baseline - text
                let body = view.visibleText(NSRange(location: Int(line.contentStart), length: Int(line.end - line.contentStart)))
                print("marker \(body.debugDescription): \(abs(off) < 0.5 ? "on its text" : "off by \(off)")")
            }
        case "selection":
            let r = view.selectedRange()
            print("selection \(r.location) \(r.length)")
        case "comments":
            for it in w.layer.items {
                let t = it.thread
                print("#\(t.id) \(t.resolved ? "resolved" : "open")\(it.detached ? " detached" : "") [\(it.start),\(it.end)) \(t.messages.map { $0.body })")
            }
            if let d = w.layer.draftRange { print("draft [\(d.location),\(NSMaxRange(d)))") }
            print("active \(w.layer.active.map(String.init) ?? "none")")
        case "menu":
            // Validates a menu item as AppKit does before showing the menu.
            func find(_ m: NSMenu) -> NSMenuItem? {
                for i in m.items {
                    if i.title == arg { return i }
                    if let sub = i.submenu, let f = find(sub) { return f }
                }
                return nil
            }
            if let item = find(NSApp.mainMenu!) {
                let enabled: Bool
                if NSApp.isActive {
                    item.menu?.update()
                    enabled = item.isEnabled
                } else {
                    enabled = validate(item, in: win)
                }
                print("menu \(arg): \(enabled ? "enabled" : "disabled")\(item.state == .on ? ", checked" : "")")
            } else {
                print("menu \(arg): not found")
            }
        case "banner":
            print("banner \(w.banner.message ?? "none")")
        case "focus":
            print("focus \(String(describing: win.firstResponder.map { type(of: $0) }))")
        case "sh":
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/bin/sh")
            p.arguments = ["-c", arg]
            p.currentDirectoryURL = URL(fileURLWithPath: (w.path as NSString).deletingLastPathComponent)
            try? p.run()
            p.waitUntilExit()
            return 0.3
        case "quit":
            NSApp.terminate(nil)
        default:
            print("script: unknown step \(cmd)")
        }
        return nil
    }

    private static func typeChar(_ c: String, _ win: NSWindow) {
        send(keyDown: c, ignoring: c, code: 0, mods: [], win)
    }

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

    // ANSI key codes, for chords matched by physical key.
    private static let codes: [String: UInt16] = [
        "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12,
        "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23,
        "=": 24, "9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31, "u": 32, "[": 33, "i": 34,
        "p": 35, "l": 37, "j": 38, "k": 40, "n": 45, "m": 46, "/": 44,
    ]

    private static let shifted: [String: String] = ["7": "&", "8": "*", "9": "(", "=": "+", "/": "?"]

    private static func key(_ name: String, _ win: NSWindow) {
        var parts = name.split(separator: "-").map(String.init)
        let base = parts.removeLast()
        var mods: NSEvent.ModifierFlags = []
        for p in parts {
            switch p {
            case "cmd": mods.insert(.command)
            case "shift": mods.insert(.shift)
            case "opt", "alt": mods.insert(.option)
            case "ctrl": mods.insert(.control)
            default: break
            }
        }
        if let (chars, code) = named[base] {
            let c = base == "tab" && mods.contains(.shift) ? "\u{19}" : chars
            send(keyDown: c, ignoring: c, code: code, mods: mods, win)
        } else {
            let ignoring = mods.contains(.shift) ? (shifted[base] ?? base.uppercased()) : base
            send(keyDown: ignoring, ignoring: ignoring, code: codes[base] ?? 0, mods: mods, win)
        }
    }

    /// Queues a key press, so AppKit handles it as it does real ones
    /// (`NSApp.currentEvent` included); with `now`, handles it at once.
    /// The object a menu item's action goes to: up the window's responder
    /// chain, as AppKit sends it for the key window.
    private static func target(for action: Selector, in win: NSWindow) -> AnyObject? {
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

    /// Whether the item's target enables it, as AppKit validates menus.
    private static func validate(_ item: NSMenuItem, in win: NSWindow) -> Bool {
        guard let action = item.action, let t = target(for: action, in: win) else { return false }
        if let v = t as? NSMenuItemValidation { return v.validateMenuItem(item) }
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

    /// A shortcut while the app isn't active (macOS won't let a test take
    /// focus from the app you are using): the window's views get it first,
    /// then the menu item with that key equivalent, sent up the window's
    /// responder chain, as AppKit does for the key window.
    private static func dispatchShortcut(_ e: NSEvent, base: String, mods: NSEvent.ModifierFlags, _ win: NSWindow) -> Bool {
        if win.contentView?.performKeyEquivalent(with: e) == true { return true }
        guard let item = menuItem(key: base, mods: mods, in: NSApp.mainMenu!), let action = item.action else { return false }
        if validate(item, in: win), let t = target(for: action, in: win) {
            NSApp.sendAction(action, to: t, from: item)
        }
        return true
    }

    private static func send(keyDown chars: String, ignoring: String, code: UInt16, mods: NSEvent.ModifierFlags, _ win: NSWindow, now: Bool = false) {
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            if let e = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime,
                                        windowNumber: win.windowNumber, context: nil, characters: chars,
                                        charactersIgnoringModifiers: ignoring, isARepeat: false, keyCode: code) {
                let shortcut = mods.contains(.command) || (mods.contains(.control) && mods.contains(.option))
                if type == .keyDown && shortcut && !NSApp.isActive {
                    let base = named.first(where: { $0.value.0 == chars })?.key ?? ignoring
                    let key = base == "enter" ? "\r" : (base.count == 1 ? base : chars)
                    if dispatchShortcut(e, base: key, mods: mods.intersection([.command, .shift, .option, .control]), win) { return }
                }
                if now { NSApp.sendEvent(e) } else { NSApp.postEvent(e, atStart: false) }
            }
        }
    }

    /// A real click: the mouse-up is queued first, because controls and
    /// text views track the mouse in their own event loop until it arrives.
    private static func click(_ v: NSView, at p: NSPoint, mods: NSEvent.ModifierFlags = []) {
        guard let win = v.window else { return }
        let inWindow = v.convert(p, to: nil)
        let t = ProcessInfo.processInfo.systemUptime
        let upEvent = NSEvent.mouseEvent(with: .leftMouseUp, location: inWindow, modifierFlags: mods, timestamp: t + 0.05,
                                         windowNumber: win.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 0)
        let downEvent = NSEvent.mouseEvent(with: .leftMouseDown, location: inWindow, modifierFlags: mods, timestamp: t,
                                           windowNumber: win.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1)
        if let u = upEvent { NSApp.postEvent(u, atStart: false) }
        guard let d = downEvent else { return }
        if NSApp.isActive {
            NSApp.sendEvent(d)
        } else if let frame = win.contentView?.superview, let hit = frame.hitTest(frame.convert(inWindow, from: nil)) {
            // An inactive app's window takes a first click only to become
            // active; macOS may not let a test activate it (see `start`), so
            // the view AppKit would hit-test to gets the click.
            hit.mouseDown(with: d)
        }
    }

    /// The window as drawn, title bar included, written as PNG.
    private static func shot(_ win: NSWindow, path: String) {
        guard let frameView = win.contentView?.superview else { return }
        frameView.layoutSubtreeIfNeeded()
        let b = frameView.bounds
        guard let rep = frameView.bitmapImageRepForCachingDisplay(in: b) else { return }
        frameView.cacheDisplay(in: b, to: rep)
        if let data = rep.representation(using: .png, properties: [:]) {
            try? data.write(to: URL(fileURLWithPath: path))
        }
    }
}
#endif
