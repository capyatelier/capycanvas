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
    var viewing = JSON()
    @Environment(\.displayScale) private var displayScale
    @StateObject private var field = ColorFieldImageCache()
    @StateObject private var guide = ColorFieldImageCache()
    @StateObject private var hdrField = HDRColorFieldImageCache()
    var body: some View {
        let side = bounds[2].number
        let pixels = ceil(((bounds[0].number + side).rounded() - bounds[0].number.rounded()) * displayScale)
        let hdrRequest = viewing.isNull ? JSON() : viewing.replacing("stops", with: model["intensity"])
        let request = viewing.isNull ? JSON() : JSON(["side": pixels, "hue": model["wheel_components"][0].number,
            "shape": model["shape"].string, "space": model["rgb_space"].string, "viewing": hdrRequest.raw])
        let hdrImage = hdrField.image
        Canvas(colorMode: .extendedLinear) { graphics, _ in
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
            let image = viewing.isNull ? field.image(side: pixels,
                hue: Float(model["wheel_components"][0].number), shape: shape, rgbSpace: model["rgb_space"].string) : hdrImage
            if let image {
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
                clipped.draw(Image(decorative: image, scale: 1).allowedDynamicRange(.high).interpolation(.low),
                    in: CGRect(x: 0, y: 0, width: side, height: side))
            }
            let radius = (geometry["outer"].number + geometry["inner"].number) * side / 2
            let ring = Path(ellipseIn: CGRect(x: center.x - radius, y: center.y - radius,
                width: radius * 2, height: radius * 2))
            if let image = guide.image(side: pixels, hue: 0, shape: shape, rgbSpace: model["rgb_space"].string, guide: true) {
                var clipped = graphics
                clipped.clip(to: ring.strokedPath(StrokeStyle(lineWidth: (geometry["outer"].number - geometry["inner"].number) * side)))
                clipped.draw(Image(decorative: image, scale: 1).allowedDynamicRange(.high).interpolation(.low),
                    in: CGRect(x: 0, y: 0, width: side, height: side))
            }
            let markerRadius = min(10, max(6, side * 0.04))
            for (key, fill) in [("wheel_hue_marker", "wheel_hue_color"), ("wheel_marker", "marker_color")] {
                let center = point(model[key])
                let marker = Path(ellipseIn: CGRect(x: center.x - markerRadius, y: center.y - markerRadius,
                    width: markerRadius * 2, height: markerRadius * 2))
                if key == "wheel_marker" && !viewing.isNull {
                    let p = ColorUI.resolve(["type": "hdr_preview", "color": model["definition"].raw,
                        "document_space": viewing["document_space"].raw, "recipe": viewing["recipe"].raw, "headroom": viewing["headroom"].number])["linear"]
                    graphics.fill(marker, with: .color(Color(.sRGBLinear, red: p[0].number, green: p[1].number, blue: p[2].number)))
                } else { graphics.fill(marker, with: .color(model[fill].paintColor)) }
                graphics.stroke(marker, with: .color(.black.opacity(0.5)), lineWidth: 4)
                graphics.stroke(marker, with: .color(.white), lineWidth: 2)
            }
        }.onChange(of: request.stableKey, initial: true) { _, _ in hdrField.request(request) }
            .onDisappear { hdrField.cancel() }
    }
}

/// UI bitmap work has one running job and one replaceable pending request.
/// Shared Rust still owns every color sample. Native drawing never waits for it.
@MainActor final class HDRColorFieldImageCache: ObservableObject {
    @Published private(set) var image: CGImage?
    private let worker = HDRFieldWorker()
    private var pending: JSON?
    private var running = false
    private var generation: UInt64 = 0
    private var requested = ""
    func request(_ value: JSON) {
        guard !value["viewing"].isNull else { cancel(); return }
        let key = value.stableKey
        guard key != requested else { return }
        requested = key; pending = value; start()
    }
    func cancel() {
        generation &+= 1; pending = nil; requested = ""
        if image != nil { image = nil }
        worker.clear()
    }
    private func start() {
        guard !running, let value = pending else { return }
        running = true; pending = nil
        let token = generation
        worker.render(value) { [weak self] image in
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                // Completed jobs are ordered. Show the latest completed bitmap
                // while coalescing subsequent pointer updates into one job.
                if generation == token { self.image = image }
                running = false; start()
            }
        }
    }
}

