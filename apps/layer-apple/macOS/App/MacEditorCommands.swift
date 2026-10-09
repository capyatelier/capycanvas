import AppKit
import SwiftUI

/// The canvas Select All, Cut, Copy and Paste shortcuts share their keys with
/// native text editing. Let AppKit route them to native controls first,
/// including remote file-panel fields.
@MainActor func nativeTextMenuAction(_ action: JSON) -> Bool {
    let selectors = ["select_all": #selector(NSText.selectAll(_:)), "cut": #selector(NSText.cut(_:)),
        "copy": #selector(NSText.copy(_:)), "paste_image": #selector(NSText.paste(_:))]
    guard action["type"].string == "invoke" else { return false }
    let command = action["command"].string
    let selector = selectors[command]
    guard selector != nil || ["copy_pixels", "copy_merged", "paste_as_new_image", "paste_in_place", "paste_at_view", "paste_at_cursor", "paste_into"].contains(command) else { return false }
    let window = NSApp.keyWindow
    let responder = window?.firstResponder
    let native = responder is NSText || responder is NSTextField
        || (responder as? NSTextInputClient)?.hasMarkedText() == true
        || window?.sheetParent != nil || window?.attachedSheet != nil || NSApp.modalWindow != nil
    return selector.map { NSApp.sendAction($0, to: nil, from: nil) } == true || native
}

private struct FocusedEditorKey: FocusedValueKey { typealias Value = EditorStore }
extension FocusedValues {
    var editorStore: EditorStore? {
        get { self[FocusedEditorKey.self] }
        set { self[FocusedEditorKey.self] = newValue }
    }
}

struct MacEditorCommands: Commands {
    @FocusedValue(\.editorStore) private var store
    @Environment(\.openWindow) private var openWindow
    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            if let store { CatalogMenuItems(store: store, id: "file") }
            else {
                // A windowless app must still open an editor. Keeping New
                // present also retains File before the first scene gains focus.
                Button("New Window") { openWindow(id: "editor") }.keyboardShortcut("n")
            }
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
            // AppKit supplies View's native Full Screen item. The shared command
            // remains available to customized controls and shortcut editing.
            if let store { CatalogMenuItems(store: store, id: "view", excluding: ["fullscreen"]) }
        }
        CommandGroup(after: .windowArrangement) {
            if let store {
                CatalogMenuItems(store: store, id: "window")
            }
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
