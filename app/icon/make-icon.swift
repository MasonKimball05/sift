// Draws sift's app icon: files dropping into a funnel, on the same blue as
// Daybook. Run `make icon`; Tauri's `cargo tauri icon` then makes every size.
import AppKit

let size: CGFloat = 1024
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size), bitsPerSample: 8,
                           samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
let ctx = NSGraphicsContext.current!.cgContext
ctx.translateBy(x: 0, y: size)
ctx.scaleBy(x: 1, y: -1) // work top-down

let light = NSColor(srgbRed: 0x6D / 255.0, green: 0x95 / 255.0, blue: 0xF6 / 255.0, alpha: 1)
let blue = NSColor(srgbRed: 0x2A / 255.0, green: 0x5B / 255.0, blue: 0xD7 / 255.0, alpha: 1)

// The Mac tile: a rounded square inside the standard margin, with a soft shadow.
let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
let tilePath = CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: 10), blur: 24, color: NSColor.black.withAlphaComponent(0.3).cgColor)
ctx.addPath(tilePath); ctx.setFillColor(blue.cgColor); ctx.fillPath()
ctx.restoreGState()
ctx.saveGState()
ctx.addPath(tilePath); ctx.clip()
let gradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [light.cgColor, blue.cgColor] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: tile.minY), end: CGPoint(x: 0, y: tile.maxY), options: [])
ctx.restoreGState()

// Three files falling in, slightly tilted, fading toward the top.
func file(_ center: CGPoint, angle: CGFloat, alpha: CGFloat) {
    ctx.saveGState()
    ctx.translateBy(x: center.x, y: center.y)
    ctx.rotate(by: angle)
    let page = CGRect(x: -52, y: -66, width: 104, height: 132)
    ctx.addPath(CGPath(roundedRect: page, cornerWidth: 16, cornerHeight: 16, transform: nil))
    ctx.setFillColor(NSColor.white.withAlphaComponent(alpha).cgColor)
    ctx.fillPath()
    // Text lines on the page.
    ctx.setFillColor(blue.withAlphaComponent(0.55 * alpha).cgColor)
    for (i, width) in [62.0, 62, 40].enumerated() {
        ctx.fill(CGRect(x: -31, y: -30 + Double(i) * 24, width: width, height: 10))
    }
    ctx.restoreGState()
}
file(CGPoint(x: 390, y: 285), angle: -0.28, alpha: 0.7)
file(CGPoint(x: 640, y: 255), angle: 0.22, alpha: 0.85)
file(CGPoint(x: 515, y: 395), angle: -0.05, alpha: 1)

// The funnel: a wide rim narrowing to a spout.
let funnel = CGMutablePath()
funnel.move(to: CGPoint(x: 262, y: 500))
funnel.addLine(to: CGPoint(x: 762, y: 500))
funnel.addLine(to: CGPoint(x: 570, y: 680))
funnel.addLine(to: CGPoint(x: 570, y: 800))
funnel.addQuadCurve(to: CGPoint(x: 454, y: 800), control: CGPoint(x: 512, y: 830))
funnel.addLine(to: CGPoint(x: 454, y: 680))
funnel.closeSubpath()
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: 12), blur: 26, color: NSColor(srgbRed: 0.08, green: 0.16, blue: 0.45, alpha: 0.4).cgColor)
ctx.addPath(funnel); ctx.setFillColor(NSColor.white.cgColor); ctx.fillPath()
ctx.restoreGState()
// The rim's lip.
ctx.addPath(CGPath(roundedRect: CGRect(x: 240, y: 478, width: 544, height: 50), cornerWidth: 25, cornerHeight: 25, transform: nil))
ctx.setFillColor(NSColor(srgbRed: 0.90, green: 0.93, blue: 1.0, alpha: 1).cgColor)
ctx.fillPath()

NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
