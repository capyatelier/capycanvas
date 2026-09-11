import SwiftUI

struct PanelControls: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        if panel["id"].string == "layers" { LayerPanel(store: store, panel: panel) }
        else { controls }
    }
    private var controls: some View {
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
        case "brushes": ToolSetControls(store: store)
        case "tool_settings": ToolSettingsControls(store: store)
        case "brush_size": number("Brush size", key: "diameter", spec: "brush_size", action: "set_brush_size")
        case "brush_opacity": number("Brush opacity", key: "opacity", spec: "opacity", action: "set_brush_opacity")
        case "size_presets": sizes
        case "brush_color":
            ForEach(0..<3, id: \.self) { component in
                NumberControl(store: store, label: ["Red", "Green", "Blue"][component], value: store.state["brush"]["color"][component].number, control: store.catalog["opacity"]) { value, completion in
                    var rgba = store.state["brush"]["color"].array.map(\.number); rgba[component] = value
                    store.edit(["type": "set_color", "rgba": rgba], completion: completion)
                }
            }
        default: Text(item["label"].string).fontWeight(.bold)
        }
    }
    private func number(_ label: String, key: String, spec: String, action: String) -> some View {
        NumberControl(store: store, label: label, value: store.state["brush"][key].number, control: store.catalog[spec]) { value, completion in
            store.edit(["type": action, "value": value], completion: completion)
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

}
