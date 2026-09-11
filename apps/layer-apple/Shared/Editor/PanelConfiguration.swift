import SwiftUI

struct PanelConfiguration: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top) {
                    Text(panel["configuration_title"].string).fontWeight(.bold)
                    Spacer(minLength: 4)
                    Button { store.customize(["type": "close_expanded"]) } label: { Image(systemName: "xmark") }
                        .buttonStyle(.plain).accessibilityLabel("Close configuration").accessibilityIdentifier("close-panel-configuration")
                }
                Text(panel["configuration_hint"].string).foregroundStyle(.secondary)
                ForEach(panel["controls"].array.indices, id: \.self) { index in
                    let item = panel["controls"][index]
                    VStack(alignment: .leading, spacing: 6) {
                        Button { store.customize(["type": "set_control_visible", "panel": panel["id"].raw,
                            "control": item["control"].raw, "visible": !item["visible_in_panel"].bool]) } label: {
                            HStack(spacing: 6) {
                                Image(systemName: item["visible_in_panel"].bool ? "checkmark.square.fill" : "square")
                                Text(item["label"].string).fontWeight(.medium)
                            }.frame(maxWidth: .infinity, alignment: .leading).frame(minHeight: 28).contentShape(Rectangle())
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
}

private struct ConfigurationHeight: PreferenceKey {
    static var defaultValue: CGFloat { 0 }
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}
