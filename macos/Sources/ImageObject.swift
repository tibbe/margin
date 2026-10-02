import AppKit
import margin_ffi

/// An image block shown as the image: one object, which takes its line.
/// Its first character is laid out as the space the image is drawn in, and
/// the rest of its line is hidden, so the cursor never rests in its source.
struct ImageObject {
    /// Its place in the analysis's image blocks.
    let index: Int
    let info: ImageBlockInfo
    let source: ImageSource
    let line: Int
    /// Its line's end: spaces after its source are part of it.
    let end: Int

    /// The first character of its source.
    var start: Int { Int(info.range.start) }
    /// The end of its source.
    var sourceEnd: Int { Int(info.range.end) }
    /// What selecting it selects.
    var range: NSRange { NSRange(location: start, length: end - start) }

    /// Whether `p` is on it, its edges included.
    func contains(_ p: Int) -> Bool { start <= p && p <= end }
}

/// How an image looks now.
enum ObjectLook {
    /// The image, at the size it is drawn.
    case image(LoadedImage, NSSize)
    /// Its size isn't known yet: a box holds its place.
    case placeholder(NSSize)
    /// It can't be shown: its alt text after the broken-image mark, at text
    /// size.
    case broken(BrokenImage)

    var size: NSSize {
        switch self {
        case .image(_, let size), .placeholder(let size): size
        case .broken(let b): NSSize(width: b.width, height: b.ascent + b.descent)
        }
    }

    /// Whether it sits in a line of text, at text size, rather than taking
    /// a line as tall as itself.
    var isText: Bool {
        if case .broken = self { true } else { false }
    }

    /// The size an image shows at: its own (in points, by its resolution),
    /// scaled with the zoom, never wider than `room` and never stretched to
    /// fill it.
    static func size(of image: LoadedImage, zoom: CGFloat, room: CGFloat) -> NSSize {
        var w = image.size.width * zoom
        var h = image.size.height * zoom
        if w > room {
            h *= room / w
            w = room
        }
        return NSSize(width: max(1, w.rounded()), height: max(1, h.rounded()))
    }

    /// Draws it in `rect`, for `object`: under the highlights (comments,
    /// find) that cover it, and the selection's tint when `selection` is
    /// its color. An image is drawn from a decoding `scale` pixels per point
    /// (see `ImageLibrary.drawable`); a placeholder holds its place until
    /// that is ready, unless it `canWait` not, as when printing.
    func draw(
        in rect: NSRect, for object: ImageObject, highlights: [(NSRange, NSColor)], selection: NSColor?,
        scale: CGFloat, canWait: Bool
    ) {
        let covering = highlights.filter { NSIntersectionRange($0.0, object.range).length > 0 }.map(\.1)
        let tints = covering + [selection].compactMap { $0 }
        switch self {
        case .image:
            let pixels = Int(ceil(max(rect.width, rect.height) * (canWait ? scale : max(scale, 3))))
            if let image = ImageLibrary.shared.drawable(object.source, pixels: pixels, now: !canWait) {
                image.draw(
                    in: rect, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true,
                    hints: [.interpolation: NSImageInterpolation.high.rawValue])
                for c in tints { tint(rect, c) }
            } else {
                drawBox(rect, tints)
            }
        case .placeholder:
            drawBox(rect, tints)
        case .broken(let b):
            b.draw(in: rect, for: object, highlights: highlights, selection: selection)
        }
    }

    /// A box on the code background, as a placeholder is drawn.
    private func drawBox(_ rect: NSRect, _ tints: [NSColor]) {
        let box = NSBezierPath(roundedRect: rect, xRadius: 6, yRadius: 6)
        Theme.codeBackground.setFill()
        box.fill()
        NSGraphicsContext.saveGraphicsState()
        box.addClip()
        for c in tints { tint(rect, c) }
        NSGraphicsContext.restoreGraphicsState()
    }

