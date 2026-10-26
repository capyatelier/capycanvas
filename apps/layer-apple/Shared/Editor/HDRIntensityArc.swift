import SwiftUI

struct HDRIntensityArc: View {
    @Environment(\.capyNativeCopy) private var nativeCopy
    let model: JSON
    let viewing: JSON
    let identity: String
    let language: String
    let geometry: JSON
    let caption: JSON
    let size: CGFloat
    var contact: ((String) -> Void)? = nil
    var touches: ColorTouchSurface? = nil
    let set: (Double) -> Void
    @State private var original: Double?
    @State private var range: [Double]?
    @StateObject private var resources = ColorUIResourceCache()
    var body: some View {
        let arc = resources.resolve(["type": "intensity_arc", "size": size, "stops": model["intensity"].number,
            "base": model["base"].raw, "document_space": viewing["document_space"].raw,
            "recipe": viewing["recipe"].raw, "headroom": viewing["headroom"].number, "depth": model["document_depth"].raw], language: language)
        ZStack(alignment: .topLeading) {
            HDRIntensityTrack(points: geometry["points"].array.map(point), colors: arc["colors"].array.map(HDRIntensityTrack.linearColor),
                center: point(geometry["center"]), marker: point(arc["marker"]), width: geometry["width"].number, radius: geometry["marker_radius"].number,
                zero: point(arc["zero"]))
            ParameterInput(region: .parameter(hdr: true), identity: identity, nudge: { phase, _, delta in
                if phase == "down" { original = model["intensity"].number; contact?("down") }
                if phase == "cancel" { if let original { set(original) }; original = nil; contact?("cancel"); return }
                if phase == "up" { original = nil; contact?("up"); return }
                set(model["intensity"].number + (delta[0] + delta[1]) * 0.1)
            }, touches: touches) { phase, _, p, side in
                if phase == "reset" { set(0); return }
                if phase == "cancel" { if let original { set(original) }; original = nil; range = nil; contact?("cancel"); return }
                if phase == "down" { original = model["intensity"].number; range = [arc["minimum"].number, arc["maximum"].number]; contact?("down") }
                let range = range ?? [arc["minimum"].number, arc["maximum"].number]
                let value = ColorUI.resolve(["type": "intensity_point", "size": side, "point": [p.x, p.y], "minimum": range[0], "maximum": range[1]], language: language)
                if !value.isNull { set(value.number) }
                if phase == "up" { original = nil; self.range = nil; contact?("up") }
            }.accessibilityHidden(true)
            let font = caption[2].number, label = String(format: "%+.2f EV", model["intensity"].number)
            Text(label).font(.system(size: font)).monospacedDigit().fixedSize().allowsHitTesting(false)
                .offset(x: caption[0].number - EditorTextMetrics.width(label, size: font, weight: .regular, monospacedDigits: true) / 2,
                    y: caption[1].number - EditorTextMetrics.ascent(size: font))
                .accessibilityElement().accessibilityLabel(nativeCopy["color"]["intensity"].string).accessibilityValue(label)
                .accessibilityIdentifier("color-intensity")
                .accessibilityAdjustableAction { set(model["intensity"].number + ($0 == .increment ? 0.1 : -0.1)) }
        }
    }
    private func point(_ p: JSON) -> CGPoint { CGPoint(x: p[0].number, y: p[1].number) }
}

struct HDRIntensityTrack: View {
    let points: [CGPoint]
    let colors: [Color]
    let center: CGPoint
    let marker: CGPoint
    let width: CGFloat
    let radius: CGFloat
    var zero: CGPoint?
    var markerFill: Color?
    var rimOpacity = 0.65
    var zeroTick: [CGPoint] = []
    var ink: Color = .white
    static func linearColor(_ value: JSON) -> Color {
        Color(.sRGBLinear, red: value[0].number, green: value[1].number, blue: value[2].number)
    }
    var body: some View {
        Canvas(colorMode: .extendedLinear) { graphics, _ in
            guard let first = points.first, let last = points.last, !colors.isEmpty else { return }
            let start = atan2(first.y - center.y, first.x - center.x)
            let sweep = atan2(last.y - center.y, last.x - center.x) - start
            let stops = colors.enumerated().map { index, color in
                Gradient.Stop(color: color, location: (start + sweep * CGFloat(index) / CGFloat(max(1, colors.count - 1))) / (2 * .pi))
            }.sorted { $0.location < $1.location }
            var path = Path(); path.addLines(points)
            graphics.stroke(path, with: .conicGradient(Gradient(stops: stops), center: center),
                style: StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round))
            if let zero {
                graphics.fill(Path(ellipseIn: CGRect(x: zero.x - 1.5, y: zero.y - 1.5, width: 3, height: 3)), with: .color(.white.opacity(0.8)))
            }
            let circle = Path(ellipseIn: CGRect(x: marker.x - radius, y: marker.y - radius, width: radius * 2, height: radius * 2))
            if let markerFill { graphics.fill(circle, with: .color(markerFill)) }
            graphics.stroke(circle, with: .color(.black.opacity(rimOpacity)), lineWidth: 4)
            graphics.stroke(circle, with: .color(.white), lineWidth: 2)
            if !zeroTick.isEmpty {
                var tick = Path(); tick.addLines(zeroTick)
                graphics.stroke(tick, with: .color(ink.opacity(0.9)), lineWidth: 1)
            }
        }.allowedDynamicRange(.high).allowsHitTesting(false)
    }
}
