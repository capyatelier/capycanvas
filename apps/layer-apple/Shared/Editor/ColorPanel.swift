import SwiftUI

struct ColorPanel: View {
    @ObservedObject var store: EditorStore
    @StateObject private var resources = ColorPanelLayoutCache()
    @FocusState private var readoutFocused: Bool
    private var model: JSON { store.colorPanel }
    private var hdr: Bool { model["hdr"].bool }
    private var context: String {
        "\(store.state["document_file"]["epoch"].uint):\(model["rgb_space"].string):\(model["shape"].string):\(store.displayColors["paint_slot"].string)"
    }
    private var captureState: String? {
        #if DEBUG
        guard ProcessInfo.processInfo.environment["CAPY_COLOR_PROBE"] == "1" else { return nil }
        return try? JSON(["space": model["space"].raw, "shape": model["shape"].raw,
            "hue": model["wheel_components"][0].raw, "components": model["components"].array.map { $0["value"].raw },
            "rgba": store.state["brush"]["color"].raw]).encoded()
        #else
        return nil
        #endif
    }
    var body: some View {
        GeometryReader { allocation in
            let full = ColorUI.resolve(["type": "picker_layout", "size": max(128, allocation.size.width), "hdr": hdr])
            let side = max(128, allocation.size.width * min(1, allocation.size.height / max(1, full["height"].number)))
            let spec = resources.layout(side: side, hdr: hdr)
            let layout = spec["layout"], height = spec["height"].number
            let palette = EditorPalette(source: store.state["palette"])
            ZStack(alignment: .topLeading) {
                ColorWheelDrawing(model: model, bounds: layout["wheel"], viewing: hdr ? store.colorViewing : JSON(),
                    previewing: store.colorPreviewing)
                    .frame(width: side, height: side).allowsHitTesting(false)
                ColorWheelInput(shape: ColorWheelShape(model["shape"].string).rawValue, context: context,
                    value: captureState ?? model["readout_description"].string) { part, point, size in
                    color(["op": "pick_wheel", "part": part == 1 ? "hue" : "field",
                        "point": [point.x, point.y], "size": size])
                }.colorPlaced(layout["wheel"], id: "wheel")
                ForEach([true, false], id: \.self) { white in
                    let quick = model["quick_colors"].array.first { $0["white"].bool == white } ?? JSON()
                    let name = white ? "white" : "black"
                    Button { color(["op": "quick_color", "white": white]) } label: {
                        Circle().fill(quick["rgba"].paintColor).padding(1).contentShape(Circle())
                    }.buttonStyle(ColorPanelButtonStyle(kind: .paint(quick["selected"].bool), palette: palette))
                        .clipShape(Circle()).contentShape(Circle())
                        .accessibilityLabel(quick["label"].string)
                        .accessibilityAddTraits(quick["selected"].bool ? .isSelected : [])
                        .accessibilityIdentifier("color-quick-" + name)
                        .colorPlaced(layout[name], id: "quick-" + name)
                }
                // Foreground is above background for both painting and hit testing.
                ForEach(["background", "foreground", "transparent"], id: \.self) { slot in
                    let swatch = model["swatches"].array.first { $0["slot"].string == slot } ?? JSON()
                    Button { color(["op": "select", "slot": slot]) } label: {
                        Group {
                            if hdr { HDRColorSwatch(color: slot == "transparent" ? JSON(["space": "Srgb", "rgba": [0, 0, 0, 0]]) : store.panelColors[slot], viewing: store.colorViewing).clipShape(Circle()) }
                            else { ColorPaintPreview(rgba: swatch["rgba"]) }
                        }.modifier(ColorPanelMeasurement(id: "paint-" + slot))
                            .padding(slot == "foreground" ? 3 : 1)
                            .contentShape(Circle())
                    }.buttonStyle(ColorPanelButtonStyle(kind: .paint(swatch["selected"].bool), palette: palette))
                        .clipShape(Circle()).contentShape(Circle())
                        .accessibilityLabel(swatch["label"].string)
                        .accessibilityAddTraits(swatch["selected"].bool ? .isSelected : [])
                        .accessibilityIdentifier("color-" + slot)
                        .colorPlaced(layout[slot], id: slot)
                }
                PaintColorControls(store: store, compact: true).colorPlaced(layout["edit"], id: "edit")
                if hdr {
                    HDRIntensityArc(store: store, geometry: spec["arc"], caption: layout["intensity_caption"], size: side)
                        .frame(width: side, height: height)
                }
                ForEach(0..<2, id: \.self) { index in
                    let shape = model["other_shapes"][index].string
                    Button { color(["op": "shape", "shape": shape]) } label: {
                        SharedIcon(name: "color-" + shape)
                            #if os(macOS)
                            // Keep the Mac asset outline smooth during rotation.
                            .drawingGroup()
                            #endif
                            .rotationEffect(.degrees(layout["shape_rotations"][index].number))
                            .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Circle())
                    }.buttonStyle(ColorPanelButtonStyle(kind: .shape, palette: palette))
                        .accessibilityLabel("Use \(shape == "circle" ? "Okhsv" : shape == "triangle" ? "HLS" : "HSV") \(shape)")
                        .accessibilityIdentifier("color-shape-" + shape)
                        .colorPlaced(layout["shapes"][index], id: "shape-\(index)")
                }
                Button { color(["op": "swap"]) } label: {
                    SharedIcon(name: "color-swap")
                        .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Circle())
                }.buttonStyle(ColorPanelButtonStyle(kind: .swap, palette: palette))
                    .accessibilityLabel("Swap foreground and background").accessibilityIdentifier("color-swap")
                    .colorPlaced(layout["swap"], id: "swap")
                let readoutHit = ColorReadoutHit(radius: layout["wheel"][2].number * model["geometry"]["outer"].number + 2)
                Button { color(["op": "toggle_readout"]) } label: {
                    ColorReadoutDrawing(model: model, half: layout["readout"][2].number,
                        radius: layout["readout_radius"].number,
                        ink: readoutFocused ? palette.accent : palette["text"], focused: readoutFocused)
                        .contentShape(readoutHit)
                }.buttonStyle(.plain).clipShape(readoutHit).contentShape(readoutHit)
                    .focused($readoutFocused).focusEffectDisabled()
                    .accessibilityLabel(model["readout_description"].string)
                    .accessibilityValue(model["readout_label"].string).accessibilityIdentifier("color-readout")
                    .colorPlaced(layout["readout"], id: "readout")
            }.frame(width: side, height: height, alignment: .topLeading)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }.aspectRatio(1 / Self.aspect(hdr: hdr), contentMode: .fit).frame(minWidth: 128, minHeight: 128)
            .modifier(ColorPanelMeasurement(id: "panel"))
            .accessibilityElement(children: .contain).accessibilityIdentifier("color-panel-controls")
    }
    private func color(_ action: [String: Any]) { store.dispatch(["type": "color", "action": action]) }
    static func aspect(hdr: Bool) -> CGFloat {
        hdr ? max(226, ColorUI.resolve(["type": "picker_layout", "size": 226, "hdr": true])["height"].number) / 226 : 1
    }
}

