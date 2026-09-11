import SwiftUI

struct ColorPanel: View {
    @ObservedObject var store: EditorStore
    private var model: JSON { store.snapshot["color_panel"] }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var context: String { model["space"].string + store.state["colors"]["paint_slot"].string }
    var body: some View {
        VStack(spacing: 6) {
            GeometryReader { allocation in
                let size = min(allocation.size.width, allocation.size.height)
                ZStack {
                    ColorWheelDrawing(model: model).allowsHitTesting(false)
                    ColorWheelInput(space: model["space"].string == "hls" ? 1 : 0, context: context) { part, point, size in
                        color(["op": "pick", "part": part == 1 ? "hue" : "field",
                            "point": [point.x, point.y], "size": size])
                    }
                }.frame(width: size, height: size)
            }.aspectRatio(1, contentMode: .fit)
            HStack(spacing: 2) {
                ForEach(model["swatches"].array.indices, id: \.self) { index in
                    let swatch = model["swatches"][index]
                    Button { color(["op": "select", "slot": swatch["slot"].raw]) } label: {
                        ColorSwatch(rgba: swatch["rgba"]).frame(height: 20).padding(3)
                            .frame(maxWidth: .infinity)
                            .background(swatch["selected"].bool ? palette.active : Color.clear, in: RoundedRectangle(cornerRadius: 6))
                            .contentShape(Rectangle())
                    }.buttonStyle(.plain).accessibilityLabel(swatch["label"].string)
                        .accessibilityAddTraits(swatch["selected"].bool ? .isSelected : [])
                        .accessibilityIdentifier("color-" + swatch["slot"].string)
                }
                Button { color(["op": "swap"]) } label: {
                    SharedIcon(name: "swap").frame(maxWidth: .infinity).frame(height: 26)
                        .contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityLabel("Swap foreground and background").accessibilityIdentifier("color-swap")
                Button { color(["op": "toggle_space"]) } label: {
                    ColorSpaceSymbol(triangle: model["space"].string == "hsv")
                        .stroke(lineWidth: 1.2).frame(width: 14, height: 20).frame(maxWidth: .infinity).frame(height: 26)
                        .contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityLabel("Switch HSV square / HLS triangle")
                    .accessibilityValue(model["space"].string.uppercased()).accessibilityIdentifier("color-space")
            }
            HStack(spacing: 2) {
                ForEach(model["components"].array.indices, id: \.self) { index in
                    let item = model["components"][index]
                    let editingContext = context
                    VStack(spacing: 0) {
                        Text(item["label"].string).opacity(0.55)
                        NumberControl(store: store, label: item["name"].string, value: item["value"].number,
                            control: item["numeric"], identifier: "color-\(index)", valueOnly: true) { value, completion in
                            guard editingContext == context else { completion(nil); return }
                            store.edit(["type": "color", "action": ["op": "component", "index": index, "value": value]], completion: completion)
                        }.id(context + "-\(index)")
                    }.frame(maxWidth: .infinity)
                }
            }
        }.frame(maxWidth: .infinity)
    }
    private func color(_ action: [String: Any]) { store.dispatch(["type": "color", "action": action]) }
}

private struct ColorWheelDrawing: View {
    let model: JSON
    var body: some View {
        GeometryReader { allocation in
            let size = min(allocation.size.width, allocation.size.height)
            let geometry = model["geometry"]
            let hue = model["hue_color"].paintColor
            ZStack(alignment: .topLeading) {
                if model["space"].string == "hsv" {
                    let square = geometry["square"]
                    Rectangle().fill(.linearGradient(Gradient(colors: [.white, hue]).colorSpace(.device), startPoint: .leading, endPoint: .trailing))
                        .overlay(Rectangle().fill(.linearGradient(Gradient(colors: [.clear, .black]).colorSpace(.device), startPoint: .top, endPoint: .bottom)))
                        .frame(width: square[2].number * size, height: square[2].number * size)
                        .offset(x: square[0].number * size, y: square[1].number * size)
                } else {
                    let vertices = geometry["triangle"].array
                    // The equilateral triangle's white and hue barycentric
                    // weights are affine gradients. Add them in device color
                    // space to match Rust's display-encoded interpolation.
                    let white = UnitPoint(x: vertices[0][0].number, y: vertices[0][1].number)
                    let opposite = UnitPoint(x: (vertices[1][0].number + vertices[2][0].number) / 2,
                        y: (vertices[1][1].number + vertices[2][1].number) / 2)
                    Rectangle().fill(.linearGradient(Gradient(colors: [.white, .black]).colorSpace(.device),
                        startPoint: white, endPoint: opposite))
                        .overlay(Rectangle().fill(.linearGradient(Gradient(colors: [.black, hue]).colorSpace(.device),
                            startPoint: UnitPoint(x: vertices[0][0].number, y: 0.5),
                            endPoint: UnitPoint(x: vertices[2][0].number, y: 0.5))).blendMode(.plusLighter))
                        .compositingGroup().clipShape(ColorTriangle(vertices: vertices))
                }
                Circle().stroke(.angularGradient(Gradient(colors: model["hue_stops"].array.map(\.paintColor)).colorSpace(.device), center: .center,
                    startAngle: .degrees(model["hue_start_degrees"].number), endAngle: .degrees(model["hue_start_degrees"].number + 360)),
                    lineWidth: (geometry["outer"].number - geometry["inner"].number) * size)
                    .frame(width: (geometry["outer"].number + geometry["inner"].number) * size,
                        height: (geometry["outer"].number + geometry["inner"].number) * size)
                    .position(x: geometry["center"][0].number * size, y: geometry["center"][1].number * size)
                ForEach(["hue_marker", "field_marker"], id: \.self) { key in
                    Circle().stroke(.black, lineWidth: 3).overlay(Circle().stroke(.white, lineWidth: 1.5))
                        .frame(width: 7, height: 7).position(x: model[key][0].number * size, y: model[key][1].number * size)
                }
            }.frame(width: size, height: size)
        }
    }
}

private struct ColorTriangle: Shape {
    let vertices: [JSON]
    func path(in rect: CGRect) -> Path {
        Path { path in
            path.addLines(vertices.map { CGPoint(x: $0[0].number * rect.width, y: $0[1].number * rect.height) })
            path.closeSubpath()
        }
    }
}
private struct ColorSpaceSymbol: Shape {
    let triangle: Bool
    func path(in rect: CGRect) -> Path {
        Path { path in
            if triangle {
                path.addLines([CGPoint(x: 2, y: rect.height - 4), CGPoint(x: rect.width / 2, y: 4), CGPoint(x: rect.width - 2, y: rect.height - 4)])
                path.closeSubpath()
            } else { path.addRect(rect.insetBy(dx: 2, dy: 4)) }
        }
    }
}
struct ColorSwatch: View {
    let rgba: JSON
    var body: some View {
        Canvas { graphics, size in
            for row in 0..<Int(ceil(size.height / 5)) {
                for column in 0..<Int(ceil(size.width / 5)) {
                    let level = (row + column) % 2 == 0 ? 0.8 : 0.55
                    graphics.fill(Path(CGRect(x: column * 5, y: row * 5, width: 5, height: 5)), with: .color(Color(.sRGB, white: level, opacity: 1)))
                }
            }
            graphics.fill(Path(CGRect(origin: .zero, size: size)), with: .color(rgba.paintColor))
        }.clipped()
    }
}
private extension JSON {
    var paintColor: Color { Color(.sRGB, red: self[0].number, green: self[1].number, blue: self[2].number,
        opacity: self[3].isNull ? 1 : self[3].number) }
}
