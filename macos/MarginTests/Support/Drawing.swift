import AppKit
import XCTest

@testable import MarginKit

/// What the window draws, read back from its views.
extension Harness {
    /// Draws the window's content (no title bar) as
    /// `macos/build/snapshots/NAME.png`, to look at while working.
    @discardableResult
    func snapshot(_ name: String) -> URL {
        let v = win.contentView!
        layOut()
        view.layoutManager?.ensureLayout(for: view.textContainer!)
        let rep = v.bitmapImageRepForCachingDisplay(in: v.bounds)!
        // Images are decoded when first drawn: draw again once they are.
        v.cacheDisplay(in: v.bounds, to: rep)
        if ImageLibrary.shared.isDecoding {
            wait(until: { !ImageLibrary.shared.isDecoding }, timeout: 3, "decoding")
            v.cacheDisplay(in: v.bounds, to: rep)
        }
        // From this file, in macos/MarginTests/Support/.
        let macos = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent()
        let url = macos.appendingPathComponent("build/snapshots/\(name).png")
        try! FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try! rep.representation(using: .png, properties: [:])!.write(to: url)
        return url
    }

    /// The fill drawn behind each shown character of `s`, sampled just above
    /// its x-height (so pick letters without ascenders), as runs of
    /// characters filled alike: `"oov":a " now":b`.
    func fills(_ s: String) -> String {
        let r = (view.string as NSString).range(of: s)
        guard r.location != NSNotFound, let lm = view.layoutManager, let tc = view.textContainer,
            let storage = view.textStorage
        else { return "\(s) not found" }
        lm.ensureLayout(for: tc)
        let b = view.visibleRect
        guard let rep = view.bitmapImageRepForCachingDisplay(in: b) else { return "no bitmap" }
        view.cacheDisplay(in: b, to: rep)
        let scale = CGFloat(rep.pixelsWide) / b.width
        let origin = view.textContainerOrigin
        let text = storage.string as NSString
        var seen: [[Int]] = []
        var runs: [(text: String, fill: Int)] = []
        for ci in r.location..<NSMaxRange(r) where storage.attribute(.marginHidden, at: ci, effectiveRange: nil) == nil
        {
            let g = lm.glyphIndexForCharacter(at: ci)
            var line = NSRange()
            let frag = lm.lineFragmentRect(forGlyphAt: g, effectiveRange: &line)
            // From glyph locations, which TextKit gets right where its
            // bounding rectangles aren't (after a reflowed line break).
            let x0 = lm.location(forGlyphAt: g).x
            let x1 =
                g + 1 < NSMaxRange(line)
                ? lm.location(forGlyphAt: g + 1).x : lm.lineFragmentUsedRect(forGlyphAt: g, effectiveRange: nil).maxX
            let glyph = NSRect(x: frag.minX + x0, y: frag.minY, width: max(0, x1 - x0), height: frag.height)
            let font =
                storage.attribute(.font, at: ci, effectiveRange: nil) as? NSFont ?? Theme.font(size: Theme.bodySize)
            let baseline = frag.minY + lm.location(forGlyphAt: g).y
            let p = NSPoint(x: origin.x + glyph.midX, y: origin.y + baseline - font.xHeight - 2)
            guard
                let c = rep.colorAt(x: Int((p.x - b.minX) * scale), y: Int((p.y - b.minY) * scale))?
                    .usingColorSpace(.sRGB)
            else { continue }
            let rgb = [c.redComponent, c.greenComponent, c.blueComponent].map { Int(($0 * 255).rounded()) }
            // Within a step or two is the same fill.
            let fill =
                seen.firstIndex { zip($0, rgb).allSatisfy { abs($0 - $1) <= 2 } }
                ?? {
                    seen.append(rgb)
                    return seen.count - 1
                }()
            let ch = text.substring(with: NSRange(location: ci, length: 1))
            if let last = runs.last, last.fill == fill {
                runs[runs.count - 1].text += ch
            } else {
                runs.append((ch, fill))
            }
        }
        return runs.map { "\($0.text.debugDescription):\(Character(UnicodeScalar(UInt8(97 + $0.fill))))" }
            .joined(separator: " ")
    }

    /// Whether each list item's marker sits on its own text, on the
    /// baseline of the item's last character: `"Margin: About.": on its text`.
    var markers: [String] {
        let lm = view.layoutManager!
        lm.ensureLayout(for: view.textContainer!)
        return view.items.map { it in
            let line = view.lines[Int(it.line)]
            let last = max(Int(line.contentStart), Int(line.end) - 1)
            let text = view.fragmentRect(at: last).minY + lm.location(forGlyphAt: lm.glyphIndexForCharacter(at: last)).y
            let off = view.markerPosition(it).baseline - text
            let body = view.visibleText(
                NSRange(location: Int(line.contentStart), length: Int(line.end - line.contentStart)))
            return "\(body.debugDescription): \(abs(off) < 0.5 ? "on its text" : "off by \(off)")"
        }
    }
}
