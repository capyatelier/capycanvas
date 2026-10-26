import SwiftUI

struct PaintColorControls: View {
    @ObservedObject var store: EditorStore
    var compact = false
    var body: some View {
        Button { store.editPaintColor(store.snapshot["paint_pair"]["front_swatch"].string) } label: {
            if compact { SharedIcon(name: "pencil").frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()) }
            else { Text(store.catalog["native_copy"]["color"]["edit_menu"].string) }
        }.buttonStyle(.plain).disabled(store.snapshot["paint_pair"]["front_swatch"].string == "transparent")
            .accessibilityLabel(store.catalog["native_copy"]["color"]["edit"].string).accessibilityIdentifier("paint-edit-color")
            .help(store.catalog["native_copy"]["color"]["edit_menu"].string)
    }
}
