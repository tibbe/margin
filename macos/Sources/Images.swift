import AppKit
import ImageIO
import margin_ffi

/// Where an image's path or URL points.
enum ImageSource: Hashable {
    /// A file, by its absolute path.
    case file(String)
    /// An image fetched over the network.
    case remote(URL)
    /// Nothing Margin can load: what was written, and why.
    case unusable(written: String, reason: String)
    /// A Mermaid diagram's source, drawn in the page's colors.
    case diagram(String, DiagramColors)

    /// The source of an image whose destination is `written`, in a document
    /// in `folder`. A relative path is from the document's folder, and one
    /// starting with `/` from the file system's root.
    static func resolve(_ written: String, from folder: String) -> ImageSource {
        let dest = written.trimmingCharacters(in: .whitespaces)
        if dest.isEmpty { return .unusable(written: written, reason: "The image has no path.") }
        // A scheme has two letters or more; one is a Windows drive.
        if let url = URL(string: dest), let scheme = url.scheme?.lowercased(), scheme.count > 1 {
            switch scheme {
            case "http", "https", "data": return .remote(url)
            case "file": return .file(url.standardizedFileURL.path)
            default: return .unusable(written: dest, reason: "Margin can’t load images from “\(scheme):” links.")
            }
        }
        let path = dest.removingPercentEncoding ?? dest
        let full = path.hasPrefix("/") ? path : (folder as NSString).appendingPathComponent(path)
        return .file(URL(fileURLWithPath: full).standardized.path)
    }

    /// The path or URL, as hovering a broken image shows it.
    var location: String {
        switch self {
        case .file(let path): path
        case .remote(let url): url.absoluteString
        case .unusable(let written, _): written
        case .diagram: "diagram"
        }
    }

    /// The image's file name, shown for a broken image without alt text.
    var fileName: String {
        switch self {
        case .file(let path): (path as NSString).lastPathComponent
        case .remote(let url):
            url.lastPathComponent.isEmpty ? (url.host() ?? url.absoluteString) : url.lastPathComponent
        case .unusable(let written, _): (written as NSString).lastPathComponent
        case .diagram: "diagram"
        }
    }
}

/// A loaded image: its size, for layout, and its file's bytes, decoded
/// only to be drawn (see `ImageLibrary.drawable`).
struct LoadedImage {
    /// Its own size, in points: its pixels at its resolution.
    let size: NSSize
    /// Its pixels on its longer side; zero for a vector image.
    let pixels: Int
    let data: Data
    /// A vector image (SVG, PDF), drawn as is at any size.
    let vector: NSImage?
    /// A drawn diagram's SVG, which the core turns into pixels at any size,
    /// and the labels it draws, where, in its own points.
    let svg: String?
    let labels: [DiagramLabel]

    /// A drawn diagram, `size` points.
    init(svg: String, size: NSSize, labels: [DiagramLabel]) {
        self.svg = svg
        self.size = size
        self.labels = labels
        pixels = 0
        data = Data()
        vector = nil
    }

    /// Its size and kind from its header, without decoding it; nil if it
    /// isn't an image.
    init?(_ data: Data) {
        self.data = data
        svg = nil
        labels = []
        if let src = CGImageSourceCreateWithData(data as CFData, nil), CGImageSourceGetCount(src) > 0,
            let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any],
            let w = props[kCGImagePropertyPixelWidth] as? Int, let h = props[kCGImagePropertyPixelHeight] as? Int,
            w > 0, h > 0
        {
            // EXIF orientations 5 to 8 turn it on its side.
            let turned = (props[kCGImagePropertyOrientation] as? Int).map { $0 >= 5 } ?? false
            let dpi = props[kCGImagePropertyDPIWidth] as? Double ?? 72
            let points = 72 / max(dpi, 1)
            size = NSSize(width: Double(turned ? h : w) * points, height: Double(turned ? w : h) * points)
            pixels = max(w, h)
            vector = nil
            return
        }
        // ImageIO doesn't read every format (SVG); NSImage draws those at
        // any size, from their description.
        guard let image = NSImage(data: data), image.isValid, image.size.width > 0, image.size.height > 0 else {
            return nil
        }
        size = image.size
        pixels = 0
        vector = image
    }
}

/// How far an image has got.
enum ImageState {
    /// Its size isn't known yet: a placeholder holds its place.
    case loading
    case loaded(LoadedImage)
    /// It can't be shown, and why.
    case failed(String)
    /// A diagram that can't be drawn: why, and the line of its source at
    /// fault (from 0), when known.
    case unrenderable(String, line: Int?)
}

/// Loads images, local and remote, without blocking the window, and keeps
/// them for the app's run, shared by every window. A change to an image's
/// state is posted as `changed`, and an image decoded for drawing as
/// `decoded`, with the source under `sourceKey`.
///
/// A load reads only an image's size; it is decoded when drawn, at the
/// size drawn, into a cache of `budget` bytes that forgets the images
/// drawn least recently. So a document of many large images costs the
/// memory of those on screen, not of all of them at full size.
final class ImageLibrary {
    static let shared = ImageLibrary()
    static let changed = Notification.Name("MarginImageChanged")
    static let decoded = Notification.Name("MarginImageDecoded")
    static let sourceKey = "source"

