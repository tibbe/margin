import AppKit
import margin_ffi

/// An image or a diagram shown as such: one object, which takes its lines.
/// Its first character is laid out as the space it is drawn in, and the
/// rest of its lines hidden, so the cursor never rests in its source.
struct DocObject {
    enum Kind {
        case image(ImageBlockInfo)
        case diagram(DiagramBlockInfo)
    }

    let kind: Kind
    /// Its place in the analysis's image blocks, or its diagram blocks.
    let index: Int
    /// What it is drawn from: an image's file or URL, or a diagram's source
    /// in the page's colors.
    let source: ImageSource
    /// Its first line, and its last.
    let line: Int
    let lastLine: Int
    /// Its first character, and the end of its source.
    let start: Int
    let sourceEnd: Int
    /// Its last line's end: spaces after an image's source are part of it.
    let end: Int

    /// What selecting it selects.
    var range: NSRange { NSRange(location: start, length: end - start) }

    var isDiagram: Bool {
        if case .diagram = kind { true } else { false }
    }

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
    /// A diagram that can't be drawn: why, and its source.
    case diagramError(DiagramError)

    var size: NSSize {
        switch self {
        case .image(_, let size), .placeholder(let size): size
        case .broken(let b): NSSize(width: b.width, height: b.ascent + b.descent)
        case .diagramError(let e): NSSize(width: e.width, height: e.height)
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
    /// A diagram's `labels`, by index, are find matches, in their color.
    func draw(
        in rect: NSRect, for object: DocObject, highlights: [(NSRange, NSColor)], selection: NSColor?,
        labels: [(Int, NSColor)] = [], scale: CGFloat, canWait: Bool
    ) {
        let covering = highlights.filter { NSIntersectionRange($0.0, object.range).length > 0 }.map(\.1)
        let tints = covering + [selection].compactMap { $0 }
        switch self {
        case .image(let loaded, _):
            let pixels = Int(ceil(max(rect.width, rect.height) * (canWait ? scale : max(scale, 3))))
            if let image = ImageLibrary.shared.drawable(object.source, pixels: pixels, now: !canWait) {
                image.draw(
                    in: rect, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true,
                    hints: [.interpolation: NSImageInterpolation.high.rawValue])
                for c in tints { tint(rect, c) }
                // Find matches in a diagram's labels, over them.
                let sx = rect.width / max(1, loaded.size.width)
                let sy = rect.height / max(1, loaded.size.height)
                for (i, color) in labels where i < loaded.labels.count {
                    let l = loaded.labels[i]
                    color.setFill()
                    NSBezierPath(
                        roundedRect: NSRect(
                            x: rect.minX + CGFloat(l.x) * sx, y: rect.minY + CGFloat(l.y) * sy,
                            width: CGFloat(l.width) * sx, height: CGFloat(l.height) * sy
                        ).insetBy(dx: -2, dy: -1), xRadius: 2, yRadius: 2
                    ).fill()
                }
            } else {
                drawBox(rect, tints)
            }
        case .placeholder:
            drawBox(rect, tints)
        case .broken(let b):
            b.draw(in: rect, for: object, highlights: highlights, selection: selection)
        case .diagramError(let e):
            e.draw(in: rect, for: object, highlights: highlights, selection: selection)
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

    func draw(in rect: NSRect, for object: DocObject, highlights: [(NSRange, NSColor)], selection: NSColor?) {
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

/// A diagram that can't be drawn, as it is shown: a box on the code
/// background with the error mark and why, in the error color, then the
/// diagram's source, the line at fault tinted, so it can be fixed in Show
/// Markdown.
struct DiagramError {
    let message: String
    /// The source's lines: where each is in the document, and its text.
    let lines: [(source: NSRange, text: String)]
    /// The line at fault, from 0.
    let fault: Int?
    let width: CGFloat
    let height: CGFloat
    private let pad: CGFloat
    private let headerHeight: CGFloat
    private let lineHeight: CGFloat
    private let font: NSFont
    private let headerFont: NSFont
    private let mark: NSImage?

    init(message: String, lines: [(source: NSRange, text: String)], fault: Int?, room: CGFloat) {
        self.message = message
        self.lines = lines
        self.fault = fault
        let s = Theme.scale
        pad = (10 * s).rounded()
        font = Theme.font(size: Theme.bodySize * 0.88, mono: true)
        headerFont = Theme.font(size: Theme.bodySize * 0.88)
        lineHeight = ceil(Theme.naturalHeight(font) * 1.2)
        headerHeight = ceil(Theme.naturalHeight(headerFont) * 1.2)
        let config = NSImage.SymbolConfiguration(pointSize: Theme.bodySize * 0.88, weight: .regular)
            .applying(NSImage.SymbolConfiguration(hierarchicalColor: Theme.error))
        mark = NSImage(systemSymbolName: "exclamationmark.triangle.fill", accessibilityDescription: "Error")?
            .withSymbolConfiguration(config)
        width = room
        height = pad + headerHeight + (6 * s).rounded() + CGFloat(lines.count) * lineHeight + pad
    }

    /// Where line `i` of the source is drawn in `rect`.
    private func lineRect(_ i: Int, in rect: NSRect) -> NSRect {
        let top = rect.minY + pad + headerHeight + (6 * Theme.scale).rounded()
        return NSRect(x: rect.minX, y: top + CGFloat(i) * lineHeight, width: rect.width, height: lineHeight)
    }

    func draw(in rect: NSRect, for object: DocObject, highlights: [(NSRange, NSColor)], selection: NSColor?) {
        let box = NSBezierPath(roundedRect: rect, xRadius: 6, yRadius: 6)
        Theme.codeBackground.setFill()
        box.fill()
        NSGraphicsContext.saveGraphicsState()
        box.addClip()
        if let fault, fault < lines.count {
            Theme.error.withAlphaComponent(0.15).setFill()
            lineRect(fault, in: rect).fill()
        }
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: Theme.text]
        for (r, c) in highlights where NSIntersectionRange(r, object.range).length > 0 {
            c.setFill()
            // Over its start, it is on the whole diagram: a comment.
            if NSLocationInRange(object.start, r) {
                rect.fill(using: .sourceOver)
                continue
            }
            for (i, l) in lines.enumerated() {
                let hit = NSIntersectionRange(r, l.source)
                guard hit.length > 0 else { continue }
                let text = l.text as NSString
                let x = { (n: Int) in text.substring(to: min(n, text.length)).size(withAttributes: attrs).width }
                let a = x(hit.location - l.source.location)
                let b = x(NSMaxRange(hit) - l.source.location)
                let lr = lineRect(i, in: rect)
                NSRect(x: lr.minX + pad + a, y: lr.minY, width: b - a, height: lr.height).fill(using: .sourceOver)
            }
        }
        if let selection {
            selection.withAlphaComponent(0.45).setFill()
            rect.fill(using: .sourceOver)
        }
        var x = rect.minX + pad
        if let mark {
            let m = NSRect(
                x: x, y: (rect.minY + pad + (headerHeight - mark.size.height) / 2).rounded(),
                width: mark.size.width, height: mark.size.height)
            mark.draw(in: m, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
            x += mark.size.width + (6 * Theme.scale).rounded()
        }
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingTail
        (message as NSString).draw(
            with: NSRect(x: x, y: rect.minY + pad, width: max(0, rect.maxX - pad - x), height: headerHeight),
            options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine],
            attributes: [.font: headerFont, .foregroundColor: Theme.error, .paragraphStyle: style])
        for (i, l) in lines.enumerated() {
            let lr = lineRect(i, in: rect)
            (l.text as NSString).draw(
                with: NSRect(x: lr.minX + pad, y: lr.minY, width: max(0, lr.width - 2 * pad), height: lr.height),
                options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine],
                attributes: attrs.merging([.paragraphStyle: style]) { $1 })
        }
        NSGraphicsContext.restoreGraphicsState()
    }
}
