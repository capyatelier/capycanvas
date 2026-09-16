import SwiftUI

/// Names, IDs, gamut conversion and durable palette contents stay in shared Rust.
struct ColorLibraryView: View {
    @ObservedObject var store: EditorStore
    let slot: String
    let use: (JSON) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var selected: UInt64
    @State private var name = ""
    @State private var error: String?
    @State private var busy = false
    @State private var removing = false
    init(store: EditorStore, slot: String, use: @escaping (JSON) -> Void) {
        self.store = store; self.slot = slot; self.use = use
        _selected = State(initialValue: store.state["colors"]["library"]["palettes"][0]["id"].uint)
    }
    private var palettes: [JSON] { store.state["colors"]["library"]["palettes"].array }
    private var palette: JSON { palettes.first { $0["id"].uint == selected } ?? palettes.first ?? JSON() }
    private func apply(_ action: [String: Any]) {
        guard !busy else { return }
        busy = true
        store.edit(["type": "color", "action": ["op": "library", "action": action]]) {
            busy = false; error = $0
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Palettes").font(.headline)
            Picker("Palette", selection: $selected) {
                ForEach(palettes, id: \.paletteID) { Text($0["name"].string).tag($0["id"].uint) }
            }.accessibilityIdentifier("color-library-palette")
            TextField("Palette or swatch name", text: $name).textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("color-library-name")
            HStack {
                Button("New") { apply(["op": "create_palette", "name": name]) }.accessibilityIdentifier("color-library-new")
                Button("Rename") { apply(["op": "rename_palette", "id": selected, "name": name]) }
                    .accessibilityIdentifier("color-library-rename")
                Button("Remove", role: .destructive) { removing = true }.disabled(palettes.count <= 1)
                    .accessibilityIdentifier("color-library-remove")
            }
            Button("Save Current Color") {
                apply(["op": "store", "palette": selected, "name": name, "color": store.state["colors"][slot].raw])
            }.accessibilityIdentifier("color-library-store")
            if let error { Text(error).foregroundStyle(.red).accessibilityIdentifier("color-library-error") }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if palette["swatches"].array.isEmpty { Text("No saved colors yet.").foregroundStyle(.secondary) }
                    ForEach(palette["swatches"].array, id: \.paletteID) { swatch in
                        SavedColorRow(swatch: swatch, use: use, change: apply)
                    }
                }
            }
            HStack { Spacer(); Button("Close") { dismiss() }.keyboardShortcut(.cancelAction) }
        }.disabled(busy).padding(24).frame(minWidth: 320, idealWidth: 420, maxWidth: 520, minHeight: 450, idealHeight: 580)
            .onChange(of: palettes.map(\.paletteID)) { previous, current in
                if let added = current.first(where: { !previous.contains($0) }) { selected = added }
                else if !current.contains(selected) { selected = current.first ?? 0 }
            }
            .alert("Remove palette?", isPresented: $removing) {
                Button("Remove", role: .destructive) { apply(["op": "remove_palette", "id": selected]) }
                Button("Cancel", role: .cancel) {}
            } message: { Text("This removes the palette and its saved colors.") }
    }
}

private struct SavedColorRow: View {
    let swatch: JSON
    let use: (JSON) -> Void
    let change: ([String: Any]) -> Void
    @State private var name: String
    init(swatch: JSON, use: @escaping (JSON) -> Void, change: @escaping ([String: Any]) -> Void) {
        self.swatch = swatch; self.use = use; self.change = change
        _name = State(initialValue: swatch["name"].string)
    }
    var body: some View {
        let preview = ColorUI.preview(swatch["color"])
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Button { use(swatch["color"]) } label: { ColorSwatch(rgba: preview["rgba"]).frame(width: 56, height: 40) }
                    .buttonStyle(.plain).accessibilityLabel("Use \(swatch["name"].string)")
                    .accessibilityIdentifier("color-swatch-\(swatch.paletteID)")
                TextField("Swatch name", text: $name).textFieldStyle(.roundedBorder)
                    .accessibilityIdentifier("color-swatch-name-\(swatch.paletteID)")
            }
            if !preview["in_gamut"].bool { Text("Outside sRGB preview gamut").font(.caption) }
            HStack {
                Button("Rename") { change(["op": "rename", "id": swatch.paletteID, "name": name]) }
                    .accessibilityIdentifier("color-swatch-rename-\(swatch.paletteID)")
                Button("Remove", role: .destructive) { change(["op": "remove", "id": swatch.paletteID]) }
                    .accessibilityIdentifier("color-swatch-remove-\(swatch.paletteID)")
            }
        }.onChange(of: swatch["name"].string) { _, next in name = next }
    }
}
private extension JSON { var paletteID: UInt64 { self["id"].uint } }
