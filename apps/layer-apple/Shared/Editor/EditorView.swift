import SwiftUI

struct EditorView<Canvas: View>: View {
    @ObservedObject var store: EditorStore
    var showsApplicationMenus = true
    @ViewBuilder let canvas: () -> Canvas
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.openWindow) private var openWindow
    @State private var lastWindowRequest: UInt64 = 0
    private var windowRequest: UInt64 {
        store.state["requests"].array.first { $0["kind"]["type"].string == "new_window" }?["id"].uint ?? 0
    }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var panels: [String: JSON] {
        Dictionary(uniqueKeysWithValues: store.snapshot["panels"].array.map { ($0["id"].string, $0) })
    }
    var body: some View {
        ZStack(alignment: .topLeading) {
            canvas().ignoresSafeArea()
            if !store.canvasSubmitted {
                palette["bg"].ignoresSafeArea().allowsHitTesting(false)
            }
            if !store.state.isNull {
                if !store.snapshot["chrome_hidden"].bool { header }
                ForEach(store.snapshot["layout"]["groups"].array.indices, id: \.self) { i in
                    let group = store.snapshot["layout"]["groups"][i]
                    if !store.snapshot["chrome_hidden"].bool || (group["floating"].bool && !store.snapshot["hide_floating_panels"].bool) {
                        panelGroup(group).placed(group["bounds"])
                    }
                }
                if !store.snapshot["chrome_hidden"].bool {
                    HStack {
                        Spacer()
                        CameraStatus(camera: store.camera).padding(.horizontal, 8).padding(.vertical, 4)
                            .background(palette["bg"], in: Capsule())
                    }.placed(store.snapshot["layout"]["status"])
                }
                if !store.snapshot["chrome_hidden"].bool || store.snapshot["keep_zen_button"].bool {
                    let zen = store.command("zen_mode")
                    IconTile(icon: zen["icon"].string, label: zen["tooltip"].string,
                        selected: zen["selected"].bool, size: CGFloat(store.catalog["zen_icon_size"].number)) { store.invoke("zen_mode") }
                        .frame(width: 36, height: 36).background(palette["bg"], in: RoundedRectangle(cornerRadius: 6))
                        .offset(x: 6 + store.headerLeadingInset, y: 6).accessibilityIdentifier("zen-button")
                }
            }
            if let failure = store.failure {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Canvas error").font(.headline)
                    Text(failure).textSelection(.enabled)
                    Button("Dismiss") { store.failure = nil }
                }.padding(24).frame(maxWidth: 500).background(palette["panel"], in: RoundedRectangle(cornerRadius: 12))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            #if DEBUG
            if ProcessInfo.processInfo.environment["CAPY_PERSISTENCE_PROBE"] == "1" {
                Text(store.storagePending ? "Saving" : store.storageFailure == nil ? "Saved" : "Failed")
                    .foregroundStyle(.clear).frame(width: 1, height: 1)
                    .accessibilityIdentifier("persistence-status")
                    .accessibilityValue(store.state["theme"].string)
            }
            #endif
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .ignoresSafeArea().foregroundStyle(palette["text"])
        .font(.system(size: store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3))
        .tint(Color(red: 53 / 255, green: 132 / 255, blue: 228 / 255))
        .modifier(StorageAlert(store: store, active: store.snapshot["preferences"].isNull))
        .modifier(ProjectFilesModifier(files: store.projectFiles))
        .sheet(isPresented: Binding(get: { !store.snapshot["preferences"].isNull }, set: { if !$0 { store.dispatch(["type": "close_settings"]) } })) {
            SettingsView(store: store).modifier(StorageAlert(store: store))
        }
        .onAppear { systemTheme() }
        .onOpenURL { store.projectFiles.openURL($0) }
        .onChange(of: windowRequest, initial: true) { _, id in
            guard id > lastWindowRequest else { return }
            lastWindowRequest = id; openWindow(id: "editor")
            store.dispatch(["type": "complete_request", "id": id, "error": NSNull()])
        }
        .onChange(of: colorScheme) { _, _ in systemTheme() }
    }
    private func systemTheme() { store.dispatch(["type": "system_theme_changed", "theme": colorScheme == .dark ? "dark" : "light"]) }
    private var header: some View {
        ZStack {
            let tab = store.state["tabs"][0]
            Text(verbatim: "\(tab["title"].string) · \(Int(tab["width"].number)) × \(Int(tab["height"].number))")
                .fontWeight(.semibold).padding(.horizontal, 8).frame(height: 36)
                .background(palette["bg"], in: RoundedRectangle(cornerRadius: 6))
            HStack(spacing: 6) {
                Color.clear.frame(width: 36, height: 36)
                if showsApplicationMenus {
                    let menus = [store.catalog["file_menu"]] + store.catalog["menus"].array
                    ForEach(menus.indices, id: \.self) { index in
                        let menu = menus[index]
                        Menu { CatalogMenuItems(store: store, label: menu["label"].string) } label: {
                            Text(menu["label"].string).fontWeight(.bold).padding(.horizontal, 17).frame(height: 36)
                                .background(palette["bg"], in: RoundedRectangle(cornerRadius: 6))
                        }.buttonStyle(.plain)
                    }
                }
                Spacer()
                IconTile(icon: "settings", label: "Settings") { store.invoke("settings") }.frame(width: 36, height: 36)
            }.padding(.leading, store.headerLeadingInset)
        }.padding(6).frame(height: 48)
    }
    private func panelGroup(_ group: JSON) -> some View {
        let panel = panels[group["active"].string] ?? JSON()
        return VStack(spacing: 0) {
            if group["tabs_visible"].bool {
                HStack(spacing: 0) {
                    ForEach(group["panels"].array.indices, id: \.self) { index in
                        let tab = panels[group["panels"][index].string] ?? JSON()
                        Button { store.dispatch(["type": "select_panel_tab", "group": group["id"].raw, "panel": tab["id"].raw]) } label: {
                            HStack(spacing: 6) {
                                if tab["tab"]["show_icon"].bool { SharedIcon(name: tab["icon"].string) }
                                if tab["tab"]["show_name"].bool { Text(tab["title"].string).fontWeight(.bold).lineLimit(1) }
                            }.padding(.horizontal, 8).frame(height: 36)
                                .background(tab["id"].string == panel["id"].string ? palette["panel"] : Color.clear)
                        }.buttonStyle(.plain).accessibilityLabel(tab["title"].string)
                    }
                    Spacer(minLength: 0)
                    SharedIcon(name: "grip").opacity(0.65).frame(width: 20, height: 36)
                }.background(palette["tabbar"])
            }
            if !group["tiles"].isNull {
                ZStack(alignment: .topLeading) {
                    Color.clear
                    ForEach(panel["tiles"].array.indices, id: \.self) { index in
                        let tile = panel["tiles"][index]
                        IconTile(icon: tile["icon"].string, label: tile["tooltip"].string,
                            selected: tile["selected"].bool, enabled: tile["enabled"].bool) {
                            store.dispatch(["type": "activate_tile", "panel": panel["id"].raw, "tile": tile["id"].raw])
                        }.placed(group["tiles"]["tiles"][index])
                    }
                    SharedIcon(name: "grip").opacity(0.65).placed(group["tiles"]["grip"])
                }
            } else { PanelControls(store: store, panel: panel) }
        }.background(palette["panel"]).clipShape(RoundedRectangle(cornerRadius: 8))
            .shadow(color: .black.opacity(0.22), radius: 6, y: 2)
    }
}

