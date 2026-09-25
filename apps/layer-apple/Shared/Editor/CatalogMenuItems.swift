import SwiftUI

/// Keep in-app menu invalidation inside the header's menu view. Reading the
/// complete menu array in EditorView would rebuild the workspace on pen down/up.
struct ApplicationMenus: View {
    @ObservedObject var store: EditorStore
    var iconSize: CGFloat = 16
    var tileSize: CGFloat = 36
    var radius: CGFloat = 18
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var textSize: Double {
        store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3
    }
    static func naturalWidth(_ menus: [JSON], textSize: Double) -> CGFloat {
        menus.reduce(0) { $0 + EditorTextMetrics.width($1["label"].string, size: textSize, weight: .medium) + 16 }
            + 8 + 2 * CGFloat(max(0, menus.count - 1))
    }
    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 2) {
                ForEach(store.snapshot["application_menus"].array.indices, id: \.self) { index in
                    let menu = store.snapshot["application_menus"][index]
                    ApplicationMenuButton(store: store, id: menu["id"].string) {
                        Text(menu["label"].string).font(EditorTextMetrics.font(size: textSize, weight: .medium)).fixedSize()
                            .frame(width: EditorTextMetrics.width(menu["label"].string, size: textSize, weight: .medium))
                            .padding(.horizontal, 8).frame(height: 26)
                    }.buttonStyle(MenuLabelStyle(palette: palette))
                        .accessibilityIdentifier("menu-" + menu["label"].string)
                        .modifier(HeaderControlMeasurement(id: "menu-" + menu["label"].string))
                }
            }.padding(4).frame(height: 34).glassSurface(SquircleShape.tile, fill: palette.chromeSurface).fixedSize()
                .modifier(HeaderControlMeasurement(id: "header-menu-labels"))
            ApplicationMenuButton(store: store) {
                SharedIcon(name: "menu", size: iconSize).frame(width: tileSize, height: tileSize)
            }.buttonStyle(HeaderButtonStyle(radius: radius))
                .accessibilityLabel("Menus").accessibilityIdentifier("application-menus")
                .modifier(HeaderControlMeasurement(id: "application-menus"))
        }
    }
}

private struct MenuLabelStyle: ButtonStyle {
    let palette: EditorPalette
    func makeBody(configuration: Configuration) -> some View { Face(configuration: configuration, palette: palette) }
    private struct Face: View {
        let configuration: Configuration
        let palette: EditorPalette
        @State private var hovering = false
        var body: some View {
            configuration.label.background {
                if configuration.isPressed || hovering {
                    SquircleShape.tile.fill(palette["text"].opacity(configuration.isPressed ? 0.16 : 0.10))
                }
            }.onHover { hovering = $0 }
        }
    }
}

struct ApplicationMenuButton<Label: View>: View {
    @ObservedObject var store: EditorStore
    var id: String? = nil
    @ViewBuilder let label: () -> Label
    var body: some View {
        #if os(iOS)
        EditorMenuButton(menu: {
            AppleContextMenu(model) { action in
                if action["type"].string == "apple_recovery" { store.recovery.refresh(); store.recovery.presented = true }
                else { store.dispatch(action) }
            }
        }, identifier: "application-menu-content", label: label)
            .disabled(!store.snapshot["preferences"].isNull)
        #else
        Menu {
            if let id { CatalogMenuItems(store: store, id: id) }
            else {
                ForEach(store.snapshot["application_menus"].array.indices, id: \.self) { index in
                    let menu = store.snapshot["application_menus"][index]
                    Menu(menu["label"].string) { CatalogMenuItems(store: store, id: menu["id"].string) }
                }
            }
        } label: { label() }
        #endif
    }
    private var model: JSON { editorApplicationMenu(store, id: id) }
}

@MainActor func editorApplicationMenu(_ store: EditorStore, id: String? = nil) -> JSON {
    func catalog(_ id: String) -> JSON {
        var value = store.applicationMenu(id)["model"]
        if id == "file" {
            value = value.replacing("sections", with: JSON(value["sections"].array.map(\.raw) + [[[
                "label": "Recovered Drawings…", "enabled": store.snapshot["preferences"].isNull && !store.projectFiles.busy,
                "action": ["type": "apple_recovery"]
            ]]]))
        }
        return value
    }
    if let id { return catalog(id) }
    let primary = store.snapshot["header"]["primary_menu"]
    let fileLabel = store.snapshot["application_menus"].array.first { $0["id"].string == "file" }?["label"].string
    return primary.replacing("sections", with: JSON(primary["sections"].array.map { section in
        section.array.map { row in
            (row["label"].string == fileLabel ? row.replacing("sections", with: catalog("file")["sections"]) : row).raw
        }
    }))
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
