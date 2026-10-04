import SwiftUI

struct ScopePlot: @unchecked Sendable {
    let bins: [(channel: Int, values: [CGFloat])]
    let image: CGImage?
}

struct ScopeUpdate: @unchecked Sendable {
    let plots: [String: ScopePlot]
    let colors: [Color]
    init(header: JSON, pixels: Data) {
        colors = header["colors"].array.map { Color(.sRGB, red: $0[0].number / 255, green: $0[1].number / 255, blue: $0[2].number / 255) }
        var plots: [String: ScopePlot] = [:]
        for (kind, view) in header["plots"].object {
            let view = JSON(view)
            let bins = view["plot"].array.map { (channel: Int($0[0].uint), values: $0[1].array.map { CGFloat($0.number) }) }
            var image: CGImage?
            let width = Int(view["extent"][0].uint), height = Int(view["extent"][1].uint)
            if width > 0, height > 0, pixels.count == width * height * 4, let provider = CGDataProvider(data: pixels as CFData),
               let space = CGColorSpace(name: CGColorSpace.sRGB) {
                image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4, space: space,
                    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue), provider: provider,
                    decode: nil, shouldInterpolate: false, intent: .defaultIntent)
            }
            plots[kind] = ScopePlot(bins: bins, image: image)
        }
        self.plots = plots
    }
}

@MainActor final class ScopePlots: ObservableObject {
    @Published private(set) var plots: [String: ScopePlot] = [:]
    @Published private(set) var colors: [Color] = []
    func receive(_ update: ScopeUpdate) {
        plots.merge(update.plots) { $1 }
        colors = update.colors
    }
}
