import SwiftUI

struct PanelConfiguration: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var fontSize: CGFloat { max(1, store.catalog["text_size_pt"].number * 4 / 3) }
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top) {
                    Text(panel["configuration_title"].string).fontWeight(.bold)
                    Spacer(minLength: 4)
                    Button { store.customize(["type": "close_expanded"]) } label: { Image(systemName: "xmark") }
                        .buttonStyle(.plain).accessibilityLabel("Close configuration").accessibilityIdentifier("close-panel-configuration")
                }
                Text(panel["configuration_hint"].string).opacity(0.55)
                    .frame(minHeight: fontSize * 1.4, alignment: .leading)
                ForEach(panel["controls"].array.indices, id: \.self) { index in
                    let item = panel["controls"][index]
                    VStack(alignment: .leading, spacing: 6) {
                        Button { store.customize(["type": "set_control_visible", "panel": panel["id"].raw,
                            "control": item["control"].raw, "visible": !item["visible_in_panel"].bool]) } label: {
                            HStack(spacing: 4) {
                                RoundedRectangle(cornerRadius: 4)
                                    .fill(item["visible_in_panel"].bool ? palette.accent : Color.clear)
                                    .overlay {
                                        if item["visible_in_panel"].bool { SharedIcon(name: "check").foregroundStyle(.white) }
                                        else { RoundedRectangle(cornerRadius: 4).strokeBorder(palette["text"].opacity(0.25), lineWidth: 2) }
                                    }.frame(width: 16, height: 16)
                                Text(item["label"].string)
                            }.frame(maxWidth: .infinity, alignment: .leading).frame(minHeight: 24).contentShape(Rectangle())
                        }.buttonStyle(.plain).accessibilityValue(item["visible_in_panel"].bool ? "On" : "Off")
                            .accessibilityIdentifier("configure-visible-" + item["control"].string)
                        configuredControl(item)
                    }
                }
                if !panel["toolbar_options"].array.isEmpty {
                    WorkspaceMenu(store: store, menu: JSON(["sections": panel["toolbar_options"].raw]), width: nil)
                        .frame(height: 420)
                }
            }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
                .background(GeometryReader { allocation in
                    Color.clear.preference(key: ConfigurationHeight.self, value: allocation.size.height)
                })
        }.onPreferenceChange(ConfigurationHeight.self) { store.workspace.measureConfiguration($0) }
            .accessibilityIdentifier("panel-configuration")
    }
    @ViewBuilder private func configuredControl(_ item: JSON) -> some View {
        let control = item["control"].string
        switch control {
        case "size_presets": configurationSizes
        case "brush_color":
            Button { store.customize(["type": "open_control", "control": "brush_color"]) } label: {
                ColorSwatch(rgba: store.state["brush"]["color"])
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                    .padding(.horizontal, 12).padding(.vertical, 4).frame(height: 34)
                    .background(palette["button"].opacity(13 / 255), in: RoundedRectangle(cornerRadius: 6))
                    .contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityLabel(item["label"].string)
                .accessibilityIdentifier("configuration-brush-color")
        case "brushes": ScrollView { ToolSetControls(store: store) }.frame(height: 250)
        case "color_wheel": ColorPanel(store: store)
        case "navigator": NavigatorPanel(store: store).frame(height: 240)
        case "adjustments": AdjustmentPanel(store: store).frame(height: 280)
        case "layers", "layer_opacity", "layer_actions":
            LayerPanel(store: store, panel: panel.replacing("controls", with: JSON([
                item.replacing("visible_in_panel", with: JSON(true)).raw
            ]))).frame(height: control == "layers" ? 240 : control == "layer_opacity" ? 64 : 32)
        default: PanelControls(store: store, panel: panel).control(item)
        }
    }
    private var configurationSizes: some View {
        ConfigurationFlow(spacing: 6) {
            ForEach(store.catalog["brush_sizes"].array.indices, id: \.self) { index in
                let value = store.catalog["brush_sizes"][index].number
                Button { store.dispatch(["type": "set_brush_size", "value": value]) } label: {
                    Text(String(Int(value))).fontWeight(.bold).padding(.horizontal, 12)
                        .frame(minWidth: 52).frame(height: fontSize * 1.66 + 8)
                        .background(palette["button"].opacity(13 / 255), in: RoundedRectangle(cornerRadius: 6))
                        .contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityLabel("\(Int(value)) px")
                    .accessibilityIdentifier("configuration-size-\(Int(value))")
            }
        }.padding(3)
    }
}

/// Intrinsic-width wrapping matches the web flex row and Android FlowRow.
/// Rust still supplies every preset/action and resolves the measured drawer.
private struct ConfigurationFlow: Layout {
    let spacing: CGFloat
    private func positions(_ subviews: Subviews, width: CGFloat) -> (CGSize, [CGPoint]) {
        var x: CGFloat = 0, y: CGFloat = 0, rowHeight: CGFloat = 0
        var points: [CGPoint] = []
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > width {
                x = 0; y += rowHeight + spacing; rowHeight = 0
            }
            points.append(CGPoint(x: x, y: y))
            x += size.width + spacing; rowHeight = max(rowHeight, size.height)
        }
        return (CGSize(width: width, height: y + rowHeight), points)
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let natural = subviews.reduce(CGFloat(0)) { $0 + $1.sizeThatFits(.unspecified).width + spacing }
        let requested = proposal.width ?? max(0, natural - spacing)
        let width = requested.isFinite ? max(0, requested) : max(0, natural - spacing)
        return positions(subviews, width: width).0
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (index, point) in positions(subviews, width: bounds.width).1.enumerated() {
            subviews[index].place(at: CGPoint(x: bounds.minX + point.x, y: bounds.minY + point.y),
                anchor: .topLeading, proposal: .unspecified)
        }
    }
}

private struct ConfigurationHeight: PreferenceKey {
    static var defaultValue: CGFloat { 0 }
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}
