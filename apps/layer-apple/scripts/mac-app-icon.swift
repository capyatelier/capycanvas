import CoreGraphics
import Foundation
import ImageIO

let arguments = Array(CommandLine.arguments.dropFirst())
guard arguments.count >= 3,
      arguments.count % 2 == 1,
      let input = CGImageSourceCreateWithURL(URL(fileURLWithPath: arguments[0]) as CFURL, nil),
      let source = CGImageSourceCreateImageAtIndex(input, 0, nil),
      source.width == 1024, source.height == 1024 else {
    fatalError("Expected a 1024-pixel square source and destination/size pairs")
}

func macIcon(size: Int) -> CGImage {
    let scale = CGFloat(size) / 1024
    let body = CGRect(x: 100 * scale, y: 100 * scale, width: 824 * scale, height: 824 * scale)
    let radius = 185 * scale
    let context = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8,
                            bytesPerRow: size * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.clear(CGRect(x: 0, y: 0, width: size, height: size))
    context.addPath(CGPath(roundedRect: body, cornerWidth: radius, cornerHeight: radius, transform: nil))
    context.clip()
    context.interpolationQuality = .high
    context.draw(source, in: body)
    return context.makeImage()!
}

for index in stride(from: 1, to: arguments.count, by: 2) {
    let url = URL(fileURLWithPath: arguments[index])
    guard let size = Int(arguments[index + 1]), size > 0,
          let destination = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil) else {
        fatalError("Invalid icon output")
    }
    CGImageDestinationAddImage(destination, macIcon(size: size), nil)
    guard CGImageDestinationFinalize(destination) else {
        fatalError("Could not write \(url.lastPathComponent)")
    }
}
