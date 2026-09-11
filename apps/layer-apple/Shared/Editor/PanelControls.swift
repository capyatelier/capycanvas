import SwiftUI

struct PanelControls: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    var scrollable = true
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        if panel["id"].string == "layers" { LayerPanel(store: store, panel: panel) }
        else if panel["id"].string == "adjustments" {
            if panel["controls"].array.contains(where: { $0["control"].string == "adjustments" && $0["visible_in_panel"].bool }) {
                AdjustmentPanel(store: store)
            }
        }
        else if panel["id"].string == "navigator" {
            if panel["controls"].array.contains(where: { $0["control"].string == "navigator" && $0["visible_in_panel"].bool }) {
                NavigatorPanel(store: store)
            }
        }
        else { controls }
    }
    private var controls: some View {
        Group {
            if scrollable { ScrollView { controlBody }.frame(maxWidth: .infinity, maxHeight: .infinity) }
            else { controlBody }
        }
    }
    private var controlBody: some View {
        VStack(alignment: .leading, spacing: 12) {
                ForEach(panel["controls"].array.indices, id: \.self) { index in
                    let item = panel["controls"][index]
                    if item["visible_in_panel"].bool { control(item) }
                }
        }.padding(panel["id"].string == "properties" || panel["id"].string == "stats" ? 6 : 8)
            .frame(maxWidth: .infinity, alignment: .topLeading)
    }
    @ViewBuilder func control(_ item: JSON) -> some View {
        switch item["control"].string {
        case "brushes": ToolSetControls(store: store)
        case "tool_settings": ToolSettingsControls(store: store)
        case "color_wheel": ColorPanel(store: store)
        case "properties": LayerPropertiesPanel(store: store)
        case "stats": RendererStatsPanel(store: store, stats: store.rendererStats)
        case "brush_size": number("Brush size", key: "diameter", spec: "brush_size", action: "set_brush_size")
        case "brush_opacity": number("Brush opacity", key: "opacity", spec: "opacity", action: "set_brush_opacity")
        case "size_presets": sizes
        case "brush_color":
            ForEach(0..<3, id: \.self) { component in
                NumberControl(store: store, label: ["Red", "Green", "Blue"][component], value: store.state["brush"]["color"][component].number, control: store.catalog["opacity"]) { value, completion in
                    store.edit(["type": "color", "action": ["op": "rgba_component", "index": component, "value": value]], completion: completion)
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
        let lineHeight = max(1, store.catalog["text_size_pt"].number * 4 / 3) * 1.42
        return SizePresetsLayout(cellHeight: 42 + lineHeight) {
            ForEach(store.catalog["brush_sizes"].array.indices, id: \.self) { index in
                let size = store.catalog["brush_sizes"][index].number
                Button { store.dispatch(["type": "set_brush_size", "value": size]) } label: {
                    VStack(spacing: 4) {
                        Circle().frame(width: min(27, 2 + sqrt(size) * 1.2), height: min(27, 2 + sqrt(size) * 1.2)).frame(height: 28)
                        Text(String(Int(size))).frame(height: lineHeight)
                    }.padding(2).frame(maxWidth: .infinity).frame(height: 36 + lineHeight)
                        .background(size == store.state["brush"]["diameter"].number ? palette.active : Color.clear, in: RoundedRectangle(cornerRadius: 6))
                }.buttonStyle(.plain).padding(3)
            }
        }
    }

}

/// The same two/three/four-column breakpoints and cell spacing as the web and
/// Android panels. Intrinsic font height is accounted for before Rust measures
/// the surrounding panel; wide drawers keep four columns instead of adding more.
private struct SizePresetsLayout: Layout {
    let cellHeight: CGFloat
    private func columns(_ width: CGFloat) -> Int { width < 130 ? 2 : width < 174 ? 3 : 4 }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let requested = proposal.width ?? 224
        let width = requested.isFinite ? max(0, requested) : 224
        let rows = (subviews.count + columns(width) - 1) / columns(width)
        return CGSize(width: width, height: CGFloat(rows) * cellHeight + CGFloat(max(0, rows - 1)) * 4)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let count = columns(bounds.width)
        let width = max(0, (bounds.width - CGFloat(count - 1) * 2) / CGFloat(count))
        for (index, view) in subviews.enumerated() {
            view.place(at: CGPoint(x: bounds.minX + CGFloat(index % count) * (width + 2),
                y: bounds.minY + CGFloat(index / count) * (cellHeight + 4)), anchor: .topLeading,
                proposal: ProposedViewSize(width: width, height: cellHeight))
        }
    }
}
