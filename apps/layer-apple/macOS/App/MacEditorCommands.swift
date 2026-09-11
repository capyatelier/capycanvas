import SwiftUI

private struct FocusedEditorKey: FocusedValueKey { typealias Value = EditorStore }
extension FocusedValues {
    var editorStore: EditorStore? {
        get { self[FocusedEditorKey.self] }
        set { self[FocusedEditorKey.self] = newValue }
    }
}

struct MacEditorCommands: Commands {
    @Environment(\.openWindow) private var openWindow
    @FocusedValue(\.editorStore) private var store
    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            if let store { CatalogMenuItems(store: store, label: "File") }
            else { Button("New Window") { openWindow(id: "editor") }.keyboardShortcut("n") }
        }
        CommandGroup(replacing: .undoRedo) {
            if let store { CatalogMenuItems(store: store, label: "Edit") }
        }
        CommandGroup(replacing: .appSettings) {
            if let store { SettingsMenuItem(store: store) }
        }
        CommandGroup(after: .toolbar) {
            if let store { CatalogMenuItems(store: store, label: "View") }
        }
        CommandMenu("Workspace") {
            if let store { CatalogMenuItems(store: store, label: "Workspace") }
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
