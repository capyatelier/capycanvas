import Foundation
import ImageIO
import CoreGraphics

struct LayerImagePixels: Sendable {
    let name: String
    let width: UInt32
    let height: UInt32
    let rgba: Data

    static func decode(_ url: URL) throws -> Self {
        let access = url.startAccessingSecurityScopedResource()
        defer { if access { url.stopAccessingSecurityScopedResource() } }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
            let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
            let width = properties[kCGImagePropertyPixelWidth] as? Int,
            let height = properties[kCGImagePropertyPixelHeight] as? Int,
            width > 0, height > 0, width <= 8192, height <= 8192 else {
            throw HostFailure(message: "Import an image up to 8192 × 8192 pixels")
        }
        // ImageIO applies EXIF orientation at the original resolution. The
        // maximum equals the source's long edge, so no pixels are downsampled.
        let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: max(width, height)]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary),
            let space = CGColorSpace(name: CGColorSpace.sRGB) else {
            throw HostFailure(message: "Could not decode this image")
        }
        let w = image.width, h = image.height
        var bytes = Data(count: w * h * 4)
        try bytes.withUnsafeMutableBytes { buffer in
            guard let context = CGContext(data: buffer.baseAddress, width: w, height: h,
                bitsPerComponent: 8, bytesPerRow: w * 4, space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue) else {
                throw HostFailure(message: "Could not allocate image pixels")
            }
            context.setBlendMode(.copy)
            context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
            // Shared imports use straight alpha, like web ImageData. Quartz
            // produces premultiplied pixels; undo that without dark fringes.
            let pixels = buffer.bindMemory(to: UInt8.self)
            for offset in stride(from: 0, to: pixels.count, by: 4) {
                let alpha = Int(pixels[offset + 3])
                if alpha > 0 && alpha < 255 {
                    for channel in 0..<3 {
                        pixels[offset + channel] = UInt8(min(255, (Int(pixels[offset + channel]) * 255 + alpha / 2) / alpha))
                    }
                }
            }
        }
        return Self(name: url.lastPathComponent, width: UInt32(w), height: UInt32(h), rgba: bytes)
    }
}
