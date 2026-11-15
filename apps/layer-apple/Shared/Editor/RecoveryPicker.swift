import SwiftUI

struct RecoveryPresentation: ViewModifier {
    @ObservedObject var store: EditorStore
    @ObservedObject var recovery: ArtworkRecovery
    private var phone: Bool { !store.snapshot["layout"]["phone"].isNull }
    private var responses: [EditorAlertResponse] {
        [EditorAlertResponse(label: recovery.copy["retry"].string, identifier: "recovery-retry", accent: true,
            enabled: !recovery.restoring && !recovery.saving, action: { recovery.retry() }),
         EditorAlertResponse(label: recovery.copy["later"].string, identifier: "recovery-later", cancel: true,
            action: { recovery.later() })]
    }
    private var recoveryBody: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(recovery.copy["explanation"].string)
            if recovery.restoring || recovery.saving { ProgressView().controlSize(.small) }
            if recovery.error == nil && !recovery.failedDrawings.isEmpty {
                ForEach(recovery.failedDrawings, id: \.failedDrawingID) { drawing in
                    VStack(alignment: .leading, spacing: 8) {
                        Text(drawing["error"].string).textSelection(.enabled)
                        Button(recovery.copy["discard"].string, role: .destructive) { recovery.discard(drawing["id"].uint) }
                            .disabled(recovery.restoring || recovery.saving)
                            .accessibilityIdentifier("recovery-discard-\(drawing["id"].uint)")
                    }
                }
            } else { Text(recovery.visibleError ?? "").textSelection(.enabled) }
        }
    }
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
                if !store.launchVisible {
                    GeometryReader { geometry in
                        let placement = store.noticePlacement(), minTextWidth = CGFloat(placement["min_text_width"].number)
                        let notices = VStack(spacing: 8) {
                            if store.failure != nil || !store.snapshot["error"].isNull {
                                EditorNotice(message: store.canvasDiagnostic,
                                    minTextWidth: minTextWidth,
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
                            if let workspaces = store.workspaces { WorkspaceStartupNotice(workspaces: workspaces, minTextWidth: minTextWidth) }
                            ProjectProgressNotice(files: store.projectFiles, copy: recovery.copy, minTextWidth: minTextWidth)
                            if !phone, let error = recovery.visibleError {
                                if recovery.error == nil && !recovery.failedDrawings.isEmpty {
                                    ForEach(recovery.failedDrawings, id: \.failedDrawingID) { drawing in
                                        EditorNotice(message: drawing["error"].string, minTextWidth: minTextWidth, working: recovery.restoring || recovery.saving) {
                                            recoveryActions
                                            Button(recovery.copy["discard"].string, role: .destructive) { recovery.discard(drawing["id"].uint) }
                                                .disabled(recovery.restoring || recovery.saving)
                                                .accessibilityIdentifier("recovery-discard-\(drawing["id"].uint)")
                                        }
                                    }
                                } else {
                                    EditorNotice(message: error, minTextWidth: minTextWidth, working: recovery.restoring || recovery.saving) { recoveryActions }
                                }
                            }
                        }
                        let stack = ViewThatFits(in: .vertical) {
                            notices.fixedSize(horizontal: false, vertical: true)
                            EditorScrollView { notices }
                        }
                        if phone {
                            let width = placement["max_width"].number
                            stack.frame(width: width, height: max(0, placement["y"].number), alignment: .bottom)
                                .offset(x: placement["x"].number - width / 2)
                        } else {
                            stack.padding(12).frame(maxWidth: .infinity, maxHeight: geometry.size.height, alignment: .bottom)
                        }
                    }
                }
            }.editorAlert(store: store, intent: recovery.copy["dialog"], title: recovery.copy["title"].string,
                isPresented: Binding(get: { phone && recovery.visibleError != nil }, set: { if !$0 { recovery.later() } }),
                responses: responses, bodyContent: AnyView(recoveryBody))
            .modifier(HostSurfacePresence(store: store, shown: recovery.visibleError != nil))
    }
    private var recoveryActions: some View {
        ForEach(responses) { response in
            Button(response.label, action: response.action).disabled(!response.enabled).accessibilityIdentifier(response.identifier)
        }
    }
}

private extension JSON { var failedDrawingID: UInt64 { self["id"].uint } }

struct EditorNotice<Actions: View>: View {
    let message: String
    let minTextWidth: CGFloat
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
            let messageBody = ViewThatFits(in: .vertical) {
                detail.fixedSize(horizontal: false, vertical: true)
                ScrollView { detail }
            }.frame(maxHeight: 180)
            let buttons = HStack(spacing: 12) {
                if working { ProgressView().controlSize(.small) }
                actions()
            }.buttonStyle(.bordered).controlSize(.regular)
            EditorNoticeRow(minTextWidth: minTextWidth) { messageBody; buttons }
        }.padding(16).frame(maxWidth: 580, alignment: .leading)
            .modifier(EditorPopupSurface(shape: RoundedRectangle(cornerRadius: 12)))
    }
}

private struct EditorNoticeRow: Layout {
    let minTextWidth: CGFloat
    private let gap: CGFloat = 12
    private func arrangement(_ proposal: ProposedViewSize, subviews: Subviews) -> AnyLayout {
        let text = subviews[0].sizeThatFits(.unspecified).width, actions = subviews[1].sizeThatFits(.unspecified).width
        let width = proposal.width ?? text + gap + actions
        let beside = ToolbarUI.resolve(["type": "notice_layout", "text": text, "content": width,
            "actions": actions, "gap": gap, "min_text_width": minTextWidth])["beside"].bool
        return beside ? AnyLayout(HStackLayout(spacing: gap)) : AnyLayout(VStackLayout(alignment: .trailing, spacing: gap))
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let layout = arrangement(proposal, subviews: subviews)
        var layoutCache = layout.makeCache(subviews: subviews)
        return layout.sizeThatFits(proposal: proposal, subviews: subviews, cache: &layoutCache)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let layout = arrangement(ProposedViewSize(bounds.size), subviews: subviews)
        var layoutCache = layout.makeCache(subviews: subviews)
        layout.placeSubviews(in: bounds, proposal: proposal, subviews: subviews, cache: &layoutCache)
    }
}

struct RecoveryInteraction: ViewModifier {
    @ObservedObject var recovery: ArtworkRecovery
    func body(content: Content) -> some View {
        content.disabled(recovery.restoring).allowsHitTesting(!recovery.restoring)
    }
}
