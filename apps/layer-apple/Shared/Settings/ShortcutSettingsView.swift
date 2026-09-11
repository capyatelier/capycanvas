import SwiftUI

/// Rust supplies searchable actions, bindings, limits and conflict resolution.
struct ShortcutSettingsView: View {
    @ObservedObject var store: EditorStore
    private var model: JSON { store.snapshot["preferences"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        VStack(spacing: 12) {
            TextField("Search shortcuts", text: Binding(get: { model["shortcut_query"].string },
                set: { action(["type": "search_shortcuts", "query": $0]) }))
                .textFieldStyle(.roundedBorder).accessibilityIdentifier("shortcut-search")
            List {
                ForEach(model["shortcuts"].array.filter { $0["visible"].bool }, id: \.shortcutID) { row in
                    Button { action(["type": "edit_shortcut", "id": row["id"].string]) } label: {
                        HStack {
                            VStack(alignment: .leading) {
                                Text(row["label"].string)
                                Text(row["group"].string).font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Text(row["shortcut"].string).foregroundStyle(.secondary)
                        }.contentShape(Rectangle())
                    }.buttonStyle(.plain).accessibilityIdentifier("shortcut-" + row["id"].string)
                }
            }
            HStack {
                if model["shortcuts"].array.allSatisfy({ !$0["visible"].bool }) { Text("No matching shortcuts").foregroundStyle(.secondary) }
                Spacer()
                Button("Reset All Shortcuts") { action(["type": "reset_all_shortcuts"]) }
            }
        }.padding()
            .sheet(isPresented: Binding(get: { !model["shortcut_editor"].isNull },
                set: { if !$0 { action(["type": "close_shortcut_editor"]) } })) {
                ShortcutEditorForm(store: store)
            }
    }
}

private struct ShortcutEditorForm: View {
    @ObservedObject var store: EditorStore
    private var model: JSON { store.snapshot["preferences"] }
    private var editor: JSON { model["shortcut_editor"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(editor["label"].string).font(.headline)
            Text(editor["group"].string).foregroundStyle(.secondary)
            ForEach(editor["bindings"].array.indices, id: \.self) { index in
                HStack {
                    Text(editor["bindings"][index].string)
                    Spacer()
                    Button("Remove") { action(["type": "remove_shortcut", "id": editor["id"].string, "index": index]) }
                        .accessibilityIdentifier("shortcut-remove-\(index)")
                }
            }
            if editor["bindings"].array.isEmpty { Text("Disabled").foregroundStyle(.secondary) }
            Text("Default: " + (editor["defaults"].array.isEmpty ? "Disabled" : editor["defaults"].array.map(\.string).joined(separator: " / "))).font(.caption)
            if !model["error"].isNull { Text(model["error"].string).foregroundStyle(.red) }
            HStack {
                Button("Add Shortcut") { action(["type": "begin_shortcut", "id": editor["id"].string]) }
                    .disabled(!editor["can_add"].bool).accessibilityIdentifier("shortcut-add")
                Button("Reset to Default") { action(["type": "reset_shortcut", "id": editor["id"].string]) }
                    .disabled(!editor["modified"].bool).accessibilityIdentifier("shortcut-reset")
                Spacer()
                Button("Done") { action(["type": "close_shortcut_editor"]) }
                    .accessibilityIdentifier("shortcut-editor-done")
            }
        }.padding(24).frame(minWidth: 420, minHeight: 220)
            .sheet(isPresented: Binding(get: { !model["capture"].isNull },
                set: { if !$0 { action(["type": "cancel_shortcut"]) } })) {
                ShortcutCaptureForm(store: store)
            }
    }
}

private struct ShortcutCaptureForm: View {
    @ObservedObject var store: EditorStore
    private var capture: JSON { store.snapshot["preferences"]["capture"] }
    private func action(_ value: [String: Any]) { store.dispatch(["type": "preferences", "action": value]) }
    var body: some View {
        VStack(spacing: 20) {
            Text(capture["label"].string).font(.headline)
            Text(capture["shortcut"].string).font(.title2).accessibilityIdentifier("shortcut-captured")
            if !capture["notice"].string.isEmpty { Text(capture["notice"].string).foregroundStyle(capture["error"].isNull ? Color.secondary : Color.red) }
            HStack {
                Button("Cancel") { action(["type": "cancel_shortcut"]) }.accessibilityIdentifier("shortcut-cancel")
                Button(capture["conflict"].isNull ? "Add" : "Replace") {
                    action(["type": "confirm_shortcut", "replace": !capture["conflict"].isNull])
                }.disabled(capture["chord"].isNull || !capture["error"].isNull)
                    .accessibilityIdentifier("shortcut-confirm")
            }
        }.padding(24).frame(minWidth: 360, minHeight: 200)
            .background(ShortcutKeyCapture { key, command, shift, alt in
                store.captureShortcut(key: key, command: command, shift: shift, alt: alt)
            }.frame(width: 1, height: 1))
    }
}
private extension JSON { var shortcutID: String { self["id"].string } }
