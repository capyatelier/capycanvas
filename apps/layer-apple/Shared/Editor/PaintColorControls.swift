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
        let color: JSON
        let space: String
        let intensity: Double?
    }
    private func open() {
        let pair = store.snapshot["paint_pair"], panel = store.snapshot["color_panel"]
        selection = Selection(epoch: store.state["document_file"]["epoch"].uint,
            slot: pair["front_swatch"].string, color: pair["definition"], space: store.displayColors["rgb_space"].string,
            intensity: panel["hdr"].bool ? panel["intensity"].number : nil)
    }
    private func use(_ color: JSON, intensity: Double? = nil, for selection: Selection) {
        self.selection = nil
        guard store.state["document_file"]["epoch"].uint == selection.epoch else { return }
        var action: [String: Any] = ["op": "definition", "color": color.raw]
        if let intensity { action = ["op": "set_slot_intensity", "slot": selection.slot, "color": color.raw, "stops": intensity] }
        store.edit(["type": "color", "action": action]) { if let error = $0 { store.failure = error } }
    }
    var body: some View {
        Button(action: open) {
            if compact { SharedIcon(name: "pencil").frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()) }
            else { Text(store.catalog["native_copy"]["color"]["edit_menu"].string) }
        }.buttonStyle(.plain)
            .accessibilityLabel(store.catalog["native_copy"]["color"]["edit"].string).accessibilityIdentifier("paint-edit-color")
            .help(store.catalog["native_copy"]["color"]["edit_menu"].string)
            .sheet(item: $selection) { selection in
                ColorEditor(value: selection.color, documentSpace: selection.space, intensity: selection.intensity,
                    viewing: store.colorViewing, hdrUse: { color, stops in use(color, intensity: stops, for: selection) }) { use($0, for: selection) }
                    .modifier(EditorPopupPresentation())
            }
    }
}
