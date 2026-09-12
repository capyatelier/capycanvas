import SwiftUI

struct ColorPanel: View {
    @ObservedObject var store: EditorStore
    private var model: JSON { store.snapshot["color_panel"] }
    private var context: String { model["space"].string + store.state["colors"]["paint_slot"].string }
    private var textSize: CGFloat { max(1, store.catalog["text_size_pt"].number * 4 / 3) }
    private var buttonHeight: CGFloat { textSize * 1.66 + 8 }
    private var captureState: String? {
        #if DEBUG
        guard ProcessInfo.processInfo.environment["CAPY_COLOR_PROBE"] == "1" else { return nil }
        return try? JSON(["space": model["space"].raw, "hue": model["components"][0]["value"].raw,
            "rgba": store.state["brush"]["color"].raw]).encoded()
        #else
        return nil
        #endif
    }
    var body: some View {
        VStack(spacing: 0) {
            GeometryReader { allocation in
                let size = min(allocation.size.width, allocation.size.height)
                ZStack {
                    ColorWheelDrawing(model: model).allowsHitTesting(false)
                    ColorWheelInput(space: model["space"].string == "hls" ? 1 : 0, context: context) { part, point, size in
                        color(["op": "pick", "part": part == 1 ? "hue" : "field",
                            "point": [point.x, point.y], "size": size])
                    }
                }.frame(width: size, height: size)
            }.aspectRatio(1, contentMode: .fit).modifier(ColorPanelMeasurement(id: "wheel"))
            ColorSwatchesLayout(height: buttonHeight, swapWidth: HeaderTextMetrics.width("Swap", size: textSize, weight: .bold) + 24) {
                ForEach(model["swatches"].array.indices, id: \.self) { index in
                    let swatch = model["swatches"][index]
                    Button { color(["op": "select", "slot": swatch["slot"].raw]) } label: {
                        swatch["rgba"].paintColor.frame(height: 22).clipShape(RoundedRectangle(cornerRadius: 3))
                            .modifier(ColorPanelMeasurement(id: "paint-" + swatch["slot"].string))
                            .padding(.horizontal, 12).padding(.vertical, 4)
                            .frame(maxWidth: .infinity)
                            .frame(height: buttonHeight)
                            .contentShape(Rectangle())
                    }.buttonStyle(PaintSlotButtonStyle(selected: swatch["selected"].bool))
                        .accessibilityLabel(swatch["label"].string)
                        .accessibilityAddTraits(swatch["selected"].bool ? .isSelected : [])
                        .accessibilityIdentifier("color-" + swatch["slot"].string)
                        .modifier(ColorPanelMeasurement(id: swatch["slot"].string))
                }
                Button { color(["op": "swap"]) } label: {
                    Text("Swap").fontWeight(.bold)
                        .frame(width: HeaderTextMetrics.width("Swap", size: textSize, weight: .bold) + 24, height: buttonHeight)
                        .contentShape(Rectangle())
                }.buttonStyle(EditorControlButtonStyle()).fixedSize(horizontal: true, vertical: false)
                    .accessibilityLabel("Swap foreground and background").accessibilityIdentifier("color-swap")
                    .modifier(ColorPanelMeasurement(id: "swap"))
            }.padding(.vertical, 6)
            Button { color(["op": "toggle_space"]) } label: {
                Text(model["space"].string == "hsv" ? "HSV square" : "HLS triangle").fontWeight(.bold)
                    .frame(maxWidth: .infinity).frame(height: buttonHeight).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle()).modifier(ColorPanelMeasurement(id: "space"))
                .padding(.bottom, 8)
                .accessibilityLabel("Switch HSV square / HLS triangle")
                .accessibilityValue(model["space"].string.uppercased()).accessibilityIdentifier("color-space")
            VStack(spacing: 6) {
                ForEach(model["components"].array.indices, id: \.self) { index in
                    let item = model["components"][index]
                    let editingContext = context
                    NumberControl(store: store, label: item["name"].string, value: item["value"].number,
                            control: item["numeric"], identifier: "color-\(index)") { value, completion in
                            guard editingContext == context else { completion(nil); return }
                            store.edit(["type": "color", "action": ["op": "component", "index": index, "value": value]], completion: completion)
                        }.id(context + "-\(index)")
                }
            }
        }.frame(maxWidth: .infinity)
            .modifier(ColorPanelMeasurement(id: "panel"))
            .accessibilityElement(children: .contain).accessibilityIdentifier("color-panel-controls")
            .modifier(ColorPanelCaptureState(value: captureState))
    }
    private func color(_ action: [String: Any]) { store.dispatch(["type": "color", "action": action]) }
}

/// The pixel oracle needs the accepted color, rather than an idealized pointer
/// coordinate. Opt-in debug accessibility metadata adds no visible capture UI.
private struct ColorPanelCaptureState: ViewModifier {
    let value: String?
    func body(content: Content) -> some View {
        if let value { content.accessibilityValue(value) } else { content }
    }
}

private struct ColorSwatchesLayout: Layout {
    let height: CGFloat
    let swapWidth: CGFloat
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        CGSize(width: proposal.width ?? 226, height: height)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let slotWidth = max(30, (bounds.width - swapWidth - 18) / 3)
        for (index, view) in subviews.enumerated() {
            view.place(at: CGPoint(x: bounds.minX + CGFloat(index) * (slotWidth + 6), y: bounds.minY), anchor: .topLeading,
                proposal: ProposedViewSize(width: index == 3 ? swapWidth : slotWidth, height: height))
        }
    }
}

/// The shared paint slots show their alpha over a five-point checkerboard.
/// Selected/pressed buttons replace that background, including transparent paint.
private struct PaintSlotButtonStyle: ButtonStyle {
    let selected: Bool
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.background {
            if configuration.isPressed { Rectangle().fill(.foreground).opacity(0.16) }
            else if selected { EditorPalette.sharedAccent.opacity(0.22) }
            else {
                Canvas { graphics, size in
                    for row in 0..<Int(ceil(size.height / 5)) {
                        for column in 0..<Int(ceil(size.width / 5)) {
                            let level = Double((row + column) % 2 == 0 ? 187 : 136) / 255
                            graphics.fill(Path(CGRect(x: column * 5, y: row * 5, width: 5, height: 5)),
                                with: .color(Color(.sRGB, white: level, opacity: 1)))
                        }
                    }
                }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }
}

private struct MeasureColorPanel: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var measureColorPanel: Bool {
        get { self[MeasureColorPanel.self] }
        set { self[MeasureColorPanel.self] = newValue }
    }
}
struct ColorPanelFrames: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue()) { _, next in next }
    }
}
private struct ColorPanelMeasurement: ViewModifier {
    let id: String
    @Environment(\.measureColorPanel) private var enabled
    func body(content: Content) -> some View {
        if enabled {
            content.background(GeometryReader { proxy in
                Color.clear.preference(key: ColorPanelFrames.self, value: [id: proxy.frame(in: .named("color-capture"))])
            })
        } else { content }
    }
}