/// Native buttons retain press/cancel and keyboard behavior. Only the shared
/// paint, shape and Swap feedback differs between their visual roles.
private struct ColorPanelButtonStyle: ButtonStyle {
    enum Kind: Equatable { case paint(Bool), shape, swap }
    let kind: Kind
    let palette: EditorPalette
    func makeBody(configuration: Configuration) -> some View {
        Content(configuration: configuration, kind: kind, palette: palette)
    }
    private struct Content: View {
        let configuration: Configuration
        let kind: Kind
        let palette: EditorPalette
        @State private var hovered = false
        var body: some View {
            configuration.label
                .foregroundStyle(kind == .shape && hovered ? palette.accent : palette["text"])
                .background {
                    if case .paint(let selected) = kind {
                        Circle().strokeBorder(palette["text"].opacity(selected || hovered ? 1 : 0.25),
                            lineWidth: selected || hovered ? 2 : 1)
                    }
                }
                .background {
                    switch kind {
                    case .paint:
                        Circle().fill(configuration.isPressed ? palette["text"].opacity(0.16)
                            : hovered ? palette["text"].opacity(0.08) : palette["panel"])
                    case .swap:
                        Circle().fill(palette["text"].opacity(configuration.isPressed ? 0.16 : hovered ? 0.12 : 0))
                    case .shape: EmptyView()
                    }
                }
                .onHover { hovered = $0 }
        }
    }
}

private struct ColorPaintPreview: View {
    let rgba: JSON
    @Environment(\.editorPalette) private var palette
    var body: some View {
        Canvas(colorMode: .extendedLinear) { [palette] graphics, size in
            graphics.fillTransparencyChecker(size, palette: palette)
            graphics.fill(Path(CGRect(origin: .zero, size: size)), with: .color(rgba.paintColor))
        }.clipShape(Circle())
    }
}

