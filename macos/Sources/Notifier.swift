import AppKit
import UserNotifications
import margin_ffi

/// Where notifications about agent activity go.
protocol NotificationOutlet {
    /// Asks for permission to post, if it hasn't been asked yet.
    func askOnce()
    /// One notification per change, titled `title`, about the document at
    /// `path`.
    func post(_ activity: [ThreadActivity], title: String, path: String)
    /// Removes the notifications about the document at `path`.
    func clear(path: String)
}

/// Notifications for agent activity on documents the person isn't looking
/// at: one per thread change, grouped by document, removed once the
/// document's window is key again. Permission is asked the first time
/// activity is announced while Margin is frontmost; until then nothing is
/// posted.
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    static let shared = Notifier()

    /// The system's notification center, unless replaced.
    var outlet: NotificationOutlet = SystemNotifications()
    /// Whether the person is looking at a document: Margin is active and
    /// its window is key.
    var isLooking: (DocumentWindow) -> Bool = { NSApp.isActive && $0.window?.isKeyWindow == true }

    private var center: UNUserNotificationCenter { .current() }

    /// Category identifiers, and what shows when previews are hidden.
    fileprivate static func category(_ kind: ActivityKind) -> (String, String) {
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
        center.delegate = self
        let kinds: [ActivityKind] = [.added, .replied, .resolved, .reopened, .deleted]
        center.setNotificationCategories(
            Set(
                kinds.map { kind in
                    let (id, placeholder) = Notifier.category(kind)
                    return UNNotificationCategory(
                        identifier: id, actions: [], intentIdentifiers: [],
                        hiddenPreviewsBodyPlaceholder: placeholder, options: [])
                }))
    }

    /// Posts `activity` on `w`'s document, unless the person is looking at it.
    func post(_ activity: [ThreadActivity], in w: DocumentWindow) {
        if NSApp.isActive { outlet.askOnce() }
        if isLooking(w) { return }
        outlet.post(activity, title: w.displayName, path: w.path)
    }

    /// Removes the notifications about `path`, which the person now sees.
    func clear(path: String) {
        outlet.clear(path: path)
    }

    // MARK: - UNUserNotificationCenterDelegate

    // The center may call these off the main thread; being async, they
    // hop to the main actor rather than trap.

    /// Margin became frontmost after posting: show the notification unless
    /// its document's window is the one in front.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        let path = notification.request.content.threadIdentifier
        let seen = AppDelegate.shared.windows.contains { $0.path == path && isLooking($0) }
        return seen ? [] : [.banner, .list, .sound]
    }

    /// A click brings the document forward, focused on the thread it's about.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse
    ) async {
        let info = response.notification.request.content.userInfo
        guard response.actionIdentifier == UNNotificationDefaultActionIdentifier,
            let path = info["path"] as? String,
            let w = AppDelegate.shared.open(path: path)
        else { return }
        w.window?.deminiaturize(nil)
        NSApp.activate()
        if let id = (info["thread"] as? NSNumber)?.uint64Value { w.layer.reveal(id) }
    }
}

/// The system's notification center.
struct SystemNotifications: NotificationOutlet {
    private var center: UNUserNotificationCenter { .current() }

    func post(_ activity: [ThreadActivity], title: String, path: String) {
        Task {
            let settings = await center.notificationSettings()
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
                try? await center.add(
                    UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
            }
        }
    }

    func askOnce() {
        Task {
            guard await center.notificationSettings().authorizationStatus == .notDetermined else { return }
            _ = try? await center.requestAuthorization(options: [.alert, .sound])
        }
    }

    func clear(path: String) {
        Task {
            let delivered = await center.deliveredNotifications()
            let ids = delivered.filter { $0.request.content.threadIdentifier == path }.map(\.request.identifier)
            if !ids.isEmpty { center.removeDeliveredNotifications(withIdentifiers: ids) }
        }
    }
}
