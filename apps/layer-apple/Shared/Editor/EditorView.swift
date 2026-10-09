import SwiftUI

struct EditorView<Canvas: View>: View {
    @ObservedObject var store: EditorStore
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
            canvas().ignoresSafeArea().modifier(PhotoDropTarget(store: store)).overlay(alignment: .topLeading) {
            ZStack(alignment: .topLeading) {
            if !store.canvasSubmitted {
                palette["bg"].ignoresSafeArea().allowsHitTesting(false)
            }
            if !store.canvasSubmitted && store.failure == nil && store.snapshot["error"].isNull && !store.bootstrap.isNull {
                ProgressView(store.bootstrap[store.restartingCanvas ? "restarting_canvas" :
                    store.snapshot["brush_ready"].bool ? "preparing_canvas" : "preparing_brush"].string)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("canvas-startup")
            }
            // Controls need both the live values and their shared specifications.
            if !store.state.isNull && !store.catalog.isNull {
                if !store.snapshot["chrome_hidden"].bool { EditorHeader(store: store) }
                if !store.snapshot["chrome_hidden"].bool && store.state["workspace"]["layout"]["canvas_info"]["visible"].bool {
                    HStack {
                        Spacer()
                        ScreenStatus(store: store, palette: palette)
                        CameraStatus(store: store, camera: store.camera).padding(.horizontal, 10).padding(.vertical, 3)
                            .glassSurface(SquircleShape.tile, fill: palette.chromeSurface)
                    }.placed(store.snapshot["layout"]["status"])
                }
                WorkspacePanels(store: store, workspace: store.workspace)
                if store.snapshot["keep_zen_button"].bool {
                    let command = store.command("zen_mode")
                    IconTile(icon: command["icon"].string, label: command["tooltip"].string,
                        enabled: command["enabled"].bool, corner: .half) { store.invoke("zen_mode") }
                        .frame(width: 36, height: 36)
                        .glassSurface(SquircleShape.tile, fill: palette.chromeSurface)
                        .modifier(WorkspaceContext(store: store, target: JSON(["kind": "zen_mode"])))
                        .onGeometryChange(for: CGRect.self) { $0.frame(in: .named("editor-workspace")) } action: {
                            store.workspace.zenButton = $0
                        }
                        .onDisappear { store.workspace.zenButton = nil }
                        .offset(x: store.headerLeadingInset + 6, y: 6).accessibilityIdentifier("zen-button")
                }
            }
            HStack {
                ToneStatusLabel(store: store, palette: palette)
                ProofIndicator(model: store.proof, palette: palette)
            }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
                .padding(12).placed(store.snapshot["layout"]["work_area"])
            }
            }.modifier(RecoveryInteraction(recovery: store.recovery))
                .modifier(OptionalWorkspaceInteraction(store: store))
            #if DEBUG
            if ProcessInfo.processInfo.environment["CAPY_PERSISTENCE_PROBE"] == "1" {
                PersistenceProbe(store: store)
            }
            if ProcessInfo.processInfo.environment["CAPY_GPU_RECOVERY_TEST"] == "1" {
                HStack {
                    Button("Lose test device") { store.native?.testGpuFault(validation: false) }
                    Button("Validate test failure") { store.native?.testGpuFault(validation: true) }
                    Text(store.snapshot["gpu_ready"].bool && store.snapshot["brush_ready"].bool ? "Renderer ready" : "Renderer stopped")
                        .accessibilityIdentifier("renderer-test-status")
                }.padding(6).background(palette["panel"])
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            }
            #endif
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .modifier(EditorPopoverHost())
        .coordinateSpace(name: "editor-workspace")
        .editorChromeContact { point in
            store.layerSwipe.contact(at: point)
            store.workspace.chrome(["kind": "contact", "position": [point.x, point.y], "canvas": false])
        }
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
        .ignoresSafeArea()
        .overlay { CommandSearchLayer(store: store).ignoresSafeArea(.container) }
        .foregroundStyle(palette["text"])
        .font(.system(size: store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3))
        .tint(palette.accent)
        .modifier(StorageAlert(store: store, active: store.snapshot["preferences"].isNull))
        .modifier(OptionalWorkspaceManager(store: store))
        .modifier(ProjectFilesModifier(files: store.projectFiles))
        .modifier(DrawingTabsPresentation(store: store, tabs: store.drawingTabs))
        .modifier(ProofPresentation(model: store.proof))
        .modifier(RecoveryPresentation(store: store, recovery: store.recovery))
        .modifier(WorkspaceDialogs(store: store))
        .modifier(SizeDialogs(store: store))
        .modifier(PaletteFiles(controller: store.palettes))
        .modifier(StrokeRecordingFiles(recording: store.strokeRecording))
        .modifier(ColorEditingPresentation(controller: store.colorEditing))
        .sheet(isPresented: Binding(get: { !store.snapshot["preferences"].isNull }, set: { if !$0 { store.dispatch(["type": "close_settings"]) } }), onDismiss: { store.focusCanvas?() }) {
            SettingsView(store: store).modifier(StorageAlert(store: store)).modifier(EditorPopoverHost())
                .foregroundStyle(.primary).presentationBackground(.background)
                .modifier(EditorPresentationAppearance())
        }
        .environment(\.editorPopupStore, store)
        .environment(\.capyInterfaceLanguage, store.interfaceLanguage)
        .environment(\.capyNativeCopy, store.catalog["native_copy"])
        .environment(\.paintPair, store.paintPair)
        .environment(\.capyCommonCopy, store.bootstrap["common"])
        .environment(\.locale, store.interfaceLanguage.isEmpty ? Locale.current : Locale(identifier: store.interfaceLanguage))
        .environment(\.editorPalette, palette)
        .environment(\.glassRegistry, store.glass)
        .environment(\.colorScheme, store.state.isNull ? colorScheme : store.state["theme"].string == "dark" ? .dark : .light)
        .background(NativeEditorAppearance(store: store, preferred: store.state["settings"]["theme"].string))
        .onOpenURL { url in
            if let workspaces = store.workspaces, let kind = WorkspacePackageKind.forURL(url) { workspaces.openURL(url, kind: kind) }
            else { store.projectFiles.openURL(url) }
        }
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
                    completeLink(id, accepted: false)
                    return
                }
                openURL(url) { accepted in completeLink(id, accepted: accepted) }
            }
        }
    }
    private func completeLink(_ id: UInt64, accepted: Bool) {
        let error = accepted ? nil : "Could not open the link"
        store.dispatch(["type": "complete_request", "id": id, "error": error as Any? ?? NSNull()])
        if let error { store.failure = error }
    }
}

