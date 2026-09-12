import SwiftUI

/// Native presentation of Rust's normalized wheel geometry and display-encoded
/// color stops. Picking and color conversion remain in the shared model.
struct ColorWheelDrawing: View {
    let model: JSON
    @Environment(\.displayScale) private var displayScale
    @StateObject private var hlsField = HLSFieldImageCache()
    var body: some View {
        Canvas { graphics, allocation in
            let size = min(allocation.width, allocation.height)
            let geometry = model["geometry"], hue = model["hue_color"].paintColor
            func point(_ value: JSON) -> CGPoint {
                CGPoint(x: value[0].number * size, y: value[1].number * size)
            }
            if model["space"].string == "hsv" {
                let square = geometry["square"]
                let rect = CGRect(x: square[0].number * size, y: square[1].number * size,
                    width: square[2].number * size, height: square[2].number * size)
                graphics.fill(Path(rect), with: .linearGradient(Gradient(colors: [.white, hue]),
                    startPoint: rect.origin, endPoint: CGPoint(x: rect.maxX, y: rect.minY)))
                graphics.fill(Path(rect), with: .linearGradient(Gradient(colors: [.black.opacity(0), .black]),
                    startPoint: rect.origin, endPoint: CGPoint(x: rect.minX, y: rect.maxY)))
            } else if let image = hlsField.image(side: size * displayScale,
                hue: Float(model["components"][0]["value"].number)) {
                graphics.draw(Image(decorative: image, scale: displayScale).interpolation(.none),
                    in: CGRect(x: 0, y: 0, width: size, height: size))
            }
            let center = point(geometry["center"])
            let radius = (geometry["outer"].number + geometry["inner"].number) * size / 2
            let ring = Path(ellipseIn: CGRect(x: center.x - radius, y: center.y - radius,
                width: radius * 2, height: radius * 2))
            graphics.stroke(ring, with: .conicGradient(Gradient(colors: model["hue_stops"].array.map(\.paintColor)),
                center: center, angle: .degrees(model["hue_start_degrees"].number)),
                lineWidth: (geometry["outer"].number - geometry["inner"].number) * size)
            for key in ["hue_marker", "field_marker"] {
                let center = point(model[key])
                let marker = Path(ellipseIn: CGRect(x: center.x - 3.5, y: center.y - 3.5, width: 7, height: 7))
                graphics.stroke(marker, with: .color(.black), lineWidth: 3)
                graphics.stroke(marker, with: .color(.white), lineWidth: 1.5)
            }
        }
    }
}

/// One bitmap per visible wheel, retained only until hue or physical size changes.
/// Saturation/lightness, opacity and marker motion reuse it. Memoization does not
/// publish view state; the model/geometry already trigger drawing when necessary.
final class HLSFieldImageCache: ObservableObject {
    private var key: Key?
    private var cached: CGImage?
    private struct Key: Equatable { let side: UInt32; let hue: Float }

    func image(side: CGFloat, hue: Float) -> CGImage? {
        guard side.isFinite, side >= 1, side <= CGFloat(UInt32.max), hue.isFinite else { return nil }
        let next = Key(side: UInt32(side.rounded()), hue: hue)
        if key == next { return cached }
        let pixels = Int(next.side).multipliedReportingOverflow(by: Int(next.side))
        let length = pixels.partialValue.multipliedReportingOverflow(by: 4)
        guard !pixels.overflow, !length.overflow else { return nil }
        var bytes = Data(count: length.partialValue)
        guard bytes.withUnsafeMutableBytes({ buffer in
            capy_apple_hls_field(next.side, hue, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count)
        }) == 1, let provider = CGDataProvider(data: bytes as CFData),
            let space = CGColorSpace(name: CGColorSpace.sRGB),
            let image = CGImage(width: Int(next.side), height: Int(next.side),
                bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(next.side) * 4, space: space,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .relativeColorimetric)
        else { return nil }
        key = next; cached = image
        return image
    }
}
