import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

@main struct ImageImportChecks {
    static func write(_ url: URL, width: Int, height: Int, pixels: [UInt8], orientation: Int = 1) {
        let provider = CGDataProvider(data: Data(pixels) as CFData)!
        let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), provider: provider,
            decode: nil, shouldInterpolate: false, intent: .relativeColorimetric)!
        let output = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(output, image, [kCGImagePropertyOrientation: orientation] as CFDictionary)
        assert(CGImageDestinationFinalize(output))
    }
    static func main() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let colors: [[UInt8]] = [[255,0,0,255], [0,255,0,255], [0,0,255,128], [0,0,0,0], [255,255,0,255], [0,255,255,255]]
        let plain = directory.appendingPathComponent("plain.png")
        write(plain, width: 2, height: 3, pixels: colors.flatMap { $0 })
        let decoded = try LayerImagePixels.decode(plain)
        assert(decoded.width == 2 && decoded.height == 3)
        assert(decoded.rgba == Data(colors.flatMap { $0 }), "Preserve top-to-bottom rows, straight alpha and sRGB channels")
        let rotated = directory.appendingPathComponent("rotated.png")
        write(rotated, width: 2, height: 3, pixels: colors.flatMap { $0 }, orientation: 6)
        let turned = try LayerImagePixels.decode(rotated)
        assert(turned.width == 3 && turned.height == 2)
        assert(turned.rgba == Data([4,2,0,5,3,1].flatMap { colors[$0] }), "Apply EXIF rotation without resizing")
        let oversized = directory.appendingPathComponent("oversized.png")
        write(oversized, width: 8193, height: 1, pixels: Array(repeating: 255, count: 8193 * 4))
        do { _ = try LayerImagePixels.decode(oversized); fatalError("Oversized import must fail before decode") }
        catch is HostFailure {}
        print("Image import checks passed: orientation, sRGB, alpha, row order and size limit")
    }
}