private struct OptionalWorkspaceInteraction: ViewModifier {
    @ObservedObject var store: EditorStore
    func body(content: Content) -> some View {
        if let workspaces = store.workspaces { content.modifier(WorkspaceInteraction(workspaces: workspaces)) }
        else { content }
    }
}

private struct OptionalWorkspaceManager: ViewModifier {
    @ObservedObject var store: EditorStore
    func body(content: Content) -> some View {
        if let workspaces = store.workspaces {
            content.modifier(WorkspaceManagerPresentation(workspaces: workspaces))
        } else { content }
    }
}

private struct StorageAlert: ViewModifier {
    @ObservedObject var store: EditorStore
    var active = true
    func body(content: Content) -> some View {
        content.alert(store.bootstrap["recovery"]["attention"].string, isPresented: Binding(
            get: { active && store.storageFailure != nil },
            set: { if !$0 && active { store.storageFailure = nil } })) {
            if store.canRetryStorage { Button(store.bootstrap["recovery"]["retry"].string) { store.native?.retryPersistence() } }
            Button(store.bootstrap["common"]["ok"].string) { store.storageFailure = nil }
        } message: { Text(store.storageFailure ?? "") }
    }
}

private struct CameraStatus: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var camera: CameraReadout
    @State private var menu: JSON?
    private var copy: JSON { store.catalog["native_copy"]["header"] }
    private var refreshKey: String {
        JSON([camera.value["zoom"].raw, camera.value["rotation"].raw, camera.value["zoom_locked"].raw,
            camera.value["rotation_locked"].raw, store.catalog["navigator_commands"].array.map { store.command($0.string)["enabled"].raw }]).stableKey
    }
    private func refresh() {
        store.query(["type": "zoom_menu"]) { model in
            guard menu != nil else { return }
            menu = model
        }
    }
    private func sections(_ model: JSON, rotation: Bool) -> AppleContextMenu {
        let all = model["sections"].array, boundary = min(all.count, Int(model["rotation_section"].uint))
        let part = rotation ? Array(all[boundary...]) : Array(all[..<boundary])
        return AppleContextMenu(JSON(["title": "", "sections": part.map(\.raw)])) { store.dispatch($0) }
    }
    var body: some View {
        Button {
            if menu == nil { menu = JSON(); refresh() } else { menu = nil }
        } label: {
            Text(verbatim: "\(Int((camera.value["zoom"].number * 100).rounded()))% · \(Int((camera.value["rotation"].number * 180 / .pi).rounded()))°")
        }.buttonStyle(.plain).focusable(false).help(copy["zoom"].string)
            .accessibilityIdentifier("camera-status")
            .onChange(of: refreshKey) { if menu != nil { refresh() } }
            .editorPopover(isPresented: Binding(get: { menu != nil }, set: { if !$0 { menu = nil } })) {
                let model = menu ?? JSON()
                EditorScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        NumberControl(store: store, label: copy["zoom"].string, value: camera.value["zoom"].number,
                            control: store.catalog["zoom"], identifier: "zoom", inline: true) { value, completion in
                            store.edit(["type": "set_zoom", "zoom": value], completion: completion)
                        }.padding(.horizontal, 10).padding(.top, 8)
                        Divider()
                        EditorActionMenu(model: sections(model, rotation: false), width: 240, identifier: "zoom-menu",
                            rootFocusesSelection: false, capturesKeys: false) { menu = nil }.fixedSize(horizontal: false, vertical: true)
                        Divider()
                        NumberControl(store: store, label: copy["rotation"].string, value: camera.value["rotation"].number,
                            control: store.catalog["rotation"], identifier: "rotation", inline: true) { value, completion in
                            store.edit(["type": "set_rotation", "rotation": value], completion: completion)
                        }.padding(.horizontal, 10)
                        Divider()
                        EditorActionMenu(model: sections(model, rotation: true), width: 240, identifier: "rotation-menu",
                            rootFocusesSelection: false, capturesKeys: false) { menu = nil }.fixedSize(horizontal: false, vertical: true)
                        Divider()
                        NavigationButtons(store: store, prefix: "zoom").padding([.horizontal, .bottom], 8)
                    }
                }.frame(width: 240).frame(maxHeight: 640).fixedSize(horizontal: false, vertical: true)
            }
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
                    Button { invoke(item["action"]) } label: {
                        if item["selected"].bool { Label(item["label"].string, systemImage: "checkmark") }
                        else { Text(item["label"].string) }
                    }.disabled(!item["enabled"].bool || item["action"].isNull)
                        .help(item["hint"].string)
                        .accessibilityIdentifier(item["action"]["type"].string == "invoke"
                            ? "command-" + item["action"]["command"].string : "menu-action-" + item["label"].string)
                        .keyboardShortcut(usesShortcuts && store.snapshot["preferences"].isNull && !store.projectFiles.blocksEditor
                            ? menuShortcut(item["bindings"][0]) : nil)
                }
            }
        }
    }
    private func invoke(_ action: JSON) {
        #if os(macOS)
        if nativeTextMenuAction(action) { didInvoke(); return }
        #endif
        if action["type"].string == "invoke", ["undo", "redo"].contains(action["command"].string),
           store.palettes.undoReorder(redo: action["command"].string == "redo") { didInvoke(); return }
        store.dispatch(action); didInvoke()
    }
}

#if DEBUG
private struct PersistenceProbe: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        if let workspaces = store.workspaces { ManagedPersistenceProbe(store: store, workspaces: workspaces) }
        else { Self.label(store, saving: false, failed: false) }
    }
    static func label(_ store: EditorStore, saving: Bool, failed: Bool) -> some View {
        Text(failed || store.storageFailure != nil ? "Failed" : saving || store.storagePending ? "Saving" : "Saved")
            .foregroundStyle(.clear).frame(width: 1, height: 1)
            .accessibilityIdentifier("persistence-status").accessibilityValue(store.state["theme"].string)
    }
}
private struct ManagedPersistenceProbe: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var workspaces: WorkspaceController
    var body: some View { PersistenceProbe.label(store, saving: workspaces.hasUnsavedChanges, failed: workspaces.error != nil) }
}
#endif
