// Usage: swift glass_range.swift <image> <x> <y> <w> <h> [block]
// Splits a text-free region of a capture into blocks and prints the
// luminance range of their medians: how light and how dark the glass gets
// over that backdrop. Then the WCAG contrast of each theme's text over the
// worst block.
import CoreGraphics
import Foundation
import ImageIO

let args = CommandLine.arguments
guard args.count >= 6,
    let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: args[1]) as CFURL, nil),
    let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
else {
    print("usage: glass_range <image> <x> <y> <w> <h> [block]")
    exit(1)
}
let (x, y, w, h) = (Int(args[2])!, Int(args[3])!, Int(args[4])!, Int(args[5])!)
let block = args.count > 6 ? Int(args[6])! : 24
func linear(_ v: Double) -> Double { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
func lum(_ r: Double, _ g: Double, _ b: Double) -> Double {
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}
func hex(_ s: String) -> Double {
    let v = Int(s.dropFirst(), radix: 16)!
    return lum(Double((v >> 16) & 255) / 255, Double((v >> 8) & 255) / 255, Double(v & 255) / 255)
}
guard let crop = image.cropping(to: CGRect(x: x, y: y, width: w, height: h)) else { exit(1) }
var pixels = [UInt8](repeating: 0, count: w * h * 4)
let context = CGContext(
    data: &pixels, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
    space: CGColorSpace(name: CGColorSpace.sRGB)!,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
context.draw(crop, in: CGRect(x: 0, y: 0, width: w, height: h))
var medians: [Double] = []
for by in stride(from: 0, to: h - block + 1, by: block) {
    for bx in stride(from: 0, to: w - block + 1, by: block) {
        var lums: [Double] = []
        for py in by..<(by + block) {
            for px in bx..<(bx + block) {
                let i = (py * w + px) * 4
                lums.append(lum(Double(pixels[i]) / 255, Double(pixels[i + 1]) / 255, Double(pixels[i + 2]) / 255))
            }
        }
        lums.sort()
        medians.append(lums[lums.count / 2])
    }
}
medians.sort()
let (low, high) = (medians.first!, medians.last!)
func ratio(_ a: Double, _ b: Double) -> Double { (max(a, b) + 0.05) / (min(a, b) + 0.05) }
print(String(format: "glass luminance %.3f to %.3f over %d blocks", low, high, medians.count))
// Asciinema (dark) and GitHub Sharp (light), from themes/*.json.
let dark = [("foreground", "#cccccc"), ("muted", "#6d6d6d")]
let light = [("foreground", "#24292e"), ("muted", "#586069")]
print("  dark text over the lightest block: " + dark.map { String(format: "%@ %.2f", $0.0, ratio(hex($0.1), high)) }.joined(separator: ", "))
print("  light text over the darkest block: " + light.map { String(format: "%@ %.2f", $0.0, ratio(hex($0.1), low)) }.joined(separator: ", "))
