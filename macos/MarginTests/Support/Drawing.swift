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
        guard r.location != NSNotFound else { return "\(s) not found" }
        let b = view.visibleRect
        guard let rep = view.bitmapImageRepForCachingDisplay(in: b) else { return "no bitmap" }
        view.cacheDisplay(in: b, to: rep)
        let scale = CGFloat(rep.pixelsWide) / b.width
        let text = view.string as NSString
        var seen: [[Int]] = []
        var runs: [(text: String, fill: Int)] = []
        for ci in r.location..<NSMaxRange(r) {
            // Only characters that show: where they are laid out.
            guard let at = view.laid(ci), let next = view.laid(ci + 1),
                next.offset > at.offset
                    || next.paragraph > at.paragraph
            else { continue }
            let p = view.laidOut[at.paragraph]
            let l = p.lines[at.line]
            let x0 = view.x(at.offset, on: l)
            let x1 = next.paragraph == at.paragraph && next.line == at.line ? view.x(next.offset, on: l) : x0 + 4
            let font =
                p.text.attribute(.font, at: min(at.offset, max(0, p.text.length - 1)), effectiveRange: nil)
                as? NSFont ?? Theme.font(size: Theme.bodySize)
            let pt = NSPoint(x: (x0 + x1) / 2, y: l.baseline - font.xHeight - 2)
            guard
                let c = rep.colorAt(x: Int((pt.x - b.minX) * scale), y: Int((pt.y - b.minY) * scale))?
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
        view.items.map { it in
            let line = view.lines[Int(it.line)]
            let last = max(Int(line.contentStart), Int(line.end) - 1)
            let text = view.laid(last).map { view.laidOut[$0.paragraph].lines[$0.line].baseline } ?? 0
            let off = view.markerPosition(it).baseline - text
            let body = view.visibleText(
                NSRange(location: Int(line.contentStart), length: Int(line.end - line.contentStart)))
            return "\(body.debugDescription): \(abs(off) < 0.5 ? "on its text" : "off by \(off)")"
        }
    }
}
