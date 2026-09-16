import SwiftUI

/// One entry point shared by the Color panel, brush controls and toolbar popup.
struct PaintColorControls: View {
    @ObservedObject var store: EditorStore
    @State private var selection: Selection?
    private struct Selection: Identifiable {
        let id = UUID()
        let epoch: UInt64
        let slot: String
        let color: JSON
        let space: String
        let palettes: Bool
    }
    private func open(palettes: Bool) {
        let colors = store.state["colors"]
        let slot = colors["slot"].string == "background" ? "background" : "foreground"
        selection = Selection(epoch: store.state["document_file"]["epoch"].uint,
            slot: slot, color: colors[slot], space: colors["rgb_space"].string, palettes: palettes)
    }
    private func use(_ color: JSON, for selection: Selection) {
        self.selection = nil
        guard store.state["document_file"]["epoch"].uint == selection.epoch else { return }
        store.edit(["type": "color", "action": ["op": "set_slot", "slot": selection.slot, "color": color.raw]]) {
            if let error = $0 { store.failure = error }
        }
    }
    @ViewBuilder private var actions: some View {
        Button("Edit Color…") { open(palettes: false) }.accessibilityIdentifier("paint-edit-color")
        Button("Palettes…") { open(palettes: true) }.accessibilityIdentifier("paint-palettes")
    }
    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack { actions }
            VStack(alignment: .leading) { actions }
        }.buttonStyle(.bordered).frame(maxWidth: .infinity, alignment: .leading)
            .sheet(item: $selection) { selection in
                Group {
                    if selection.palettes {
                        ColorLibraryView(store: store, slot: selection.slot) { use($0, for: selection) }
                    } else {
                        ColorEditor(value: selection.color, documentSpace: selection.space) { use($0, for: selection) }
                    }
                }.modifier(EditorPopupPresentation())
            }
    }
}
