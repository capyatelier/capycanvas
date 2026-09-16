import SwiftUI

@MainActor struct EditorHeader: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var header: HeaderPresentation
    @ObservedObject var status: SystemStatus
    @Environment(\.scenePhase) private var phase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var statusSubscription: UUID?
    @State private var editorHeight: CGFloat = 0
    @FocusState private var focused: Bool
    init(store: EditorStore, status: SystemStatus? = nil) {
        self.store = store; self.header = store.header; self.status = status ?? .shared
    }
    private var view: JSON { store.snapshot["header"] }
    private var model: JSON { view["model"] }
    private var editing: Bool { view["editing"].bool }
    private var entries: [JSON] { model["zones"].array.flatMap(\.array) }
    private var size: JSON { view["sizes"].array.first { $0["id"].string == model["size"].string } ?? JSON() }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var light: Bool { store.state["theme"].string == "light" }
    private var geometry: JSON { header.preview.isNull ? header.geometry : header.preview["geometry"] }
    private var recoveryMenu: Bool {
        !editing && store.state["platform"].string != "mac"
            && !entries.contains { ["capy", "menu", "menu_labels", "workspaces"].contains($0["item"]["kind"].string) }
    }
    private var needsStatus: Bool {
        (editing || store.state["fullscreen"].bool) && entries.contains { ["clock", "battery"].contains($0["item"]["kind"].string) }
    }
    private var metrics: [Any] {
        entries.map { entry in
            let kind = entry["item"]["kind"].string, tile = size["tile"].number
            let hidden = !editing && (["clock", "battery"].contains(kind) && !store.state["fullscreen"].bool
                || kind == "battery" && status.battery == nil)
            var width = tile, compact = tile
            switch kind {
            case "document_title": width = 180; compact = 80
            case "workspaces":
                width = max(144, WorkspaceSwitcher.naturalWidth(store.workspaceLibrary?.status["switcher_display"].array ?? [], textSize: 44.0 / 3))
                compact = 144
            case "menu_labels":
                width = ceil(store.snapshot["application_menus"].array.reduce(0) { $0 + EditorTextMetrics.width($1["label"].string, size: 44.0 / 3, weight: .bold) + 16 })
                compact = tile
            case "clock":
                let text = editing && !store.state["fullscreen"].bool ? "Clock" : status.time
                width = max(tile, EditorTextMetrics.width(text, size: 44.0 / 3, weight: .regular, monospacedDigits: true) + 12)
                compact = width
            default: break
            }
            return ["id":entry["id"].raw, "width":hidden ? 0 : width + (editing ? 20 : 0),
                "compact":hidden ? 0 : compact + (editing ? 20 : 0)]
        }
    }
    private var specification: JSON {
        JSON(["model":model.raw, "editing":editing, "height":size["height"].number + (editing ? editorHeight : 0),
            "request":["op":"geometry", "width":store.snapshot["layout"]["viewport"][0].number,
                "insets":[store.headerLeadingInset + (recoveryMenu ? size["tile"].number + 6 : 0), 0], "metrics":metrics]])
    }
    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.clear.frame(height: size["height"].number).contentShape(Rectangle())
                .onTapGesture { header.selected = nil; focused = editing }
                .modifier(HeaderCaption(enabled: !editing))
                .modifier(WorkspaceContext(store: store, target: JSON(["kind":"header", "id":NSNull()])))
            if editing {
                ForEach(0..<3, id: \.self) { zone in
                    RoundedRectangle(cornerRadius: 6).stroke(palette["text"].opacity(0.2), style: StrokeStyle(lineWidth: 1, dash: [3,3]))
                        .placed(geometry["zones"][zone]).allowsHitTesting(false)
                }
            }
            ForEach(entries, id: \.headerID) { entry in slot(entry) }
            if recoveryMenu {
                ApplicationMenuButton(store: store) { SharedIcon(name: "menu", size: size["icon"].number)
                    .frame(width: size["tile"].number, height: size["tile"].number) }
                    .buttonStyle(EditorControlButtonStyle(background: palette.headerBackground(light: light), keepsBackground: light))
                    .accessibilityLabel("Main Menu").accessibilityIdentifier("header-recovery-menu")
                    .offset(x: store.headerLeadingInset + 6, y: 6)
            }
            ForEach(0..<3, id: \.self) { zone in
                if !geometry["overflow"][zone].isNull {
                    Group {
                        if editing {
                            Button { header.showOverflow(zone) } label: { overflowIcon }
                                .editorPopover(isPresented: Binding(get: { header.overflowZone == zone },
                                    set: { if !$0 { header.closeOverflow() } })) { overflowEditor(zone) }
                        } else {
                            EditorMenuButton(menu: { overflow(zone) }, identifier: "header-overflow-menu") { overflowIcon }
                        }
                    }.buttonStyle(EditorControlButtonStyle(background: palette.headerBackground(light: light), keepsBackground: light))
                        .accessibilityLabel("More title bar items")
                        .accessibilityIdentifier("header-overflow-\(zone)").placed(geometry["overflow"][zone])
                }
            }
            if editing {
                HeaderEditorBank(store: store, header: header, view: view)
                    .background(GeometryReader { proxy in Color.clear.preference(key: HeaderEditorHeight.self, value: proxy.size.height) })
                    .offset(y: size["height"].number)
            }
            if !header.preview.isNull {
                let source = header.heldSource
                let entry = source["kind"].string == "item"
                    ? entries.first(where: { $0["id"].uint == source["value"].uint }) ?? JSON()
                    : JSON(["id":0, "item":source["kind"].string == "tools" ? ["kind":"tools"] : source["value"].raw])
                item(entry, width: header.preview["held"]["width"].number)
                    .background(palette["panel"], in: RoundedRectangle(cornerRadius: 6))
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(header.preview["detached"].bool ? .red : palette.accent, lineWidth: 2))
                    .placed(header.preview["held"]).allowsHitTesting(false).accessibilityHidden(true)
            }
            Color.clear.frame(width: header.menuAnchor.width, height: header.menuAnchor.height)
                .editorPopover(isPresented: Binding(get: { !header.menu.isNull }, set: { if !$0 { header.closeMenu() } }), placement: .inward) {
                    WorkspaceMenu(store: store, menu: header.menu) { header.closeMenu() }
                }.offset(x: header.menuAnchor.minX, y: header.menuAnchor.minY).allowsHitTesting(false)
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(NativeReorderInput(model: header))
            .onPreferenceChange(HeaderSources.self) { header.sources = $0 }
            .onPreferenceChange(HeaderEditorHeight.self) { editorHeight = ($0 * 64).rounded() / 64 }
            .onChange(of: specification.stableKey, initial: true) { _, _ in header.reconcile(specification) }
            .focusable(editing).focused($focused)
            .onKeyPress(.leftArrow) { step(false) }
            .onKeyPress(.rightArrow) { step(true) }
            .onKeyPress(keys: [.delete, .deleteForward]) { _ in
                guard header.enabled, header.selected != nil else { return .ignored }
                header.removeSelected(); return .handled
            }
            .onKeyPress(.escape) {
                guard header.enabled else { return .ignored }
                if header.contact.dragging { header.cancel() }
                else if header.overflowZone != nil { header.closeOverflow() }
                else if !header.menu.isNull { header.closeMenu() }
                else { header.action(["type":"cancel"]) }
                return .handled
            }
            .onChange(of: needsStatus, initial: true) { _, _ in subscribe() }
            .onChange(of: phase) { _, _ in subscribe() }
            .onDisappear { header.cancel(); unsubscribe() }
    }
    private func step(_ forward: Bool) -> KeyPress.Result {
        guard header.enabled, header.selected != nil else { return .ignored }
        header.step(forward: forward); return .handled
    }
    private func subscribe() {
        if !needsStatus || phase == .background { unsubscribe() }
        else if statusSubscription == nil { statusSubscription = status.acquire() }
    }
    private func unsubscribe() { if let statusSubscription { status.release(statusSubscription) }; statusSubscription = nil }
    @ViewBuilder private func slot(_ entry: JSON) -> some View {
        let id = entry["id"].uint
        let held = header.heldSource["kind"].string == "item" && header.heldSource["value"].uint == id
        let allocation = geometry["items"].array.first { $0["id"].uint == id }
            ?? (held ? header.geometry["items"].array.first { $0["id"].uint == id } : nil)
        if let allocation {
            Group {
                if editing {
                    Button {
                        guard !header.contact.consumeClick() else { return }
                        header.selected = id; focused = true
                    } label: {
                        item(entry, width: allocation["bounds"]["width"].number).allowsHitTesting(false)
                            .overlay(Color.clear.contentShape(Rectangle()))
                    }
                        .buttonStyle(.plain).accessibilityLabel(metadata(entry)["label"].string)
                        .accessibilityIdentifier("header-item-\(id)")
                        .accessibilityAddTraits(header.selected == id ? .isSelected : [])
                        .modifier(HeaderSourceMeasurement(source: JSON(["kind":"item", "value":id])))
                        .editorContextAction { header.showMenu(HeaderSource(source: JSON(["kind":"item", "value":id]).stableKey, bounds: allocation["bounds"].rect)) }
                } else {
                    item(entry, width: allocation["bounds"]["width"].number)
                        .modifier(WorkspaceContext(store: store, target: JSON(["kind":"header", "id":id])))
                }
            }.modifier(HeaderControlMeasurement(id: "header-item-\(id)"))
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(editing && header.selected == id ? palette.accent : .clear, lineWidth: 2))
                .placed(allocation["bounds"]).opacity(held ? 0 : 1).allowsHitTesting(!held)
                .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: allocation["bounds"].rect)
        }
    }
    private func metadata(_ entry: JSON) -> JSON {
        view["items"].array.first { $0["id"].uint == entry["id"].uint }
            ?? view["components"].array.first { $0["item"].stableKey == entry["item"].stableKey } ?? JSON(["label":"Add Tools…"])
    }
    private var overflowIcon: some View {
        SharedIcon(name: "more", size: size["icon"].number)
            .frame(width: size["tile"].number, height: size["tile"].number)
            .contentShape(Rectangle())
    }
    private func overflowEditor(_ zone: Int) -> some View {
        ScrollView {
            VStack(spacing: 2) {
                ForEach(geometry["hidden"][zone].array, id: \.uint) { id in
                    if let entry = entries.first(where: { $0["id"].uint == id.uint }) {
                        Button {
                            guard !header.contact.consumeClick() else { return }
                            header.selected = id.uint; header.closeOverflow(); focused = true
                        } label: {
                            HStack(spacing: 6) {
                                SharedIcon(name: "grip", size: 12).opacity(0.5)
                                Text(metadata(entry)["label"].string)
                                Spacer(minLength: 0)
                            }.padding(.horizontal, 8).frame(height: 36).contentShape(Rectangle())
                        }.buttonStyle(.plain).accessibilityIdentifier("header-overflow-item-\(id.uint)")
                            .modifier(HeaderSourceMeasurement(source: JSON(["kind":"item", "value":id.raw])))
                    }
                }
            }.padding(6)
        }.frame(width: 240, height: min(360, CGFloat(geometry["hidden"][zone].array.count) * 38 + 12))
            .onPreferenceChange(HeaderSources.self) { header.overflowSources = $0 }
    }
    private func item(_ entry: JSON, width: CGFloat) -> some View {
        HStack(spacing: 0) {
            if editing { SharedIcon(name: "grip", size: 12).frame(width: 20).opacity(0.5) }
            HeaderItemControl(store: store, entry: entry, description: metadata(entry), size: size, status: status,
                width: max(0, width - (editing ? 20 : 0)), editing: editing)
        }.frame(width: width, height: size["tile"].number)
            .background(editing && !light ? palette["bg"] : .clear, in: RoundedRectangle(cornerRadius: 6))
    }
    private func overflow(_ zone: Int) -> AppleContextMenu {
        let rows = geometry["hidden"][zone].array.compactMap { id -> Any? in
            guard let entry = entries.first(where: { $0["id"].uint == id.uint }) else { return nil }
            var row: [String: Any] = ["label":metadata(entry)["label"].raw, "enabled":true,
                "action":["type":"apple_header_item", "id":id.raw]]
            if !editing {
                switch entry["item"]["kind"].string {
                case "menu", "menu_labels": row["sections"] = editorApplicationMenu(store)["sections"].raw
                case "workspaces":
                    if let library = store.workspaceLibrary { row["sections"] = WorkspaceSwitcher.menu(library)["sections"].raw }
                    else { row["enabled"] = false }
                case "tool": row["enabled"] = metadata(entry)["enabled"].bool
                case "document_title", "clock", "battery", "space": row["enabled"] = false
                default: break
                }
            }
            return row
        }
        return AppleContextMenu(JSON(["sections":[rows]])) { action in
            switch action["type"].string {
            case "apple_header_item":
                if editing { header.selected = action["id"].uint; focused = true }
                else { activateHeaderItem(store, entry: entries.first { $0["id"].uint == action["id"].uint } ?? JSON()) }
            case "apple_workspace_switch": store.workspaceManager.activate(JSON(["type":"switch", "value":action["id"].raw]))
            case "apple_recovery": store.recovery.refresh(); store.recovery.presented = true
            default: store.dispatch(action)
            }
        }
    }
}

