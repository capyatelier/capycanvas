import SwiftUI

private struct FocusedEditorKey: FocusedValueKey { typealias Value = EditorStore }
extension FocusedValues {
    var editorStore: EditorStore? {
        get { self[FocusedEditorKey.self] }
        set { self[FocusedEditorKey.self] = newValue }
    }
}

struct MacEditorCommands: Commands {
    @FocusedValue(\.editorStore) private var store
    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            if let store { CatalogMenuItems(store: store, id: "file") }
        }
        CommandGroup(replacing: .undoRedo) {
            if let store { CatalogMenuItems(store: store, id: "edit", excluding: ["settings"]) }
        }
        CommandMenu("Layer") { if let store { CatalogMenuItems(store: store, id: "layer") } }
        CommandMenu("Select") { if let store { CatalogMenuItems(store: store, id: "select") } }
        CommandMenu("Filter") { if let store { CatalogMenuItems(store: store, id: "filter") } }
        CommandGroup(replacing: .help) {
            if let store { CatalogMenuItems(store: store, id: "help", excluding: ["about"]) }
        }
        CommandGroup(replacing: .appInfo) {
            if let store { Button(store.command("about")["label"].string) { store.invoke("about") } }
        }
        CommandGroup(replacing: .appSettings) {
            if let store { SettingsMenuItem(store: store) }
        }
        CommandGroup(after: .toolbar) {
            if let store { CatalogMenuItems(store: store, id: "view") }
        }
        CommandGroup(after: .windowArrangement) {
            if let store { CatalogMenuItems(store: store, id: "window") }
        }
    }
}

private struct SettingsMenuItem: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        let command = store.command("settings")
        Button("Settings…") { store.invoke("settings") }
            .disabled(!command["enabled"].bool)
            .keyboardShortcut(menuShortcut(command["bindings"][0]))
    }
}
