import SwiftUI

struct BrushColorButton: View {
    @ObservedObject var store: EditorStore
    let label: String
    var body: some View {
        Button { store.customize(["type": "open_control", "control": "brush_color"]) } label: {
            Group {
                if store.snapshot["color_panel"]["hdr"].bool { HDRColorSwatch(color: store.snapshot["color_panel"]["definition"], viewing: store.colorViewing) }
                else { ColorSwatch(rgba: store.snapshot["paint_pair"]["rgba"]) }
            }
                .clipShape(RoundedRectangle(cornerRadius: 4))
                .padding(.horizontal, 12).padding(.vertical, 4).frame(height: 34)
                .background(EditorPalette(source: store.state["palette"])["button"].opacity(13 / 255),
                    in: SquircleShape.control)
                .contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityLabel(label).accessibilityIdentifier("brush-color")
    }
}

struct PanelControls: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    var scrollable = true
    var measureForWorkspace = true
    var splitFilters = false
    var drawerToolSet = JSON()
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var fitsColorWheel: Bool {
        panel["id"].string == "color" && panel["controls"].array.filter { $0["visible_in_panel"].bool }.map { $0["control"].string } == ["color_wheel"]
    }
    private var padding: CGFloat {
        if !measureForWorkspace && store.state["customization"]["drawer"]["compact"].bool { return 12 }
        return ["properties", "stats", "histogram", "waveform"].contains(panel["id"].string) ? 6 : 8
    }
    var body: some View {
        contents.environment(\.measuresWorkspacePanel, measureForWorkspace)
            .background {
                Color.clear.preference(key: PanelSizeFacts.self, value: measureForWorkspace
                    ? [PanelSizeKey(panel: panel["id"].string, part: "present"): 0] : [:])
            }
    }
    @ViewBuilder private var contents: some View {
        if panel["id"].string == "proof" { ProofPanel(store: store, controller: store.proof) }
        else if panel["id"].string == "layers" { LayerPanel(store: store, panel: panel) }
        else if panel["id"].string == "palettes" { PalettePanel(store: store, controller: store.palettes, docked: scrollable) }
        else if panel["id"].string == "filter_types" { FilterTypesPanel(store: store) }
        else if panel["id"].string == "adjustments" {
            if panel["controls"].array.contains(where: { $0["control"].string == "adjustments" && $0["visible_in_panel"].bool }) {
                AdjustmentPanel(store: store, splitPicker: splitFilters)
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
            if scrollable {
                GeometryReader { viewport in
                    EditorScrollView(showsIndicators: panel["id"].string != "color") {
                        controlBody(maximumHeight: max(128, viewport.size.height - padding * 2))
                    }
                    // Workspace layout already reserves space above the keyboard.
                    .ignoresSafeArea(.keyboard)
                }
            } else { controlBody() }
        }
    }
    private func controlBody(maximumHeight: CGFloat? = nil) -> some View {
        VStack(alignment: .leading, spacing: 12) {
                ForEach(panel["controls"].array.indices, id: \.self) { index in
                    let item = panel["controls"][index]
                    if item["visible_in_panel"].bool { control(item, maximumHeight: maximumHeight) }
                }
        }.padding(padding)
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .modifier(PanelBodyMeasurement(panel: panel["id"].string,
                kind: scrollable && !["color", "histogram", "waveform"].contains(panel["id"].string) ? .scroll : .fixed,
                naturalHeight: fitsColorWheel ? { [padding, ratio = ColorPanel.aspect(hdr: store.snapshot["color_panel"]["hdr"].bool)] width in
                    max(128, width - padding * 2) * ratio + padding * 2 } : nil))
    }
    @ViewBuilder func control(_ item: JSON, maximumHeight: CGFloat? = nil) -> some View {
        switch item["control"].string {
        case "brushes", "brush_sets", "sculpt_sets", "tools":
            ToolSetControls(store: store, panel: panel["id"].string, drawerToolSet: drawerToolSet)
        case "tool_settings": ToolSettingsControls(store: store)
        // Match the shared panel's fit-to-viewport wheel while retaining its
        // readable minimum size and scrolling for smaller/customized panels.
        case "color_wheel":
            VStack(alignment: .leading, spacing: 8) {
                ColorPanel(store: store)
            }.frame(maxHeight: maximumHeight)
        case "properties": LayerPropertiesPanel(store: store)
        case "histogram", "waveform": ScopeControl(store: store, kind: item["control"].string)
        case "stats": RendererStatsPanel(store: store, stats: store.rendererStats)
        case "brush_size": number("Brush size", key: "diameter", spec: "brush_size", action: "set_brush_size")
        case "brush_opacity": number("Brush opacity", key: "opacity", spec: "opacity", action: "set_brush_opacity")
        case "size_presets": sizes
        case "brush_color":
            BrushColorButton(store: store, label: item["label"].string)
            PaintColorControls(store: store)
        default: Text(item["label"].string).fontWeight(.bold)
        }
    }
    private func number(_ label: String, key: String, spec: String, action: String) -> some View {
        let preset = store.state["brush"]["preset"].uint
        return NumberControl(store: store, label: label, value: store.state["brush"][key].number, control: store.catalog[spec], reset: {
            guard preset == store.state["brush"]["preset"].uint else { return }
            store.edit(["type": "reset_tool_setting", "id": key == "diameter" ? "size" : "opacity"]) { _ in }
        }) { value, completion in
            guard preset == store.state["brush"]["preset"].uint else { completion(nil); return }
            store.edit(["type": action, "value": value], completion: completion)
        }.id(preset)
    }
    private var sizes: some View { BrushSizeGrid(store: store, identifier: "size") }
}

struct BrushSizeGrid: View {
    @ObservedObject var store: EditorStore
    let identifier: String
    var body: some View {
        let style = ToolbarUI.cached(["type": "style", "style": "small"], language: store.interfaceLanguage)
        let tile = store.catalog["brush_size_tile"], width = tile[0].number, height = tile[1].number
        let shape = SquircleShape(style["size"][0].number / 2)
        BrushSizeLayout(tile: CGSize(width: width, height: height), gap: style["gap"].number) {
            ForEach(store.catalog["brush_sizes"].array.indices, id: \.self) { index in
                let preset = store.catalog["brush_sizes"][index], value = preset["value"].number
                Button { store.dispatch(["type": "set_brush_size", "value": value]) } label: {
                    ZStack(alignment: .top) {
                        Circle().fill(.foreground).frame(width: preset["preview_diameter"].number, height: preset["preview_diameter"].number)
                            .frame(width: width, height: width)
                            .frame(maxHeight: .infinity, alignment: .top)
                            .mask(LinearGradient(stops: [.init(color: .black, location: 0.4), .init(color: .black.opacity(0.2), location: 0.65),
                                .init(color: .clear, location: 1)], startPoint: .top, endPoint: .bottom))
                        Text(preset["label"].string).lineLimit(1).frame(maxHeight: .infinity, alignment: .bottom).padding(.bottom, 2)
                    }.frame(width: width, height: height).contentShape(shape)
                }.buttonStyle(EditorControlButtonStyle(selected: value == store.state["brush"]["diameter"].number,
                    corner: .radius(style["size"][0].number / 2)))
                    .accessibilityLabel("\(preset["label"].string) px")
                    .accessibilityAddTraits(value == store.state["brush"]["diameter"].number ? .isSelected : [])
                    .accessibilityIdentifier("\(identifier)-\(preset["label"].string)")
            }
        }
    }
}

private struct BrushSizeLayout: Layout {
    let tile: CGSize
    let gap: CGFloat
    private func columns(_ width: CGFloat) -> Int { max(1, Int((width + gap) / (tile.width + gap))) }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let requested = proposal.width ?? 6 * (tile.width + gap) - gap
        let width = requested.isFinite ? max(tile.width, requested) : 6 * (tile.width + gap) - gap
        let rows = (subviews.count + columns(width) - 1) / columns(width)
        return CGSize(width: width, height: CGFloat(rows) * tile.height + CGFloat(max(0, rows - 1)) * gap)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let count = columns(bounds.width)
        for (index, view) in subviews.enumerated() {
            view.place(at: CGPoint(x: bounds.minX + CGFloat(index % count) * (tile.width + gap),
                y: bounds.minY + CGFloat(index / count) * (tile.height + gap)), anchor: .topLeading,
                proposal: ProposedViewSize(tile))
        }
    }
}
