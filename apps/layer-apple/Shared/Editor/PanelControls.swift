import SwiftUI

struct PanelControls: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                ForEach(panel["controls"].array.indices, id: \.self) { index in
                    let item = panel["controls"][index]
                    if item["visible_in_panel"].bool { control(item) }
                }
            }.padding(8).frame(maxWidth: .infinity, alignment: .topLeading)
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }
    @ViewBuilder private func control(_ item: JSON) -> some View {
        switch item["control"].string {
        case "brushes": brushes
        case "brush_size": number("Brush size", key: "diameter", spec: "brush_size", action: "set_brush_size")
        case "brush_opacity": number("Brush opacity", key: "opacity", spec: "opacity", action: "set_brush_opacity")
        case "size_presets": sizes
        case "brush_color":
            ForEach(0..<3, id: \.self) { component in
                NumberControl(store: store, label: ["Red", "Green", "Blue"][component], value: store.state["brush"]["color"][component].number, control: store.catalog["opacity"]) { value in
                    var rgba = store.state["brush"]["color"].array.map(\.number); rgba[component] = value
                    store.dispatch(["type": "set_color", "rgba": rgba])
                }
            }
        case "layers": layers
        case "layer_actions":
            HStack(spacing: 4) {
                ForEach(store.catalog["layer_commands"].array.indices, id: \.self) { i in
                    let command = store.command(store.catalog["layer_commands"][i].string)
                    IconTile(icon: command["icon"].string, label: command["label"].string, enabled: command["enabled"].bool) {
                        store.invoke(command["id"].string)
                    }.frame(height: 28)
                }
            }
        case "layer_opacity":
            NumberControl(store: store, label: "Opacity", value: store.state["layer_tools"]["editing_layer"]["opacity"].number, control: store.catalog["layer_opacity"]) {
                store.dispatch(["type": "set_layer_opacity", "opacity": $0])
            }
        default: Text(item["label"].string).fontWeight(.bold)
        }
    }
    private func number(_ label: String, key: String, spec: String, action: String) -> some View {
        NumberControl(store: store, label: label, value: store.state["brush"][key].number, control: store.catalog[spec]) {
            store.dispatch(["type": action, "value": $0])
        }
    }
    private var brushes: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(store.catalog["brush_categories"].array.indices, id: \.self) { categoryIndex in
                let category = store.catalog["brush_categories"][categoryIndex]
                Text(category["label"].string).fontWeight(.bold).opacity(0.55).padding(8)
                ForEach(category["brushes"].array.indices, id: \.self) { index in
                    let brush = category["brushes"][index]
                    Button { store.dispatch(["type": "select_brush", "id": brush["id"].raw]) } label: {
                        VStack(alignment: .trailing, spacing: 0) {
                            Image("preview-\(brush["id"].uint)-\(store.state["theme"].string)").resizable().frame(height: 40)
                            Text(brush["label"].string).fontWeight(.bold)
                        }.padding(.horizontal, 6).padding(.vertical, 3).frame(maxWidth: .infinity)
                            .background(brush["id"].uint == store.state["brush"]["preset"].uint ? palette.active : Color.clear, in: RoundedRectangle(cornerRadius: 6))
                    }.buttonStyle(.plain).accessibilityIdentifier("brush-\(brush["id"].uint)")
                }
            }
        }
    }
    private var sizes: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 42), spacing: 2)], spacing: 4) {
            ForEach(store.catalog["brush_sizes"].array.indices, id: \.self) { index in
                let size = store.catalog["brush_sizes"][index].number
                Button { store.dispatch(["type": "set_brush_size", "value": size]) } label: {
                    VStack(spacing: 4) {
                        Circle().frame(width: min(27, 2 + sqrt(size) * 1.2), height: min(27, 2 + sqrt(size) * 1.2)).frame(height: 28)
                        Text(String(Int(size)))
                    }.padding(5).frame(maxWidth: .infinity)
                        .background(size == store.state["brush"]["diameter"].number ? palette.active : Color.clear, in: RoundedRectangle(cornerRadius: 6))
                }.buttonStyle(.plain)
            }
        }
    }
    private var layers: some View {
        VStack(spacing: 2) {
            ForEach(store.state["layers"].array.indices, id: \.self) { index in
                let layer = store.state["layers"][index]
                HStack(spacing: 6) {
                    IconTile(icon: layer["visible"].bool ? "eye" : "eye-hidden", label: "Layer visibility") {
                        store.dispatch(["type": "set_layer_visibility", "id": layer["id"].raw, "visible": !layer["visible"].bool])
                    }.frame(width: 24, height: 32)
                    Button { store.dispatch(["type": "select_layer", "id": layer["id"].raw]) } label: {
                        HStack {
                            Rectangle().fill(palette["input"]).frame(width: 32, height: 32)
                            Text(layer["label"].string).lineLimit(1)
                            Spacer(minLength: 0)
                        }.contentShape(Rectangle())
                    }.buttonStyle(.plain)
                }.padding(4).background(layer["selected"].bool ? palette.active : Color.clear, in: RoundedRectangle(cornerRadius: 6))
            }
        }
    }
}

struct NumberControl: View {
    @ObservedObject var store: EditorStore
    let label: String
    let value: Double
    let control: JSON
    let change: (Double) -> Void
    @State private var text = ""
    @State private var fill = 0.0
    @FocusState private var editing: Bool
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label)
                Spacer()
                TextField(label, text: $text).multilineTextAlignment(.trailing).frame(width: 90)
                    .textFieldStyle(.plain).focused($editing).onSubmit { resolve(["type": "expression", "text": text]) }
            }
            Slider(value: Binding(get: { fill }, set: { fill = $0; resolve(["type": "position", "position": $0]) }), in: 0...1).accessibilityLabel(label)
        }
        .onAppear { format() }.onChange(of: value) { _, _ in if !editing { format() } }
        .onChange(of: editing) { old, current in if old && !current { resolve(["type": "expression", "text": text]) } }
    }
    private func format() {
        store.numeric(control, value: value, operation: ["type": "format"]) { result in
            if !editing { text = result["edit"].string }; fill = result["fill"].number
        }
    }
    private func resolve(_ operation: [String: Any]) {
        store.numeric(control, value: value, operation: operation) { result in
            change(result["value"].number)
        }
    }
}
