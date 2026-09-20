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
        let colors = store.state["colors"], panel = store.snapshot["color_panel"]
        let slot = colors["paint_slot"].string == "background" ? "background" : "foreground"
        selection = Selection(epoch: store.state["document_file"]["epoch"].uint,
            slot: slot, color: colors[slot], space: colors["rgb_space"].string,
            intensity: panel["hdr"].bool ? panel["intensity"].number : nil)
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
            if compact { Image(systemName: "square.and.pencil").resizable().scaledToFit().padding(3).frame(maxWidth: .infinity, maxHeight: .infinity) }
            else { Text("Edit Color…") }
        }.buttonStyle(.plain).accessibilityLabel("Edit Color").accessibilityIdentifier("paint-edit-color")
            .help("Edit Color…")
            .sheet(item: $selection) { selection in
                ColorEditor(value: selection.color, documentSpace: selection.space, intensity: selection.intensity,
                    viewing: store.colorViewing, hdrUse: { color, stops in use(color, intensity: stops, for: selection) }) { use($0, for: selection) }
                    .modifier(EditorPopupPresentation())
            }
    }
}