private struct HeaderItemControl: View {
    @ObservedObject var store: EditorStore
    let entry: JSON, description: JSON, size: JSON
    @ObservedObject var status: SystemStatus
    let width: CGFloat
    let editing: Bool
    @State private var hovering = false
    private var kind: String { entry["item"]["kind"].string }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var light: Bool { store.state["theme"].string == "light" }
    private var drawerOpen: Bool {
        let anchor = store.state["customization"]["drawer"]["anchor"]
        return !editing && anchor["kind"].string == "header" && anchor["id"].uint == entry["id"].uint
    }
    var body: some View {
        Group {
            switch kind {
            case "capy":
                tile(store.command("zen_mode")["icon"].string) { store.invoke("zen_mode") }.accessibilityIdentifier("zen-button")
                    .modifier(HeaderControlMeasurement(id: "zen-button"))
            case "menu":
                ApplicationMenuButton(store: store) { SharedIcon(name: "menu", size: size["icon"].number).frame(maxWidth: .infinity, maxHeight: .infinity) }
                    .buttonStyle(EditorControlButtonStyle(active: hovering, background: palette.headerBackground(light: light), keepsBackground: light))
                    .accessibilityLabel("Main Menu").accessibilityIdentifier("application-menus")
                    .modifier(HeaderControlMeasurement(id: "application-menus"))
            case "menu_labels": ApplicationMenus(store: store, iconSize: size["icon"].number, tileSize: size["tile"].number)
            case "settings": tile("settings") { store.invoke("settings") }.accessibilityIdentifier("settings-button")
                    .modifier(HeaderControlMeasurement(id: "settings-button"))
            case "workspaces":
                if let library = store.workspaceLibrary {
                    WorkspaceSwitcher(library: library, manager: store.workspaceManager, palette: palette, maximumWidth: width)
                } else { Text("Workspaces").lineLimit(1) }
            case "document_title":
                let tab = store.state["tabs"][0]
                Text(verbatim: "\(tab["title"].string) · \(Int(tab["width"].number)) × \(Int(tab["height"].number))")
                    .fontWeight(.semibold).lineLimit(1).padding(.horizontal, 6).accessibilityIdentifier("document-title")
                    .modifier(HeaderControlMeasurement(id: "document-title"))
                    .modifier(HeaderCaption(enabled: !editing))
            case "clock":
                Text(editing && !store.state["fullscreen"].bool ? "Clock" : status.time).monospacedDigit().lineLimit(1)
                    .accessibilityIdentifier("system-clock").modifier(HeaderCaption(enabled: !editing))
                    .modifier(HeaderControlMeasurement(id: "system-clock"))
            case "battery":
                if let battery = status.battery, store.state["fullscreen"].bool {
                    BatteryIndicator(battery: battery, dark: store.state["theme"].string == "dark")
                        .modifier(HeaderCaption(enabled: !editing))
                } else { Text("Battery").lineLimit(1) }
            case "space": Color.clear.contentShape(Rectangle()).modifier(HeaderCaption(enabled: !editing))
            case "tool":
                if entry["item"]["control"]["kind"].string == "color" {
                    Button { store.dispatch(["type":"activate_header_item", "id":entry["id"].raw]) } label: {
                        HeaderPaintIcon(colors: store.state["colors"], size: size["icon"].number)
                            .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                    }.buttonStyle(EditorControlButtonStyle(active: hovering, joinedEdge: drawerOpen ? "bottom" : nil,
                        background: palette.headerBackground(light: light), keepsBackground: light, drawerBackground: drawerOpen ? palette["panel"] : nil))
                        .accessibilityLabel(description["label"].string)
                } else {
                    tile(description["icon"].string) { store.dispatch(["type":"activate_header_item", "id":entry["id"].raw]) }
                }
            default: SharedIcon(name: "toolbar", size: size["icon"].number)
            }
        }.frame(width: width, height: size["tile"].number)
            .background {
                if ["document_title", "clock", "battery"].contains(kind) {
                    RoundedRectangle(cornerRadius: 6).fill(palette.headerBackground(light: light))
                }
            }.onHover { hovering = $0 }
    }
    private func tile(_ icon: String, action: @escaping () -> Void) -> some View {
        IconTile(icon: icon, label: description["label"].string, selected: description["selected"].bool,
            enabled: editing || description["enabled"].bool,
            size: kind == "capy" ? size["tile"].number * 440 / 512 : size["icon"].number,
            active: hovering, joinedEdge: drawerOpen ? "bottom" : nil, background: palette.headerBackground(light: light),
            keepsBackground: light, drawerBackground: drawerOpen ? palette["panel"] : nil, action: action)
    }
}

