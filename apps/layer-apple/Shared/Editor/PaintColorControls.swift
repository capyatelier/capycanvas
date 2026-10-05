import SwiftUI

struct PaintColorControls: View {
    @ObservedObject var store: EditorStore
    var compact = false
    private func open() {
        let slot = store.snapshot["paint_pair"]["front_swatch"].string
        guard slot != "transparent" else { return }
        store.colorEditing.open(colors: store.displayColors, slot: slot, viewing: store.colorViewing) { color, intensity in
            var action: [String: Any] = ["op": intensity == nil ? "set_slot" : "set_slot_intensity", "slot": slot, "color": color.raw]
            if let intensity { action["stops"] = intensity }
            store.edit(["type": "color", "action": action]) { if let error = $0 { store.failure = error } }
        }
    }
    var body: some View {
        Button(action: open) {
            if compact { SharedIcon(name: "pencil").frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()) }
            else { Text(store.catalog["native_copy"]["color"]["edit_menu"].string) }
        }.buttonStyle(.plain).disabled(store.snapshot["paint_pair"]["front_swatch"].string == "transparent")
            .accessibilityLabel(store.catalog["native_copy"]["color"]["edit"].string).accessibilityIdentifier("paint-edit-color")
            .help(store.catalog["native_copy"]["color"]["edit_menu"].string)
    }
}
