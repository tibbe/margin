import Foundation

/// Calls `handler` when a file changes, however it is written: in place,
/// or replaced by a rename (as editors and agents usually save). Watches
/// the file and its folder, and re-opens the file after it is replaced.
final class FileWatcher {
    private let path: String
    private let handler: () -> Void
    private var fileSource: DispatchSourceFileSystemObject?
    private var dirSource: DispatchSourceFileSystemObject?

    init(path: String, handler: @escaping () -> Void) {
        self.path = path
        self.handler = handler
        watchDirectory()
        watchFile()
    }

    deinit { cancel() }

    func cancel() {
        fileSource?.cancel()
        fileSource = nil
        dirSource?.cancel()
        dirSource = nil
    }

    private func source(
        for p: String, mask: DispatchSource.FileSystemEvent,
        on event: @escaping (DispatchSource.FileSystemEvent) -> Void
    ) -> DispatchSourceFileSystemObject? {
        let fd = open(p, O_EVTONLY)
        guard fd >= 0 else { return nil }
        let s = DispatchSource.makeFileSystemObjectSource(fileDescriptor: fd, eventMask: mask, queue: .main)
        s.setEventHandler { [weak s] in event(s?.data ?? []) }
        s.setCancelHandler { close(fd) }
        s.resume()
        return s
    }

    private func watchFile() {
        fileSource?.cancel()
        fileSource = source(for: path, mask: [.write, .extend, .delete, .rename, .attrib]) { [weak self] ev in
            guard let self else { return }
            if ev.contains(.delete) || ev.contains(.rename) {
                self.fileSource?.cancel()
                self.fileSource = nil
            }
            self.handler()
        }
    }

    private func watchDirectory() {
        let dir = (path as NSString).deletingLastPathComponent
        dirSource?.cancel()
        dirSource = source(for: dir, mask: [.write, .delete, .rename]) { [weak self] ev in
            guard let self else { return }
            if ev.contains(.delete) || ev.contains(.rename) {
                // The folder itself moved or went away: watch its path
                // again once something is there.
                self.dirSource?.cancel()
                self.dirSource = nil
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in self?.rewatch() }
                return
            }
            if self.fileSource == nil { self.watchFile() }
            self.handler()
        }
    }

    private func rewatch() {
        if dirSource == nil { watchDirectory() }
        if dirSource == nil {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in self?.rewatch() }
            return
        }
        watchFile()
        handler()
    }
}
