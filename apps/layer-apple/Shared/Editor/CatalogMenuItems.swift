import SwiftUI

/// The in-app iPad menus and the Mac OS menu bar consume the same catalog/state.
struct CatalogMenuItems: View {
    @ObservedObject var store: EditorStore
    let label: String
    var body: some View {
        let menu = store.catalog["menus"].array.first { $0["label"].string == label } ?? JSON()
        if menu["sections"].array.isEmpty {
            MenuItems(store: store, sections: store.snapshot["workspace_menu"]["sections"])
        } else {
            ForEach(menu["sections"].array.indices, id: \.self) { section in
                if section > 0 { Divider() }
                ForEach(menu["sections"][section].array.indices, id: \.self) { item in
                    let command = store.command(menu["sections"][section][item].string)
                    Button {
                        store.invoke(command["id"].string)
                    } label: {
                        if command["selected"].bool { Label(command["label"].string, systemImage: "checkmark") }
                        else { Text(command["label"].string) }
                    }.disabled(!command["enabled"].bool)
                        .accessibilityIdentifier("command-" + command["id"].string)
                        .keyboardShortcut(menuShortcut(command["bindings"][0]))
                }
            }
        }
    }
}

func menuShortcut(_ binding: JSON) -> KeyboardShortcut? {
    let text = binding["key"].string
    let named: [String: KeyEquivalent] = ["tab": .tab, "enter": .return, "escape": .escape,
        "delete": .deleteForward, "backspace": .delete, "arrowleft": .leftArrow,
        "arrowright": .rightArrow, "arrowup": .upArrow, "arrowdown": .downArrow,
        "home": .home, "end": .end, "pageup": .pageUp, "pagedown": .pageDown]
    let key = named[text] ?? (text.count == 1 ? text.first.map { KeyEquivalent($0) } : nil)
    guard let key else { return nil }
    var flags: EventModifiers = []
    if binding["command"].bool { flags.insert(.command) }
    if binding["shift"].bool { flags.insert(.shift) }
    if binding["alt"].bool { flags.insert(.option) }
    return KeyboardShortcut(key, modifiers: flags)
}
