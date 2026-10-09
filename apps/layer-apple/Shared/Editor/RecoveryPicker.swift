import SwiftUI

struct RecoveryPresentation: ViewModifier {
    @ObservedObject var store: EditorStore
    @ObservedObject var recovery: ArtworkRecovery
    func body(content: Content) -> some View {
        content
            #if DEBUG
            .overlay(alignment: .topLeading) {
                if ProcessInfo.processInfo.environment["CAPY_PERSISTENCE_PROBE"] == "1" {
                    Text(recovery.hasCurrentCopy ? "Recovery ready" : "Recovery pending")
                        .foregroundStyle(.clear).frame(width: 1, height: 1)
                        .accessibilityIdentifier("recovery-status")
                }
            }
            #endif
            .overlay(alignment: .bottom) {
                GeometryReader { geometry in
                    let notices = VStack(spacing: 8) {
                        if store.failure != nil || !store.snapshot["error"].isNull {
                            EditorNotice(message: store.canvasDiagnostic,
                                title: store.bootstrap[store.snapshot["gpu_ready"].bool ? "action_failed" : "canvas_init_failed"].string,
                                titleIdentifier: "Canvas error", working: store.restartingCanvas, detailIdentifier: "canvas-error-detail") {
                                if !store.snapshot["gpu_ready"].bool {
                                    Button(store.bootstrap["restart_canvas"].string) { store.restartCanvas() }.disabled(store.restartingCanvas).accessibilityIdentifier("Restart Canvas")
                                    Button(store.command("save_document_as")["label"].string) { store.invoke("save_document_as") }
                                        .disabled(!store.command("save_document_as")["enabled"].bool)
                                } else {
                                    Button(store.bootstrap["ok"].string) { store.failure = nil }
                                }
                            }
                        }
                        if let workspaces = store.workspaces { WorkspaceStartupNotice(workspaces: workspaces) }
                        ProjectProgressNotice(files: store.projectFiles, copy: recovery.copy)
                        if let error = recovery.visibleError {
                            EditorNotice(message: error, working: recovery.restoring || recovery.saving) {
                                Button(recovery.copy["retry"].string) { recovery.retry() }
                                    .disabled(recovery.restoring || recovery.saving).accessibilityIdentifier("recovery-retry")
                                Button(recovery.copy["later"].string) { recovery.later() }.accessibilityIdentifier("recovery-later")
                            }
                        }
                    }
                    ViewThatFits(in: .vertical) {
                        notices.fixedSize(horizontal: false, vertical: true)
                        EditorScrollView { notices }
                    }.padding(12).frame(maxWidth: .infinity, maxHeight: geometry.size.height, alignment: .bottom)
                }
            }
    }
}

struct EditorNotice<Actions: View>: View {
    let message: String
    var title: String?
    var titleIdentifier = ""
    var working = false
    var detailIdentifier = ""
    @ViewBuilder let actions: () -> Actions
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let title { Text(title).font(.headline).accessibilityIdentifier(titleIdentifier) }
            let detail = Text(message).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityIdentifier(detailIdentifier)
            ViewThatFits(in: .vertical) {
                detail.fixedSize(horizontal: false, vertical: true)
                ScrollView { detail }
            }.frame(maxHeight: 180)
            HStack(spacing: 12) {
                if working { ProgressView().controlSize(.small) }
                actions()
            }.buttonStyle(.bordered).controlSize(.regular)
        }.padding(16).frame(maxWidth: 580, alignment: .leading)
            .modifier(EditorPopupSurface(shape: RoundedRectangle(cornerRadius: 12)))
    }
}

struct RecoveryInteraction: ViewModifier {
    @ObservedObject var recovery: ArtworkRecovery
    func body(content: Content) -> some View {
        content.disabled(recovery.restoring).allowsHitTesting(!recovery.restoring)
    }
}