    /// The color over an image, see-through so the image shows.
    private func tint(_ rect: NSRect, _ color: NSColor) {
        let c = color.usingColorSpace(.sRGB) ?? color
        c.withAlphaComponent(c.alphaComponent < 1 ? c.alphaComponent : 0.45).setFill()
        rect.fill(using: .sourceOver)
    }
}

/// A broken image as drawn: the broken-image mark, then its alt text (its
/// file name, without one) as alt text shows, in italic link color, on one
/// line, cut short to fit.
struct BrokenImage {
    /// Why it can't be shown.
    let reason: String
    let text: NSAttributedString
    /// Where the text's parts are in the source, by their offsets in the
    /// text; none for a file name.
    private let pieces: [(source: NSRange, offset: Int)]
    private let mark: NSImage?
    private let markWidth: CGFloat
    private let gap: CGFloat
    let width: CGFloat
    let ascent: CGFloat
    let descent: CGFloat

    /// `alt` is the alt text's shown parts and where they are in the
    /// source; `room` the width there is.
    init(alt: [(NSRange, String)], fileName: String, reason: String, room: CGFloat) {
        self.reason = reason
        let font = Theme.font(size: Theme.bodySize, italic: true)
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: Theme.link]
        let joined = alt.map(\.1).joined()
        if joined.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            text = NSAttributedString(string: fileName, attributes: attrs)
            pieces = []
        } else {
            text = NSAttributedString(string: joined, attributes: attrs)
            var offset = 0
            pieces = alt.map { r, s in
                defer { offset += (s as NSString).length }
                return (r, offset)
            }
        }
        let config = NSImage.SymbolConfiguration(pointSize: Theme.bodySize, weight: .regular)
            .applying(NSImage.SymbolConfiguration(hierarchicalColor: Theme.dim))
        mark = NSImage(systemSymbolName: "photo.badge.exclamationmark", accessibilityDescription: "Broken image")?
            .withSymbolConfiguration(config)
        markWidth = ceil(mark?.size.width ?? 0)
        gap = (4 * Theme.scale).rounded()
        width = min(room, ceil(markWidth + gap + text.size().width))
        ascent = font.ascender
        descent = -font.descender
    }

    func draw(in rect: NSRect, for object: ImageObject, highlights: [(NSRange, NSColor)], selection: NSColor?) {
        let textX = rect.minX + markWidth + gap
        if let selection {
            selection.setFill()
            rect.fill()
        } else {
            for (r, c) in highlights where NSIntersectionRange(r, object.range).length > 0 {
                c.setFill()
                // Over its start, it is on the whole image: a comment.
                if NSLocationInRange(object.start, r) {
                    rect.fill()
                } else if let (a, b) = span(of: r) {
                    NSRect(x: textX + a, y: rect.minY, width: b - a, height: rect.height).intersection(rect).fill()
                }
            }
        }
        if let mark {
            let m = NSRect(
                x: rect.minX, y: (rect.midY - mark.size.height / 2).rounded(), width: mark.size.width,
                height: mark.size.height)
            mark.draw(in: m, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
        }
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingTail
        let shown = NSMutableAttributedString(attributedString: text)
        shown.addAttribute(.paragraphStyle, value: style, range: NSRange(location: 0, length: shown.length))
        shown.draw(
            with: NSRect(x: textX, y: rect.minY, width: max(0, rect.maxX - textX), height: rect.height),
            options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }

    /// Where the text drawn for source range `r` is, from the text's start.
    private func span(of r: NSRange) -> (CGFloat, CGFloat)? {
        var found: (Int, Int)?
        for p in pieces {
            let i = NSIntersectionRange(r, p.source)
            guard i.length > 0 else { continue }
            let a = p.offset + i.location - p.source.location
            found = (min(found?.0 ?? a, a), max(found?.1 ?? 0, a + i.length))
        }
        guard let (a, b) = found else { return nil }
        let x = { (n: Int) in self.text.attributedSubstring(from: NSRange(location: 0, length: n)).size().width }
        return (x(a), x(b))
    }
}