private struct StorageAlert: ViewModifier {
    @ObservedObject var store: EditorStore
    var active = true
    func body(content: Content) -> some View {
        content.alert("Settings and Workspace", isPresented: Binding(
            get: { active && store.storageFailure != nil },
            set: { if !$0 && active { store.storageFailure = nil } })) {
            if store.canRetryStorage { Button("Retry Save") { store.native?.retryPersistence() } }
            Button("OK") { store.storageFailure = nil }
        } message: { Text(store.storageFailure ?? "") }
    }
}

private struct CameraStatus: View {
    @ObservedObject var camera: CameraReadout
    var body: some View {
        Text(verbatim: "\(Int((camera.value["zoom"].number * 100).rounded()))% · \(Int((camera.value["rotation"].number * 180 / .pi).rounded()))°")
            .accessibilityIdentifier("camera-status")
    }
}

struct MenuItems: View {
    @ObservedObject var store: EditorStore
    let sections: JSON
    var didInvoke: () -> Void = {}
    var body: some View {
        ForEach(sections.array.indices, id: \.self) { i in
            if i > 0 { Divider() }
            ForEach(sections[i].array.indices, id: \.self) { j in
                let item = sections[i][j]
                if !item["sections"].array.isEmpty {
                    Menu(item["label"].string) { AnyView(MenuItems(store: store, sections: item["sections"], didInvoke: didInvoke)) }.disabled(!item["enabled"].bool)
                } else {
                    Button { store.dispatch(item["action"]); didInvoke() } label: {
                        if item["selected"].bool { Label(item["label"].string, systemImage: "checkmark") }
                        else { Text(item["label"].string) }
                    }.disabled(!item["enabled"].bool)
                }
            }
        }
    }
}
