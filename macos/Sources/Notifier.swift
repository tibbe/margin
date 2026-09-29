import AppKit
import UserNotifications

/// System notifications for agent activity on documents the person isn't
/// looking at: one per thread change, grouped by document, removed once
/// the document's window is key again. Permission is asked the first time
/// activity is announced while Margin is frontmost; until then nothing is
/// posted.
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    static let shared = Notifier()

    /// Test runs print what would be posted instead: the ad hoc test app
    /// must not ask for permission or post.
    private let scripted = scriptPath != nil
    /// Test runs say whether the person is looking at the document, rather
    /// than depend on whether macOS let the test app become active.
    var scriptedLooking = true
    /// Documents with notifications posted, in test runs.
    private var scriptedPosted: Set<String> = []

    private var center: UNUserNotificationCenter { .current() }

    /// Category identifiers, and what shows when previews are hidden.
    private static func category(_ kind: ActivityKind) -> (String, String) {
        switch kind {
        case .added: return ("added", "New comment")
        case .replied: return ("replied", "New reply")
        case .resolved: return ("resolved", "Comment resolved")
        case .reopened: return ("reopened", "Comment reopened")
        case .deleted: return ("deleted", "Comment deleted")
        }
    }

    /// Becomes the center's delegate, to route clicks, before launch
    /// finishes (a click can launch the app).
    func start() {
        guard !scripted else { return }
        center.delegate = self
        let kinds: [ActivityKind] = [.added, .replied, .resolved, .reopened, .deleted]
        center.setNotificationCategories(Set(kinds.map { kind in
            let (id, placeholder) = Notifier.category(kind)
            return UNNotificationCategory(identifier: id, actions: [], intentIdentifiers: [],
                                          hiddenPreviewsBodyPlaceholder: placeholder, options: [])
        }))
    }

    /// Margin is active and `w` is its key window.
    private func looking(at w: DocumentWindow) -> Bool {
        scripted ? scriptedLooking : NSApp.isActive && w.window?.isKeyWindow == true
    }

    /// Posts `activity` on `w`'s document, unless the person is looking at it.
    func post(_ activity: [ThreadActivity], in w: DocumentWindow) {
        if NSApp.isActive { askOnce() }
        if looking(at: w) { return }
        if scripted {
            scriptedPosted.insert(w.path)
            for a in activity {
                print("notification \(w.displayName) | \(a.headline) | \(a.message ?? "-")")
            }
            return
        }
        let path = w.path, title = w.displayName
        center.getNotificationSettings { settings in
            guard [.authorized, .provisional].contains(settings.authorizationStatus) else { return }
            for a in activity {
                let content = UNMutableNotificationContent()
                content.title = title
                content.subtitle = a.headline
                content.body = a.message ?? ""
                content.threadIdentifier = path
                content.categoryIdentifier = Notifier.category(a.kind).0
                content.sound = .default
                var info: [String: Any] = ["path": path]
                if a.kind != .deleted { info["thread"] = NSNumber(value: a.id) }
                content.userInfo = info
                self.center.add(UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
            }
        }
    }

    /// Asks for permission if it hasn't been asked yet.
    private func askOnce() {
        guard !scripted else { return }
        center.getNotificationSettings { settings in
            guard settings.authorizationStatus == .notDetermined else { return }
            self.center.requestAuthorization(options: [.alert, .sound]) { _, _ in }
        }
    }

    /// Removes the notifications about `path`, which the person now sees.
    func clear(path: String) {
        if scripted {
            if scriptedLooking, scriptedPosted.remove(path) != nil {
                print("notifications cleared for \((path as NSString).lastPathComponent)")
            }
            return
        }
        center.getDeliveredNotifications { delivered in
            let ids = delivered.filter { $0.request.content.threadIdentifier == path }.map(\.request.identifier)
            if !ids.isEmpty { self.center.removeDeliveredNotifications(withIdentifiers: ids) }
        }
    }

    // MARK: - UNUserNotificationCenterDelegate

    /// Margin became frontmost after posting: show the notification unless
    /// its document's window is the one in front.
    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void) {
        let path = notification.request.content.threadIdentifier
        DispatchQueue.main.async {
            let seen = AppDelegate.shared.windows.contains { $0.path == path && self.looking(at: $0) }
            done(seen ? [] : [.banner, .list, .sound])
        }
    }

    /// A click brings the document forward, focused on the thread it's about.
    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler done: @escaping () -> Void) {
        let info = response.notification.request.content.userInfo
        DispatchQueue.main.async {
            defer { done() }
            guard response.actionIdentifier == UNNotificationDefaultActionIdentifier,
                  let path = info["path"] as? String,
                  let w = AppDelegate.shared.open(path: path) else { return }
            w.window?.deminiaturize(nil)
            NSApp.activate()
            if let id = (info["thread"] as? NSNumber)?.uint64Value { w.layer.reveal(id) }
        }
    }
}
