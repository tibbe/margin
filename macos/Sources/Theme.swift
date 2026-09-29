import AppKit

/// Preferences that persist between runs, in the app's user defaults.
enum Prefs {
    /// Test runs keep their preferences apart, starting from the defaults.
    private static let defaults: UserDefaults = {
        guard scriptPath != nil else { return .standard }
        let suite = "io.github.tibbe.Margin.tests"
        UserDefaults.standard.removePersistentDomain(forName: suite)
        return UserDefaults(suiteName: suite) ?? .standard
    }()

    static var zoom: Double {
        get {
            let z = defaults.double(forKey: "zoom")
            return z > 0 ? z : 1
        }
        set { defaults.set(newValue, forKey: "zoom") }
    }

    /// Reflow Paragraphs: show line breaks inside paragraphs as spaces.
    static var reflow: Bool {
        get { defaults.bool(forKey: "reflowParagraphs") }
        set { defaults.set(newValue, forKey: "reflowParagraphs") }
    }

    static let zoomSteps: [Double] = [0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0]
}

/// Colors, fonts and measurements. Colors are the system's semantic colors,
/// so they follow the appearance, the accent color and accessibility
/// settings by themselves.
enum Theme {
    /// Body text size in points before zoom.
    static let baseBodySize: CGFloat = 15

    static var bodySize: CGFloat { baseBodySize * CGFloat(Prefs.zoom) }

    /// Spacing scale: the core's spacing is in pixels at a 16px body.
    static var scale: CGFloat { bodySize / 16 }

    /// Bumped whenever fonts or sizes change, so every line is restyled.
    static var generation = 0

    // Relative to body, H1 to H6.
    static let headingScales: [CGFloat] = [1.8, 1.42, 1.2, 1.07, 1.0, 0.94]
    static let headingWeights: [NSFont.Weight] = [.heavy, .bold, .bold, .semibold, .semibold, .semibold]

    static let quoteStep: CGFloat = 22
    static let itemStep: CGFloat = 28

    static var text: NSColor { .textColor }
    /// The text color, faded; it follows the appearance, as the text
    /// color does (`withAlphaComponent` alone fixes the appearance it was
    /// made in, so stored in the text it goes stale when that changes).
    static func text(alpha: CGFloat) -> NSColor {
        NSColor(name: nil) { appearance in
            var c = NSColor.textColor
            appearance.performAsCurrentDrawingAppearance { c = NSColor.textColor.usingColorSpace(.sRGB) ?? c }
            return c.withAlphaComponent(alpha)
        }
    }
    static var heading: NSColor { .labelColor }
    static var dim: NSColor { .secondaryLabelColor }
    static var border: NSColor { .separatorColor }
    static var accent: NSColor { .controlAccentColor }
    static var link: NSColor { .linkColor }
    static var codeBackground: NSColor { .quaternarySystemFill }
    static var cardBackground: NSColor { .controlBackgroundColor }

    /// Commented text: Apple's purple author color, not yellow, which is
    /// find's. In light mode Pages' own fills for it (its comment fill, and
    /// the stronger one it uses where ranges overlap, for the focused
    /// thread); Pages has no dark page, so in dark mode the author color
    /// itself (Notes' first participant color) at low alpha.
    static func highlight(active: Bool, dark: Bool) -> NSColor {
        func rgb(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> NSColor {
            NSColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: a)
        }
        if dark { return rgb(164, 119, 236, active ? 0.4 : 0.2) }
        return active ? rgb(220, 175, 253) : rgb(237, 209, 254)
    }

    /// `top` painted over `bottom`, as one color, in `appearance`.
    static func composite(_ top: NSColor, over bottom: NSColor, in appearance: NSAppearance) -> NSColor {
        var t: NSColor?, b: NSColor?
        appearance.performAsCurrentDrawingAppearance {
            t = top.usingColorSpace(.sRGB)
            b = bottom.usingColorSpace(.sRGB)
        }
        guard let t, let b else { return bottom }
        let a = t.alphaComponent + b.alphaComponent * (1 - t.alphaComponent)
        guard a > 0 else { return bottom }
        func mix(_ x: CGFloat, _ y: CGFloat) -> CGFloat {
            (x * t.alphaComponent + y * b.alphaComponent * (1 - t.alphaComponent)) / a
        }
        return NSColor(srgbRed: mix(t.redComponent, b.redComponent), green: mix(t.greenComponent, b.greenComponent),
                       blue: mix(t.blueComponent, b.blueComponent), alpha: a)
    }

    static func findMatch(current: Bool) -> NSColor {
        NSColor.findHighlightColor.withAlphaComponent(current ? 0.9 : 0.35)
    }

    private struct FontKey: Hashable {
        var size: CGFloat
        var weight: CGFloat
        var italic: Bool
        var mono: Bool
    }

    private static var fonts: [FontKey: NSFont] = [:]

    static func font(size: CGFloat, weight: NSFont.Weight = .regular, italic: Bool = false, mono: Bool = false) -> NSFont {
        let key = FontKey(size: size, weight: weight.rawValue, italic: italic, mono: mono)
        if let f = fonts[key] { return f }
        var f = mono ? NSFont.monospacedSystemFont(ofSize: size, weight: weight) : NSFont.systemFont(ofSize: size, weight: weight)
        if italic {
            let d = f.fontDescriptor.withSymbolicTraits(f.fontDescriptor.symbolicTraits.union(.italic))
            f = NSFont(descriptor: d, size: size) ?? f
        }
        fonts[key] = f
        return f
    }

    static func invalidateFonts() {
        fonts.removeAll()
        generation += 1
    }

    /// The natural height of a font's line: ascent, descent and leading.
    static func naturalHeight(_ f: NSFont) -> CGFloat {
        ceil(f.ascender - f.descender + f.leading)
    }
}

/// Horizontal layout of the page: the text column and the comment gutter.
struct PageGeometry {
    var left: CGFloat
    var docWidth: CGFloat
    var gutterX: CGFloat
    var cardWidth: CGFloat

    static let cardBaseWidth: CGFloat = 300
    static let gutterGap: CGFloat = 40
    static let sidePad: CGFloat = 28

    /// About 100 characters of body text beside the gutter; narrower
    /// windows give the text what is left.
    init(width: CGFloat, scale: CGFloat) {
        let card = PageGeometry.cardBaseWidth
        let maxDoc = 760 * scale
        let room = width - card - PageGeometry.gutterGap - 2 * PageGeometry.sidePad
        let doc = min(max(room, 300), maxDoc, max(width - 2 * PageGeometry.sidePad, 120))
        var left = ((width - doc) / 2).rounded()
        let needed = doc + PageGeometry.gutterGap + card + PageGeometry.sidePad
        if left + needed > width {
            left = max(width - needed, PageGeometry.sidePad)
        }
        self.left = left
        docWidth = doc.rounded()
        gutterX = left + docWidth + PageGeometry.gutterGap
        cardWidth = card
    }

    /// The whole width for text, with no gutter (printing).
    init(fullWidth width: CGFloat) {
        left = 0
        docWidth = width
        gutterX = width
        cardWidth = 0
    }
}
