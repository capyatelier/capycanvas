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
            canvas().ignoresSafeArea()
            if !store.canvasSubmitted {
                palette["bg"].ignoresSafeArea().allowsHitTesting(false)
            }
            // Controls need both the live values and their shared specifications.
            if !store.state.isNull && !store.catalog.isNull {
                if !store.snapshot["chrome_hidden"].bool { EditorHeader(store: store) }
                if !store.snapshot["chrome_hidden"].bool && store.state["workspace"]["layout"]["canvas_info"]["visible"].bool {
                    HStack {
                        Spacer()
                        CameraStatus(camera: store.camera).padding(.horizontal, 8).padding(.vertical, 4)
                            .background(palette["bg"], in: Capsule())
                    }.placed(store.snapshot["layout"]["status"])
                }
                WorkspacePanels(store: store, workspace: store.workspace)
            }
            if let failure = store.failure ?? (store.snapshot["error"].isNull ? nil : store.snapshot["error"].string) {
                VStack(alignment: .leading, spacing: 12) {
                    Text("Canvas error").font(.headline)
                    Text(failure).textSelection(.enabled)
                    if !store.snapshot["gpu_ready"].bool {
                        HStack {
                            Button("Restart Canvas") { store.restartCanvas() }.disabled(store.restartingCanvas)
                            Button("Save As…") { store.invoke("save_document_as") }
                                .disabled(!store.command("save_document_as")["enabled"].bool)
                        }
                    } else {
                        Button("Dismiss") { store.failure = nil }
                    }
                }.padding(24).frame(maxWidth: 500).background(palette["panel"], in: RoundedRectangle(cornerRadius: 12))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
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
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
            }
            #endif
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .modifier(EditorPopoverHost())
        .coordinateSpace(name: "editor-workspace")
        .editorChromeContact { point in
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
        .ignoresSafeArea().foregroundStyle(palette["text"])
        .font(.system(size: store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3))
        .tint(palette.accent)
        .modifier(StorageAlert(store: store, active: store.snapshot["preferences"].isNull))
        .modifier(OptionalWorkspaceManager(store: store))
        .modifier(ProjectFilesModifier(files: store.projectFiles))
        .modifier(RecoveryPresentation(recovery: store.recovery))
        .modifier(WorkspaceDialogs(store: store))
        .sheet(isPresented: Binding(get: { !store.snapshot["preferences"].isNull }, set: { if !$0 { store.dispatch(["type": "close_settings"]) } }), onDismiss: { store.focusCanvas?() }) {
            SettingsView(store: store).modifier(StorageAlert(store: store)).modifier(EditorPopoverHost())
                .foregroundStyle(.primary).presentationBackground(.background)
                .modifier(EditorPresentationAppearance())
        }
        .environment(\.editorPopupStore, store)
        .environment(\.colorScheme, store.state.isNull ? colorScheme : store.state["theme"].string == "dark" ? .dark : .light)
        .background(NativeEditorAppearance(store: store, preferred: store.state["settings"]["theme"].string))
        .onOpenURL { url in
            if store.workspaceLibrary != nil, let kind = WorkspacePackageKind.forURL(url) { store.workspaceManager.openURL(url, kind: kind) }
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

private struct OptionalWorkspaceManager: ViewModifier {
    @ObservedObject var store: EditorStore
    func body(content: Content) -> some View {
        if let library = store.workspaceLibrary {
            content.modifier(WorkspaceManagerPresentation(manager: store.workspaceManager, library: library))
        } else { content }
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
                    Button { invoke(item["action"]) } label: {
                        if item["selected"].bool { Label(item["label"].string, systemImage: "checkmark") }
                        else { Text(item["label"].string) }
                    }.disabled(!item["enabled"].bool || item["action"].isNull)
                        .help(item["hint"].string)
                        .accessibilityIdentifier(item["action"]["type"].string == "invoke"
                            ? "command-" + item["action"]["command"].string : "menu-action-" + item["label"].string)
                        .keyboardShortcut(usesShortcuts && store.snapshot["preferences"].isNull
                            ? menuShortcut(item["bindings"][0]) : nil)
                }
            }
        }
    }
    private func invoke(_ action: JSON) {
        #if os(macOS)
        if nativeTextMenuAction(action) { didInvoke(); return }
        #endif
        store.dispatch(action); didInvoke()
    }
}

#if DEBUG
private struct PersistenceProbe: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        if let library = store.workspaceLibrary { ManagedPersistenceProbe(store: store, library: library) }
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
    @ObservedObject var library: WorkspaceLibrary
    var body: some View { PersistenceProbe.label(store, saving: library.hasUnsavedChanges, failed: library.error != nil) }
}
#endif
