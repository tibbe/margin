import AppKit
import Network
import XCTest

@testable import MarginKit

extension Harness {
    /// Writes a PNG of one color, `width`×`height` pixels at 72 dpi (as many
    /// points), in the document's folder; its bytes.
    @discardableResult
    func png(_ name: String, _ width: Int, _ height: Int, _ color: NSColor = .red) throws -> Data {
        let data = Harness.png(width, height, color)
        let url = dir.appendingPathComponent(name)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: url)
        return data
    }

    static func png(_ width: Int, _ height: Int, _ color: NSColor = .red) -> Data {
        let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8, samplesPerPixel: 4,
            hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        rep.size = NSSize(width: width, height: height)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        color.setFill()
        NSRect(x: 0, y: 0, width: width, height: height).fill()
        NSGraphicsContext.restoreGraphicsState()
        return rep.representation(using: .png, properties: [:])!
    }

    /// Waits until no image is loading any more.
    func waitForImages(timeout: Double = 5, file: StaticString = #filePath, line: UInt = #line) {
        wait(
            until: {
                !view.imageLayoutPending
                    && view.objects.allSatisfy {
                        if case .loading = ImageLibrary.shared.state(of: $0.source) { false } else { true }
                    }
            }, timeout: timeout, "images", file: file, line: line)
        layOut()
    }

    /// Each image as shown: `image 200×100`, `placeholder 713×150`,
    /// `broken "alt"`.
    var images: [String] {
        view.ensureFresh()
        return view.objects.map { o in
            switch view.look(of: o) {
            case .image(_, let s): "image \(Int(s.width))×\(Int(s.height))"
            case .placeholder(let s): "placeholder \(Int(s.width))×\(Int(s.height))"
            case .broken(let b): "broken \(b.text.string.debugDescription)"
            }
        }
    }

    /// Where image `i` is drawn, in the text view.
    func imageRect(_ i: Int = 0) -> NSRect {
        view.ensureFresh()
        view.layoutManager?.ensureLayout(for: view.textContainer!)
        let o = view.objects[i]
        return view.objectRect(o, look: view.look(of: o)) ?? .zero
    }

    /// The color drawn at `p` in `v` (the text view by default), as
    /// `red`, `white` and the like, `reddish` for red under a tint, or
    /// its components.
    func color(at p: NSPoint, in v: NSView? = nil) -> String {
        let v = v ?? view
        view.layoutManager?.ensureLayout(for: view.textContainer!)
        let b = v.bounds
        guard let rep = v.bitmapImageRepForCachingDisplay(in: b) else { return "no bitmap" }
        // Images are decoded when first drawn: draw again once they are.
        v.cacheDisplay(in: b, to: rep)
        if ImageLibrary.shared.isDecoding {
            wait(until: { !ImageLibrary.shared.isDecoding }, timeout: 3, "decoding")
            v.cacheDisplay(in: b, to: rep)
        }
        let scale = CGFloat(rep.pixelsWide) / b.width
        guard let c = rep.colorAt(x: Int(p.x * scale), y: Int(p.y * scale))?.usingColorSpace(.sRGB) else {
            return "no color"
        }
        let (r, g, bl) = (c.redComponent, c.greenComponent, c.blueComponent)
        if r > 0.8 && g < 0.3 && bl < 0.3 { return "red" }
        if r > 0.95 && g > 0.95 && bl > 0.95 { return "white" }
        if r > max(g, bl) + 0.15 { return "reddish" }
        return String(format: "%.2f %.2f %.2f", r, g, bl)
    }

    /// The center of image `i`'s color.
    func imageColor(_ i: Int = 0) -> String {
        let r = imageRect(i)
        return color(at: NSPoint(x: r.midX, y: r.midY))
    }

    /// A click in the middle of image `i`.
    func click(image i: Int = 0) {
        let r = imageRect(i)
        click(view, at: NSPoint(x: r.midX, y: r.midY))
    }
}

/// Remote images held back until a test lets them load, so their
/// placeholders can be seen.
@MainActor
final class HeldImages {
    private var waiting: [URL: CheckedContinuation<Result<Data, ImageLoadError>, Never>] = [:]
    private var released: [URL: Data] = [:]

    init() {
        ImageLibrary.shared.fetch = { @MainActor [unowned self] source in
            guard case .remote(let url) = source else { return await ImageLibrary.read(source) }
            if let data = self.released[url] { return .success(data) }
            return await withCheckedContinuation { self.waiting[url] = $0 }
        }
    }

    /// Image `url` arrives, with `data`, now or once it is asked for.
    func release(_ url: String, _ data: Data) {
        let key = URL(string: url)!
        released[key] = data
        waiting.removeValue(forKey: key)?.resume(returning: .success(data))
    }
}

/// A web server on this machine's loopback only, for remote images: it
/// answers a GET with the file of that name in `dir`, else 404, and keeps
/// each request's User-Agent.
final class ImageServer: @unchecked Sendable {
    private let listener: NWListener
    private let dir: URL
    private let lock = NSLock()
    private var agentsSeen: [String] = []

    var agents: [String] { lock.withLock { agentsSeen } }
    var port: UInt16 { listener.port?.rawValue ?? 0 }

    init(serving dir: URL) throws {
        self.dir = dir
        let params = NWParameters.tcp
        params.requiredInterfaceType = .loopback
        params.acceptLocalOnly = true
        listener = try NWListener(using: params)
        listener.newConnectionHandler = { [weak self] c in self?.serve(c) }
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in
            if case .ready = state { ready.signal() }
        }
        listener.start(queue: .global())
        _ = ready.wait(timeout: .now() + 5)
    }

    func url(_ name: String) -> String { "http://127.0.0.1:\(port)/\(name)" }

    private func serve(_ c: NWConnection) {
        c.start(queue: .global())
        c.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, _, _ in
            guard let self else { return c.cancel() }
            let lines = String(decoding: data ?? Data(), as: UTF8.self).components(separatedBy: "\r\n")
            let path = lines.first?.split(separator: " ").dropFirst().first.map(String.init) ?? "/"
            let agent = lines.first { $0.lowercased().hasPrefix("user-agent:") }
                .map { $0.dropFirst("user-agent:".count).trimmingCharacters(in: .whitespaces) }
            self.lock.withLock { self.agentsSeen.append(agent ?? "") }
            let body = try? Data(contentsOf: self.dir.appendingPathComponent(String(path.dropFirst())))
            let head =
                body.map { "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: \($0.count)\r\n" }
                ?? "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n"
            c.send(
                content: Data((head + "Connection: close\r\n\r\n").utf8) + (body ?? Data()),
                completion: .contentProcessed { _ in c.cancel() })
        }
    }

    func stop() { listener.cancel() }
}
