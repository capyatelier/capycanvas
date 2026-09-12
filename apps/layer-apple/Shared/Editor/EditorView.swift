import SwiftUI

struct EditorView<Canvas: View>: View {
    @ObservedObject var store: EditorStore
    var showsApplicationMenus = true
    @ViewBuilder let canvas: () -> Canvas
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.openWindow) private var openWindow
    @Environment(\.supportsMultipleWindows) private var supportsMultipleWindows
    @State private var lastWindowRequest: UInt64 = 0
    @State private var lastLinkRequest: UInt64 = 0
    @Environment(\.openURL) private var openURL
    private var linkRequest: UInt64 {
        store.state["requests"].array.first { $0["kind"]["type"].string == "open_link" }?["id"].uint ?? 0
    }
    private var windowRequest: UInt64 {
        store.state["requests"].array.first { $0["kind"]["type"].string == "new_window" }?["id"].uint ?? 0
    }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        ZStack(alignment: .topLeading) {
            canvas().ignoresSafeArea()
            if !store.canvasSubmitted {
                palette["bg"].ignoresSafeArea().allowsHitTesting(false)
            }
            if !store.state.isNull {
                if !store.snapshot["chrome_hidden"].bool { header }
                WorkspacePanels(store: store, workspace: store.workspace)
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
                        .modifier(WorkspaceContext(store: store, target: JSON(["kind": "zen_mode"])))
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
        .coordinateSpace(name: "editor-workspace")
        .simultaneousGesture(SpatialTapGesture(coordinateSpace: .named("editor-workspace")).onEnded { event in
            store.workspace.chrome(["kind": "contact", "position": [event.location.x, event.location.y], "canvas": false])
        })
        .onContinuousHover(coordinateSpace: .named("editor-workspace")) { phase in
            switch phase {
            case .active(let point): store.workspace.chrome(["kind": "motion", "position": [point.x, point.y]])
            case .ended: store.workspace.chrome(["kind": "leave", "touch": false])
            }
        }
        .onPreferenceChange(WorkspaceTabs.self) { bounds in
            store.workspace.tabs = bounds.compactMap { key, rect in
                let parts = key.split(separator: ":").compactMap { UInt64($0) }
                guard parts.count == 2 else { return nil }
                return JSON(["group": parts[0], "index": parts[1], "bounds": ["x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height]])
            }
        }
        .onPreferenceChange(WorkspaceTabFrames.self) { store.workspace.tabFrames = $0 }
        .ignoresSafeArea().foregroundStyle(palette["text"])
        .font(.system(size: store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3))
        .tint(Color(red: 53 / 255, green: 132 / 255, blue: 228 / 255))
        .modifier(StorageAlert(store: store, active: store.snapshot["preferences"].isNull))
        .modifier(ProjectFilesModifier(files: store.projectFiles))
        .modifier(RecoveryPresentation(recovery: store.recovery))
        .modifier(WorkspaceDialogs(store: store))
        .sheet(isPresented: Binding(get: { !store.snapshot["preferences"].isNull }, set: { if !$0 { store.dispatch(["type": "close_settings"]) } })) {
            SettingsView(store: store).modifier(StorageAlert(store: store))
        }
        .onAppear { systemTheme() }
        .onOpenURL { store.projectFiles.openURL($0) }
        .onChange(of: windowRequest, initial: true) { _, id in
            guard id > lastWindowRequest else { return }
            lastWindowRequest = id
            if supportsMultipleWindows {
                openWindow(id: "editor")
                store.dispatch(["type": "complete_request", "id": id, "error": NSNull()])
            } else {
                let error = "Multiple drawing windows are unavailable in this environment"
                store.dispatch(["type": "complete_request", "id": id, "error": error])
                store.projectFiles.error = error
            }
        }
        .onChange(of: linkRequest, initial: true) { _, id in
            guard id > lastLinkRequest,
                let request = store.state["requests"].array.first(where: { $0["id"].uint == id }) else { return }
            lastLinkRequest = id
            store.query(["type": "application_link", "link": request["kind"]["link"].raw]) { result in
                guard let url = URL(string: result.string) else {
                    store.dispatch(["type": "complete_request", "id": id, "error": "Could not open the link"])
                    return
                }
                openURL(url) { accepted in
                    store.dispatch(["type": "complete_request", "id": id,
                        "error": accepted ? NSNull() : "Could not open the link" as Any])
                }
            }
        }
        .onChange(of: colorScheme) { _, _ in systemTheme() }
    }
    private func systemTheme() { store.dispatch(["type": "system_theme_changed", "theme": colorScheme == .dark ? "dark" : "light"]) }
    private var header: some View {
        EditorHeaderLayout {
            HStack(spacing: 6) {
                Color.clear.frame(width: 36 + store.headerLeadingInset, height: 36)
                if showsApplicationMenus {
                    ApplicationMenus(store: store, palette: palette)
                }
            }
            let tab = store.state["tabs"][0]
            Text(verbatim: "\(tab["title"].string) · \(Int(tab["width"].number)) × \(Int(tab["height"].number))")
                .lineLimit(1)
                .accessibilityIdentifier("document-title")
                .fontWeight(.semibold).padding(.horizontal, 8).frame(height: 36)
                .background(palette["bg"], in: RoundedRectangle(cornerRadius: 6))
            HStack(spacing: 6) {
                if SystemStatus.visible(policy: store.state["settings"]["show_clock"].string, fullscreen: store.state["fullscreen"].bool) {
                    SystemStatusView(palette: palette, dark: store.state["theme"].string == "dark")
                }
                IconTile(icon: "settings", label: "Settings") { store.invoke("settings") }.frame(width: 36, height: 36)
                    .background(palette["bg"], in: RoundedRectangle(cornerRadius: 6))
            }
        }.padding(6).frame(height: 48)
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
    var usesShortcuts = false
    var body: some View {
        ForEach(sections.array.indices, id: \.self) { i in
            if i > 0 { Divider() }
            ForEach(sections[i].array.indices, id: \.self) { j in
                let item = sections[i][j]
                if !item["sections"].array.isEmpty {
                    Menu(item["label"].string) { AnyView(MenuItems(store: store, sections: item["sections"], didInvoke: didInvoke, usesShortcuts: usesShortcuts)) }.disabled(!item["enabled"].bool)
                } else {
                    Button { store.dispatch(item["action"]); didInvoke() } label: {
                        if item["selected"].bool { Label(item["label"].string, systemImage: "checkmark") }
                        else { Text(item["label"].string) }
                    }.disabled(!item["enabled"].bool || item["action"].isNull)
                        .help(item["hint"].string)
                        .accessibilityIdentifier(item["action"]["type"].string == "invoke"
                            ? "command-" + item["action"]["command"].string : "menu-action-" + item["label"].string)
                        .keyboardShortcut(usesShortcuts ? menuShortcut(item["bindings"][0]) : nil)
                }
            }
        }
    }
}

/// Keep the title centered where space permits, clamping beside native menus.
/// Narrow windows use the complete submenu list through ViewThatFits.
private struct EditorHeaderLayout: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        CGSize(width: proposal.width ?? 800, height: 36)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard subviews.count == 3 else { return }
        let title = subviews[1].sizeThatFits(.unspecified)
        let trailing = subviews[2].sizeThatFits(.unspecified)
        let available = max(0, bounds.width - title.width - trailing.width - 12)
        let ideal = subviews[0].sizeThatFits(.unspecified)
        let leading = ideal.width <= available ? ideal : subviews[0].sizeThatFits(ProposedViewSize(width: available, height: 36))
        subviews[0].place(at: bounds.origin, proposal: ProposedViewSize(leading))
        let titleWidth = min(title.width, max(0, bounds.width - leading.width - trailing.width - 12))
        let center = max(leading.width + 6, min((bounds.width - titleWidth) / 2, bounds.width - trailing.width - 6 - titleWidth))
        subviews[1].place(at: CGPoint(x: bounds.minX + center, y: bounds.minY),
            proposal: ProposedViewSize(width: titleWidth, height: 36))
        subviews[2].place(at: CGPoint(x: bounds.maxX - trailing.width, y: bounds.minY), proposal: ProposedViewSize(trailing))
    }
}
