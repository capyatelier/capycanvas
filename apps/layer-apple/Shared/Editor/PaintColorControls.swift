import SwiftUI

/// Color entry captures the document and paint slot. Cancel never publishes a draft.
struct PaintColorControls: View {
    @ObservedObject var store: EditorStore
    var compact = false
    @State private var selection: Selection?
    private struct Selection: Identifiable {
        let id = UUID()
        let epoch: UInt64
        let slot: String
        let colors: JSON
    }
    private func open() {
        let slot = store.snapshot["paint_pair"]["front_swatch"].string
        guard slot != "transparent" else { return }
        selection = Selection(epoch: store.state["document_file"]["epoch"].uint, slot: slot, colors: store.displayColors)
    }
    private func use(_ color: JSON, intensity: Double? = nil, for selection: Selection) {
        self.selection = nil
        guard store.state["document_file"]["epoch"].uint == selection.epoch else { return }
        var action: [String: Any] = ["op": intensity == nil ? "set_slot" : "set_slot_intensity", "slot": selection.slot, "color": color.raw]
        if let intensity { action["stops"] = intensity }
        store.edit(["type": "color", "action": action]) { if let error = $0 { store.failure = error } }
    }
    var body: some View {
        Button(action: open) {
            if compact { SharedIcon(name: "pencil").frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()) }
            else { Text(store.catalog["native_copy"]["color"]["edit_menu"].string) }
        }.buttonStyle(.plain).disabled(store.snapshot["paint_pair"]["front_swatch"].string == "transparent")
            .accessibilityLabel(store.catalog["native_copy"]["color"]["edit"].string).accessibilityIdentifier("paint-edit-color")
            .help(store.catalog["native_copy"]["color"]["edit_menu"].string)
            .sheet(item: $selection) { selection in
                ColorEditor(colors: selection.colors, slot: selection.slot, viewing: store.colorViewing) { color, stops in use(color, intensity: stops, for: selection) }
                    .modifier(EditorPopupPresentation())
            }
    }
}
