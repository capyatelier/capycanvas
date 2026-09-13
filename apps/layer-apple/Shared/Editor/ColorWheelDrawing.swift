import SwiftUI

enum ColorWheelShape: UInt32, CaseIterable {
    case circle, square, triangle
    init(_ name: String) { self = name == "square" ? .square : name == "triangle" ? .triangle : .circle }
}

/// Rust owns geometry, hue mapping and pixel conversion. Retain static resources
/// separately from the changing field and markers, including during wheel drags.
struct ColorWheelDrawing: View {
    let model: JSON
    // Use panel coordinates to control the backing surface's pixel alignment.
    let bounds: JSON
    @Environment(\.displayScale) private var displayScale
    @StateObject private var field = ColorFieldImageCache()
    @StateObject private var guide = ColorFieldImageCache()
    var body: some View {
        Canvas { graphics, _ in
            var graphics = graphics
            let side = bounds[2].number
            // Match Web Canvas's rounded destination edges. Rasterize at that
            // destination's physical size to avoid a second image resampling.
            let x = bounds[0].number.rounded(), y = bounds[1].number.rounded()
            let width = (bounds[0].number + side).rounded() - x
            graphics.translateBy(x: x, y: y)
            graphics.scaleBy(x: width / side,
                y: ((bounds[1].number + side).rounded() - y) / side)
            let pixels = ceil(width * displayScale)
            let geometry = model["geometry"], shape = ColorWheelShape(model["shape"].string)
            func point(_ value: JSON) -> CGPoint { CGPoint(x: value[0].number * side, y: value[1].number * side) }
            let center = point(geometry["center"])
            if let image = field.image(side: pixels,
                hue: Float(model["wheel_components"][0].number), shape: shape) {
                var clipped = graphics
                if shape == .circle {
                    let radius = geometry["disc_radius"].number * side
                    clipped.clip(to: Path(ellipseIn: CGRect(x: center.x - radius, y: center.y - radius,
                        width: radius * 2, height: radius * 2)))
                } else if shape == .square {
                    let square = geometry["square"]
                    let rect = CGRect(x: square[0].number * side, y: square[1].number * side,
                        width: square[2].number * side, height: square[2].number * side)
                    clipped.clip(to: Path(roundedRect: rect, cornerRadius: min(6, side * 0.02)))
                }
                clipped.draw(Image(decorative: image, scale: 1).interpolation(.low),
                    in: CGRect(x: 0, y: 0, width: side, height: side))
            }
            let radius = (geometry["outer"].number + geometry["inner"].number) * side / 2
            let ring = Path(ellipseIn: CGRect(x: center.x - radius, y: center.y - radius,
                width: radius * 2, height: radius * 2))
            if let image = guide.image(side: pixels, hue: 0, shape: shape, guide: true) {
                var clipped = graphics
                clipped.clip(to: ring.strokedPath(StrokeStyle(lineWidth: (geometry["outer"].number - geometry["inner"].number) * side)))
                clipped.draw(Image(decorative: image, scale: 1).interpolation(.low),
                    in: CGRect(x: 0, y: 0, width: side, height: side))
            }
            let markerRadius = min(10, max(6, side * 0.04))
            for (key, fill) in [("wheel_hue_marker", "wheel_hue_color"), ("wheel_marker", "marker_color")] {
                let center = point(model[key])
                let marker = Path(ellipseIn: CGRect(x: center.x - markerRadius, y: center.y - markerRadius,
                    width: markerRadius * 2, height: markerRadius * 2))
                graphics.fill(marker, with: .color(model[fill].paintColor))
                graphics.stroke(marker, with: .color(.black.opacity(0.5)), lineWidth: 4)
                graphics.stroke(marker, with: .color(.white), lineWidth: 2)
            }
        }
    }
}

final class ColorWheelResources: ObservableObject {
    private var key: Key?
    private var cached = JSON()
    private struct Key: Equatable { let side: Float; let shape: ColorWheelShape }
    func layout(side: CGFloat, shape: ColorWheelShape) -> JSON {
        let next = Key(side: Float(side), shape: shape)
        if key == next { return cached }
        guard let pointer = capy_apple_color_resources(next.side, shape.rawValue) else { return JSON() }
        defer { capy_apple_string_free(pointer) }
        guard let result = try? JSON.decode(String(cString: pointer)) else { return JSON() }
        key = next; cached = result["layout"]
        return cached
    }
}

/// One retained image for each field/guide. Marker/readout changes reuse it; shape,
/// hue and physical raster size invalidate it. Native clips and markers remain
/// antialiased; the color samples come directly from shared Rust conversion.
final class ColorFieldImageCache: ObservableObject {
    private var key: Key?
    private var cached: CGImage?
    private struct Key: Equatable { let side: UInt32; let hue: Float; let shape: ColorWheelShape; let guide: Bool }
    func image(side: CGFloat, hue: Float, shape: ColorWheelShape, guide: Bool = false) -> CGImage? {
        guard side.isFinite, side >= 1, side <= 2048, hue.isFinite else { return nil }
        let next = Key(side: UInt32(side.rounded()), hue: guide ? 0 : hue, shape: shape, guide: guide)
        if key == next { return cached }
        var bytes = Data(count: Int(next.side) * Int(next.side) * 4)
        guard bytes.withUnsafeMutableBytes({ buffer in
            capy_apple_color_field(next.side, next.hue, shape.rawValue, guide, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count)
        }) == 1, let provider = CGDataProvider(data: bytes as CFData),
            let space = CGColorSpace(name: CGColorSpace.sRGB),
            let image = CGImage(width: Int(next.side), height: Int(next.side),
                bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(next.side) * 4, space: space,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                provider: provider, decode: nil, shouldInterpolate: shape == .circle, intent: .relativeColorimetric)
        else { return nil }
        key = next; cached = image
        return image
    }
}