    /// How many bytes of decoded images to keep.
    var budget = 150_000_000
    private var images: [ImageSource: (image: NSImage, pixels: Int, bytes: Int, used: Int)] = [:]
    private var decoding = Set<ImageSource>()
    private var clock = 0
    /// The bytes of decoded images kept now.
    private(set) var decodedBytes = 0
    /// Whether an image is being decoded for drawing.
    var isDecoding: Bool { !decoding.isEmpty }

    /// Reads an image's bytes: a file's, or a server's answer; on failure,
    /// says why. Tests stand in for it.
    var fetch: @MainActor (ImageSource) async -> Result<Data, ImageLoadError> = ImageLibrary.read

    private var states: [ImageSource: ImageState] = [:]
    /// When each loaded file was last modified, to notice it change.
    private var modified: [String: Date] = [:]
    private var waiters: [(sources: Set<ImageSource>, done: () -> Void)] = []

    /// The image's state; one not asked for before starts loading.
    func state(of source: ImageSource) -> ImageState {
        if let s = states[source] { return s }
        load(source)
        return states[source] ?? .loading
    }

    /// Runs `done` once none of `sources` is loading any more.
    func whenSettled(_ sources: [ImageSource], _ done: @escaping () -> Void) {
        let pending = Set(sources.filter { if case .loading = state(of: $0) { true } else { false } })
        if pending.isEmpty { return done() }
        waiters.append((pending, done))
    }

    /// Loads again the files that failed or have changed since they
    /// loaded: a missing image may have been put in place.
    func recheck() {
        for (source, state) in states {
            guard case .file(let path) = source else { continue }
            switch state {
            case .failed: load(source)
            case .loaded where ImageLibrary.modificationDate(path) != modified[path]: load(source)
            default: break
            }
        }
    }

    private func load(_ source: ImageSource) {
        if case .unusable(_, let reason) = source {
            states[source] = .failed(reason)
            return
        }
        if case .diagram(let text, let colors) = source {
            states[source] = .loading
            let colors = Colors(colors)
            Task.detached(priority: .userInitiated) {
                let outcome = Outcome(renderDiagram(source: text, colors: colors.colors))
                await MainActor.run { self.settle(source, outcome.state) }
            }
            return
        }
        // A reload keeps showing what it has until it is done.
        if states[source] == nil { states[source] = .loading }
        Task {
            let state: ImageState
            switch await fetch(source) {
            case .success(let data):
                if let image = LoadedImage(data) {
                    state = .loaded(image)
                } else {
                    state = .failed("It isn’t an image Margin can read.")
                }
            case .failure(let error):
                state = .failed(error.reason)
            }
            if case .file(let path) = source { modified[path] = ImageLibrary.modificationDate(path) }
            settle(source, state)
        }
    }

    private func settle(_ source: ImageSource, _ state: ImageState) {
        states[source] = state
        forget(source)
        NotificationCenter.default.post(
            name: ImageLibrary.changed, object: self, userInfo: [ImageLibrary.sourceKey: source])
        for i in waiters.indices.reversed() {
            waiters[i].sources.remove(source)
            if waiters[i].sources.isEmpty { waiters.remove(at: i).done() }
        }
    }

    /// The image to draw `source` with, decoded with `pixels` on its longer
    /// side. Decoding happens off the main thread, and posts `decoded`
    /// when done; until then this is nil, or a smaller decoding. With
    /// `now`, as when printing, it decodes at once instead.
    func drawable(_ source: ImageSource, pixels: Int, now: Bool) -> NSImage? {
        guard case .loaded(let l)? = states[source] else { return nil }
        if let v = l.vector { return v }
        // A diagram draws sharp at any size; an image no larger than it is.
        let want = l.svg != nil ? max(1, pixels) : max(1, min(pixels, l.pixels))
        clock += 1
        let have = images[source]
        if have != nil { images[source]?.used = clock }
        if let have, have.pixels >= want { return have.image }
        if now {
            guard let cg = ImageLibrary.decode(l, pixels: want) else { return have?.image }
            return keep(source, cg.image, size: l.size, pixels: want)
        }
        if !decoding.contains(source) {
            decoding.insert(source)
            let data = l.data
            let job = Job(image: l)
            Task.detached(priority: .userInitiated) {
                let cg = ImageLibrary.decode(job.image, pixels: want)
                await MainActor.run {
                    self.decoding.remove(source)
                    // Loaded again meanwhile: that load's decoding is due.
                    guard case .loaded(let now)? = self.states[source], now.data == data, now.svg == job.image.svg,
                        let cg
                    else { return }
                    _ = self.keep(source, cg.image, size: now.size, pixels: want)
                    NotificationCenter.default.post(
                        name: ImageLibrary.decoded, object: self, userInfo: [ImageLibrary.sourceKey: source])
                }
            }
        }
        return have?.image
    }