/// The canonical 16-unit color-pair icon, with the workspace's live paints.
private struct HeaderPaintIcon: View {
    let colors: JSON
    let size: CGFloat
    var body: some View {
        ZStack(alignment: .topLeading) {
            swatch("background", edge: 8.75).offset(x: 6.5 * size / 16, y: 6.5 * size / 16)
            swatch("foreground", edge: 9.5).offset(x: 0.75 * size / 16, y: 0.75 * size / 16)
        }.frame(width: size, height: size).accessibilityHidden(true)
    }
    private func swatch(_ slot: String, edge: CGFloat) -> some View {
        let shape = RoundedRectangle(cornerRadius: size / 16)
        return ColorSwatch(rgba: colors[slot]).frame(width: edge * size / 16, height: edge * size / 16)
            .clipShape(shape).overlay(shape.stroke(.foreground, lineWidth: size / 16))
    }
}

@MainActor private func activateHeaderItem(_ store: EditorStore, entry: JSON) {
    switch entry["item"]["kind"].string {
    case "capy": store.invoke("zen_mode")
    case "settings": store.invoke("settings")
    case "tool": store.dispatch(["type":"activate_header_item", "id":entry["id"].raw])
    default: break
    }
}

private struct HeaderEditorBank: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var header: HeaderPresentation
    let view: JSON
    private var entries: [JSON] { view["model"]["zones"].array.flatMap(\.array) }
    var body: some View {
        ConfigurationFlow(spacing: 6, trailingLast: true) {
            chip("Add Tools…", source: JSON(["kind":"tools"]))
            ForEach(view["components"].array.indices, id: \.self) { index in
                let component = view["components"][index]
                if !component["singleton"].bool || !entries.contains(where: { $0["item"].stableKey == component["item"].stableKey }) {
                    chip(component["label"].string, source: JSON(["kind":"component", "value":component["item"].raw]))
                }
            }
            HStack(spacing: 8) {
                Picker("Title bar size", selection: Binding(get: { view["model"]["size"].string }, set: { header.action(["type":"set_size", "size":$0]) })) {
                    ForEach(view["sizes"].array.indices, id: \.self) { index in Text(view["sizes"][index]["label"].string).tag(view["sizes"][index]["id"].string) }
                }.labelsHidden().fixedSize().accessibilityIdentifier("header-size")
                Toggle("Show footer", isOn: Binding(get: { store.state["workspace"]["layout"]["canvas_info"]["visible"].bool },
                    set: { header.action(["type":"canvas_info", "visible":$0]) })).fixedSize()
                    .accessibilityIdentifier("header-footer")
                Button("Cancel") { header.action(["type":"cancel"]) }.accessibilityIdentifier("header-cancel")
                Button("Done") { header.action(["type":"edit", "editing":false]) }.accessibilityIdentifier("header-done")
            }.frame(height: 36)
        }.padding(6).background(EditorPalette(source: store.state["palette"])["panel"])
            .accessibilityElement(children: .contain).accessibilityIdentifier("header-editor")
    }
    private func chip(_ label: String, source: JSON) -> some View {
        HStack(spacing: 6) { SharedIcon(name: "grip", size: 12); Text(label).lineLimit(1) }
            .padding(.horizontal, 10).frame(height: 36)
            .background(EditorPalette(source: store.state["palette"])["bg"], in: RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle()).accessibilityElement(children: .ignore).accessibilityLabel(label)
            .accessibilityIdentifier("header-component-" + (source["kind"].string == "tools" ? "tools" : source["value"]["kind"].string))
            .modifier(HeaderSourceMeasurement(source: source))
    }
}

private struct HeaderCaption: ViewModifier {
    let enabled: Bool
    func body(content: Content) -> some View {
        #if os(macOS)
        content.gesture(WindowDragGesture(), isEnabled: enabled)
        #else
        content
        #endif
    }
}
private struct HeaderEditorHeight: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}
private extension JSON { var headerID: UInt64 { self["id"].uint } }
