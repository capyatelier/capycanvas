import SwiftUI

struct HDRIntensityArc: View {
    @ObservedObject var store: EditorStore
    let geometry: JSON
    let caption: JSON
    let size: CGFloat
    @State private var original: Double?
    @State private var range: [Double]?
    @State private var editing = false
    @State private var input = "0"
    private var model: JSON { store.colorPanel }
    var body: some View {
        let viewing = store.colorViewing
        let arc = ColorUI.resolve(["type": "intensity_arc", "size": size, "stops": model["intensity"].number,
            "base": model["base"].raw, "document_space": model["rgb_space"].raw,
            "recipe": viewing["recipe"].raw, "headroom": viewing["headroom"].number, "depth": model["document_depth"].raw])
        ZStack(alignment: .topLeading) {
            Canvas(colorMode: .extendedLinear) { graphics, _ in
                let points = geometry["points"].array, colors = arc["colors"].array
                for i in 0..<max(0, points.count - 1) where colors.indices.contains(i + 1) {
                    let a = point(points[i]), b = point(points[i + 1])
                    var path = Path(); path.move(to: a); path.addLine(to: b)
                    graphics.stroke(path, with: .linearGradient(Gradient(colors: [linear(colors[i]), linear(colors[i + 1])]), startPoint: a, endPoint: b),
                        style: StrokeStyle(lineWidth: geometry["width"].number, lineCap: .round))
                }
                let zero = point(arc["zero"])
                graphics.fill(Path(ellipseIn: CGRect(x: zero.x - 1.5, y: zero.y - 1.5, width: 3, height: 3)), with: .color(.white.opacity(0.8)))
                let p = point(arc["marker"]), r = geometry["marker_radius"].number
                let marker = Path(ellipseIn: CGRect(x: p.x - r, y: p.y - r, width: r * 2, height: r * 2))
                graphics.stroke(marker, with: .color(.black.opacity(0.65)), lineWidth: 4)
                graphics.stroke(marker, with: .color(.white), lineWidth: 2)
            }.allowedDynamicRange(.high).allowsHitTesting(false)
            ParameterInput(hdr: true, identity: "\(store.state["document_file"]["epoch"].uint):\(store.displayColors["paint_slot"].string)", nudge: { phase, _, delta in
                if phase == "down" { original = model["intensity"].number }
                if phase == "cancel" { if let original { set(original) }; original = nil; return }
                if phase == "up" { original = nil; return }
                set(model["intensity"].number + (delta[0] + delta[1]) * 0.1)
            }) { phase, _, p, side in
                if phase == "reset" { set(0); return }
                if phase == "cancel" { if let original { set(original) }; original = nil; range = nil; return }
                if phase == "down" { original = model["intensity"].number; range = [arc["minimum"].number, arc["maximum"].number] }
                let range = range ?? [arc["minimum"].number, arc["maximum"].number]
                let value = ColorUI.resolve(["type": "intensity_point", "size": side, "point": [p.x, p.y], "minimum": range[0], "maximum": range[1]])
                if !value.isNull { set(value.number) }
                if phase == "up" { original = nil; self.range = nil }
            }.accessibilityHidden(true)
            let font = caption[2].number, label = String(format: "%+.2f EV", model["intensity"].number)
            Button { input = String(format: "%.2f", model["intensity"].number); editing = true } label: {
                Text(label).font(.system(size: font)).monospacedDigit().fixedSize()
            }.buttonStyle(.plain)
                .offset(x: caption[0].number - EditorTextMetrics.width(label, size: font, weight: .regular, monospacedDigits: true) / 2,
                    y: caption[1].number - EditorTextMetrics.ascent(size: font))
                .accessibilityLabel("HDR intensity").accessibilityIdentifier("color-intensity")
                .accessibilityAdjustableAction { set(model["intensity"].number + ($0 == .increment ? 0.1 : -0.1)) }
                .popover(isPresented: $editing) {
                    let draft = ColorUI.resolve(["type": "form", "request": ["color": model["definition"].raw,
                        "document_space": model["rgb_space"].raw, "intensity": model["intensity"].number, "document_depth": model["document_depth"].raw, "change_intensity_text": input]])
                    VStack(spacing: 12) {
                        Text("Intensity (EV)").font(.headline)
                        TextField("Intensity (EV)", text: $input).textFieldStyle(.roundedBorder)
                        if !draft["error"].isNull { Text(draft["error"].string).font(.caption).foregroundStyle(.red) }
                        HStack {
                            Button("Cancel") { editing = false }.keyboardShortcut(.cancelAction)
                            Button("Apply") { set(draft["draft"]["intensity"].number); editing = false }
                                .disabled(!draft["error"].isNull || draft["value"].isNull).keyboardShortcut(.defaultAction)
                        }
                    }.padding(16).frame(width: 220)
                }
        }
    }
    private func set(_ value: Double) { store.dispatch(["type": "color", "action": ["op": "hdr_intensity", "stops": value]]) }
    private func point(_ p: JSON) -> CGPoint { CGPoint(x: p[0].number, y: p[1].number) }
    private func linear(_ p: JSON) -> Color { Color(.sRGBLinear, red: p[0].number, green: p[1].number, blue: p[2].number) }
}
