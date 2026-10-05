import AppKit
import XCTest

@testable import MarginKit

/// What a view drew, in sRGB, to read back.
struct Drawn {
    /// Its pixels: premultiplied RGBA, the top row first.
    let context: CGContext
    /// What of the view it is, in the view's coordinates.
    let rect: NSRect
    let flipped: Bool
    let scale: CGFloat

    /// The color drawn at `p`, in the view's coordinates, from the
    /// pixel's own bytes.
    func color(at p: NSPoint) -> NSColor? {
        let x = Int((p.x - rect.minX) * scale)
        let y = Int((flipped ? p.y - rect.minY : rect.maxY - p.y) * scale)
        guard x >= 0, y >= 0, x < context.width, y < context.height, let data = context.data else { return nil }
        let px = data.assumingMemoryBound(to: UInt8.self) + y * context.bytesPerRow + x * 4
        let a = CGFloat(px[3]) / 255
        let c = { (i: Int) in a > 0 ? CGFloat(px[i]) / 255 / a : 0 }
        return NSColor(srgbRed: c(0), green: c(1), blue: c(2), alpha: a)
    }

    var image: NSBitmapImageRep { NSBitmapImageRep(cgImage: context.makeImage()!) }
}

/// What the window draws, read back from its views.
extension Harness {
    /// What `v` (the text view by default) draws in `rect` (all of it by
    /// default), at twice its size, into a context in sRGB. What AppKit's
    /// bitmaps draw and read back is in the screen's colors or in Generic
    /// RGB, which would make what tests read depend on the screen. Images
    /// are decoded when first drawn, so it draws again once they are.
    func drawing(of v: NSView? = nil, in rect: NSRect? = nil) -> Drawn {
        let v = v ?? view
        let r = rect ?? v.bounds
        let scale: CGFloat = 2
        func draw() -> CGContext {
            let cg = CGContext(
                data: nil, width: Int((r.width * scale).rounded(.up)), height: Int((r.height * scale).rounded(.up)),
                bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
            cg.scaleBy(x: scale, y: scale)
            if v.isFlipped {
                cg.translateBy(x: 0, y: r.height)
                cg.scaleBy(x: 1, y: -1)
            }
            cg.translateBy(x: -r.minX, y: -r.minY)
            v.displayIgnoringOpacity(r, in: NSGraphicsContext(cgContext: cg, flipped: v.isFlipped))
            return cg
        }
        var cg = draw()
        if ImageLibrary.shared.isDecoding {
            wait(until: { !ImageLibrary.shared.isDecoding }, timeout: 5, "decoding")
            cg = draw()
        }
        return Drawn(context: cg, rect: r, flipped: v.isFlipped, scale: scale)
    }

    /// Draws the window's content (no title bar) as
    /// `macos/build/snapshots/NAME.png`, to look at while working.
    @discardableResult
    func snapshot(_ name: String) -> URL {
        layOut()
        let rep = drawing(of: win.contentView!).image
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
        let drawn = drawing(in: view.visibleRect)
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
            guard let c = drawn.color(at: pt) else { continue }
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