/// The mutable raster cache is private and touched only on this serial queue.
/// Immutable CGImages are the only results crossing back to the main actor.
private final class HDRFieldWorker: @unchecked Sendable {
    private let queue = DispatchQueue(label: "art.capycanvas.hdr-picker", qos: .userInitiated)
    private let cache = ColorFieldImageCache()
    func clear() { queue.async { self.cache.clear() } }
    func render(_ value: JSON, completion: @escaping @Sendable (CGImage?) -> Void) {
        queue.async {
            completion(self.cache.image(side: value["side"].number, hue: Float(value["hue"].number),
                shape: ColorWheelShape(value["shape"].string), rgbSpace: value["space"].string, hdr: value["viewing"]))
        }
    }
}

final class ColorPanelLayoutCache: ObservableObject {
    private var side: Float?
    private var hdr = false
    private var cached = JSON()
    func layout(side: CGFloat, hdr: Bool = false) -> JSON {
        let next = Float(side)
        if self.side == next && self.hdr == hdr { return cached }
        self.side = next; self.hdr = hdr
        cached = ColorUI.resolve(["type": "picker_layout", "size": side, "hdr": hdr])
        return cached
    }
}

/// One retained image for each field/guide. Marker/readout changes reuse it; shape,
/// hue and physical raster size invalidate it. Native clips and markers remain
/// antialiased; the color samples come directly from shared Rust conversion.
final class ColorFieldImageCache: ObservableObject {
    private var key: Key?
    private var cached: CGImage?
    private var baseKey: Key?
    private var base = [Float]()
    private struct Key: Equatable { let side: UInt32; let hue: Float; let shape: ColorWheelShape; let space: String; let guide: Bool; let hdr: String }
    func clear() { key = nil; cached = nil; baseKey = nil; base = [] }
    func image(side: CGFloat, hue: Float, shape: ColorWheelShape, rgbSpace: String, guide: Bool = false, hdr: JSON = JSON()) -> CGImage? {
        guard side.isFinite, side >= 1, side <= 2048, hue.isFinite else { return nil }
        let next = Key(side: UInt32(side.rounded()), hue: guide ? 0 : hue, shape: shape, space: rgbSpace, guide: guide, hdr: hdr.stableKey)
        if key == next { return cached }
        if !hdr.isNull {
            let request = JSON(["space": rgbSpace, "hue": hue, "shape": String(describing: shape), "stops": hdr["stops"].number,
                "recipe": hdr["recipe"].raw, "headroom": hdr["headroom"].number])
            guard let text = try? request.encoded() else { return nil }
            let sourceKey = Key(side: next.side, hue: hue, shape: shape, space: rgbSpace, guide: false, hdr: "")
            if baseKey != sourceKey {
                var pixels = [Float](repeating: 0, count: Int(next.side * next.side * 4))
                guard text.withCString({ pointer in pixels.withUnsafeMutableBufferPointer { capy_apple_hdr_field(next.side, pointer, $0.baseAddress, $0.count, 1) } }) else { return nil }
                baseKey = sourceKey; base = pixels
            }
            var pixels = base
            guard text.withCString({ pointer in pixels.withUnsafeMutableBufferPointer { capy_apple_hdr_field(next.side, pointer, $0.baseAddress, $0.count, 2) } }) else { return nil }
            let bytes = pixels.withUnsafeBytes { Data($0) }
            guard let provider = CGDataProvider(data: bytes as CFData), let space = CGColorSpace(name: CGColorSpace.extendedLinearSRGB),
                let image = CGImage(width: Int(next.side), height: Int(next.side), bitsPerComponent: 32, bitsPerPixel: 128,
                    bytesPerRow: Int(next.side) * 16, space: space, bitmapInfo: [.floatComponents, .byteOrder32Little, CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)],
                    provider: provider, decode: nil, shouldInterpolate: true, intent: .relativeColorimetric) else { return nil }
            key = next; cached = image; return image
        }
        baseKey = nil; base = []
        var bytes = Data(count: Int(next.side) * Int(next.side) * 4)
        guard bytes.withUnsafeMutableBytes({ buffer in
            rgbSpace.withCString {
                capy_apple_color_field(next.side, next.hue, shape.rawValue, $0, guide, buffer.bindMemory(to: UInt8.self).baseAddress, buffer.count)
            }
        }) == 1, let provider = CGDataProvider(data: bytes as CFData),
            let space = CGColorSpace(name: CGColorSpace.displayP3),
            let image = CGImage(width: Int(next.side), height: Int(next.side),
                bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(next.side) * 4, space: space,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                provider: provider, decode: nil, shouldInterpolate: shape == .circle, intent: .relativeColorimetric)
        else { return nil }
        key = next; cached = image
        return image
    }
}