/// The readout occupies the corner outside the hue ring. Its clipped native
/// button leaves the entire visible ring available to the wheel recognizer.
struct ColorReadoutHit: Shape {
    let radius: CGFloat
    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: rect.origin)
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY - radius))
        path.addArc(center: CGPoint(x: rect.maxX, y: rect.maxY), radius: radius,
            startAngle: .degrees(-90), endAngle: .degrees(-180), clockwise: true)
        path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.closeSubpath()
        return path
    }
}

private struct ColorReadoutDrawing: View {
    let model: JSON
    let half: CGFloat
    let radius: CGFloat
    let ink: Color
    let focused: Bool
    private var texts: [String] { model["readout_layout_text"].array.map(\.string) }
    private var rgb: Bool { model["readout"].string == "rgb" }
    private var labelSize: CGFloat { min(12, max(9, half * 2 * 0.044)) }
    private var metrics: (font: CGFloat, glyphs: [Character: CGFloat], digit: CGFloat, widths: [CGFloat]) {
        var font = labelSize
        let glyphs = Set(texts.joined() + "0123456789")
        while true {
            let widths = Dictionary(uniqueKeysWithValues: glyphs.map {
                ($0, EditorTextMetrics.width(String($0), size: font, weight: .regular))
            })
            let digit = "0123456789".compactMap { widths[$0] }.max() ?? 0
            let spans = texts.map { text in
                text.reduce(CGFloat(0)) { $0 + ($1.isNumber || $1 == " " ? digit : widths[$1, default: 0]) }
                    + (rgb ? font * 0.8 + 2 : 0)
            }
            if spans.reduce(0, +) + 6 <= radius * .pi / 2 - 4 || font <= 8 {
                return (font, widths, digit, spans)
            }
            font -= 0.25
        }
    }
    var body: some View {
        let metrics = metrics
        Canvas { graphics, _ in
            let font = metrics.font, widths = metrics.widths
            EditorTextMetrics.draw(model["readout_label"].string, size: labelSize, weight: .bold,
                in: graphics, baseline: CGPoint(x: 2, y: labelSize + 1), color: ink.opacity(0.9))
            if focused {
                let width = EditorTextMetrics.width(model["readout_label"].string, size: labelSize, weight: .bold) + 5
                graphics.stroke(Path(roundedRect: CGRect(x: 1, y: 1, width: width, height: labelSize + 4),
                    cornerRadius: 5), with: .color(ink.opacity(0.9)), lineWidth: 1.5)
            }
            let available = radius * .pi / 2 - 4
            func width(_ glyph: Character) -> CGFloat { metrics.glyphs[glyph, default: 0] }
            func advance(_ glyph: Character) -> CGFloat { glyph.isNumber || glyph == " " ? metrics.digit : width(glyph) }
            let total = widths.reduce(0, +), chip = font * 0.8
            let gap = min(radius * 0.24, max(3, (available - total) * 0.5))
            var cursor = -(total + gap * 2) * 0.5
            func transformed(_ angle: CGFloat) -> GraphicsContext {
                var local = graphics
                local.translateBy(x: half + radius * cos(angle), y: half + radius * sin(angle))
                local.rotate(by: .radians(angle + .pi / 2))
                return local
            }
            for (index, text) in texts.enumerated() {
                let span = widths[index], mid = -3 * CGFloat.pi / 4 + (cursor + span * 0.5) / radius
                cursor += span + gap
                var along = -span * 0.5
                if rgb {
                    let local = transformed(mid + (along + chip * 0.5) / radius)
                    let colors = [Color(.sRGB, red: 0.93, green: 0.31, blue: 0.36),
                                  Color(.sRGB, red: 0.25, green: 0.73, blue: 0.43),
                                  Color(.sRGB, red: 0.29, green: 0.56, blue: 0.98)]
                    local.fill(Path(roundedRect: CGRect(x: -chip * 0.5, y: -font * 0.76, width: chip, height: chip),
                        cornerRadius: 2), with: .color(colors[index]))
                    along += chip + 2
                }
                for glyph in text {
                    let cell = advance(glyph)
                    let local = transformed(mid + (along + cell * 0.5) / radius)
                    EditorTextMetrics.draw(String(glyph), size: font, weight: .regular,
                        in: local, baseline: CGPoint(x: -width(glyph) * 0.5, y: 0), color: ink.opacity(0.8))
                    along += cell
                }
            }
        }
    }
}

private extension View {
    func colorPlaced(_ bounds: JSON, id: String) -> some View {
        frame(width: bounds[2].number, height: bounds[3].number)
            .modifier(ColorPanelMeasurement(id: id))
            .offset(x: bounds[0].number, y: bounds[1].number)
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
        value.merge(nextValue()) { _, new in new }
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