    private func keep(_ source: ImageSource, _ cg: CGImage, size: NSSize, pixels: Int) -> NSImage {
        forget(source)
        let image = NSImage(cgImage: cg, size: size)
        let bytes = cg.bytesPerRow * cg.height
        images[source] = (image, pixels, bytes, clock)
        decodedBytes += bytes
        while decodedBytes > budget,
            let old = images.filter({ $0.key != source }).min(by: { $0.value.used < $1.value.used })
        {
            forget(old.key)
        }
        return image
    }

    private func forget(_ source: ImageSource) {
        if let old = images.removeValue(forKey: source) { decodedBytes -= old.bytes }
    }

    /// A decoded image, safe to hand from the thread that decoded it.
    struct Decoded: @unchecked Sendable {
        let image: CGImage
    }

    /// A loaded image, to decode on another thread.
    struct Job: @unchecked Sendable {
        let image: LoadedImage
    }

    /// The page's colors, to draw a diagram in on another thread.
    struct Colors: @unchecked Sendable {
        let colors: DiagramColors
        nonisolated init(_ colors: DiagramColors) { self.colors = colors }
    }

    /// What drawing a diagram gave, from another thread.
    struct Outcome: @unchecked Sendable {
        let outcome: DiagramOutcome
        nonisolated init(_ outcome: DiagramOutcome) { self.outcome = outcome }

        @MainActor var state: ImageState {
            switch outcome {
            case .drawn(let svg, let width, let height, let labels):
                .loaded(
                    LoadedImage(svg: svg, size: NSSize(width: Double(width), height: Double(height)), labels: labels))
            case .failed(_, let message, let line):
                .unrenderable(message, line: line.map { Int($0) })
            }
        }
    }

    /// Decodes an image with `pixels` on its longer side: a bitmap turned
    /// upright, as Apple recommends for showing large images smaller, or a
    /// diagram drawn by the core.
    nonisolated static func decode(_ image: LoadedImage, pixels: Int) -> Decoded? {
        if let svg = image.svg {
            let scale = Double(pixels) / max(1, max(image.size.width, image.size.height))
            let w = max(1, Int((image.size.width * scale).rounded()))
            let h = max(1, Int((image.size.height * scale).rounded()))
            guard let rgba = rasterizeDiagram(svg: svg, width: UInt32(w), height: UInt32(h)),
                let provider = CGDataProvider(data: Data(rgba) as CFData),
                let cg = CGImage(
                    width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue), provider: provider,
                    decode: nil, shouldInterpolate: true, intent: .defaultIntent)
            else { return nil }
            return Decoded(image: cg)
        }
        guard let src = CGImageSourceCreateWithData(image.data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceThumbnailMaxPixelSize: pixels,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        return CGImageSourceCreateThumbnailAtIndex(src, 0, options as CFDictionary).map { Decoded(image: $0) }
    }

    private static func modificationDate(_ path: String) -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: path))?[.modificationDate] as? Date
    }

    /// No cookies or cache kept, and a User-Agent that says only "margin".
    private static let session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.httpAdditionalHeaders = ["User-Agent": "margin"]
        config.timeoutIntervalForRequest = 20
        return URLSession(configuration: config)
    }()

    /// Reads a file, or asks the server.
    static func read(_ source: ImageSource) async -> Result<Data, ImageLoadError> {
        switch source {
        case .file(let path):
            var isFolder: ObjCBool = false
            guard FileManager.default.fileExists(atPath: path, isDirectory: &isFolder) else {
                return .failure(ImageLoadError("There’s no file there."))
            }
            if isFolder.boolValue { return .failure(ImageLoadError("It’s a folder.")) }
            let read = Task.detached { () -> Result<Data, ImageLoadError> in
                do {
                    return .success(try Data(contentsOf: URL(fileURLWithPath: path), options: .mappedIfSafe))
                } catch {
                    return .failure(ImageLoadError(error.localizedDescription))
                }
            }
            return await read.value
        case .remote(let url):
            var request = URLRequest(url: url)
            request.setValue("margin", forHTTPHeaderField: "User-Agent")
            do {
                let (data, response) = try await session.data(for: request)
                if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                    let status = HTTPURLResponse.localizedString(forStatusCode: http.statusCode)
                    return .failure(ImageLoadError("The server answered \(http.statusCode) (\(status))."))
                }
                return .success(data)
            } catch {
                return .failure(ImageLoadError(error.localizedDescription))
            }
        case .unusable(_, let reason):
            return .failure(ImageLoadError(reason))
        case .diagram:
            return .failure(ImageLoadError("A diagram is drawn, not read."))
        }
    }
}

/// Why an image couldn't be loaded, as hovering it says.
nonisolated struct ImageLoadError: Error {
    let reason: String

    init(_ reason: String) { self.reason = reason }
}
