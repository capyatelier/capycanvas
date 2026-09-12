import SwiftUI

/// Keep in-app menu invalidation inside the header's menu view. Reading the
/// complete menu array in EditorView would rebuild the workspace on pen down/up.
struct ApplicationMenus: View {
    @ObservedObject var store: EditorStore
    let palette: EditorPalette
    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 6) {
                ForEach(store.snapshot["application_menus"].array.indices, id: \.self) { index in
                    let menu = store.snapshot["application_menus"][index]
                    Menu { CatalogMenuItems(store: store, id: menu["id"].string) } label: {
                        Text(menu["label"].string).fontWeight(.bold).padding(.horizontal, 17).frame(height: 36)
                            .background(palette["bg"], in: RoundedRectangle(cornerRadius: 6))
                    }.buttonStyle(.plain).accessibilityIdentifier("menu-" + menu["label"].string)
                }
            }.fixedSize()
            Menu {
                ForEach(store.snapshot["application_menus"].array.indices, id: \.self) { index in
                    let menu = store.snapshot["application_menus"][index]
                    Menu(menu["label"].string) { CatalogMenuItems(store: store, id: menu["id"].string) }
                }
            } label: {
                SharedIcon(name: "menu").frame(width: 36, height: 36)
                    .background(palette["bg"], in: RoundedRectangle(cornerRadius: 6))
            }.buttonStyle(.plain).accessibilityLabel("Menus").accessibilityIdentifier("application-menus")
        }
    }
}

/// The in-app iPad menus and the Mac OS menu bar consume the same catalog/state.
struct CatalogMenuItems: View {
    @ObservedObject var store: EditorStore
    let id: String
    var excluding: Set<String> = []
    var body: some View {
        let model = store.applicationMenu(id)
        let sections = JSON(model["model"]["sections"].array.map { section in
            section.array.filter { !excluding.contains($0["action"]["command"].string) }.map(\.raw)
        }.filter { !$0.isEmpty })
        MenuItems(store: store, sections: sections, usesShortcuts: true)
            .disabled(!store.snapshot["preferences"].isNull)
        if id == "file" {
            Divider()
            Button("Recovered Drawings…") { store.recovery.refresh(); store.recovery.presented = true }
                .disabled(!store.snapshot["preferences"].isNull || store.projectFiles.busy)
        }
    }
}

func menuShortcut(_ binding: JSON) -> KeyboardShortcut? {
    let text = binding["key"].string
    let named: [String: KeyEquivalent] = ["tab": .tab, "enter": .return, "escape": .escape,
        "delete": .deleteForward, "backspace": .delete, "arrowleft": .leftArrow,
        "arrowright": .rightArrow, "arrowup": .upArrow, "arrowdown": .downArrow,
        "home": .home, "end": .end, "pageup": .pageUp, "pagedown": .pageDown]
    let function = text.first == "f" ? Int(text.dropFirst()).flatMap { number -> KeyEquivalent? in
        guard (1...24).contains(number), let scalar = UnicodeScalar(0xF703 + number) else { return nil }
        return KeyEquivalent(Character(scalar))
    } : nil
    let key = named[text] ?? function ?? (text.count == 1 ? text.first.map { KeyEquivalent($0) } : nil)
    guard let key else { return nil }
    var flags: EventModifiers = []
    if binding["command"].bool { flags.insert(.command) }
    if binding["shift"].bool { flags.insert(.shift) }
    if binding["alt"].bool { flags.insert(.option) }
    return KeyboardShortcut(key, modifiers: flags)
}
