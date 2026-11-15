import SwiftUI

@MainActor final class CanvasNoticePresence: ObservableObject {
    @Published private(set) var notice = JSON()
    private var shown: UInt64?
    private var timeout: DispatchWorkItem?
    var answer: (UInt64, Bool, String?) -> Void = { _, _, _ in }
    func publish(_ next: JSON) {
        guard !next.isNull else { hide(); shown = nil; return }
        let id = next["id"].uint
        guard id != shown else { if !notice.isNull { notice = next }; return }
        shown = id; notice = next
        timeout?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.shown == id, !self.notice.isNull else { return }
            self.notice = JSON(); self.answer(id, false, nil)
        }
        timeout = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 4, execute: work)
    }
    func hide() {
        timeout?.cancel(); timeout = nil
        if !notice.isNull { notice = JSON() }
    }
    func accept(_ action: String) {
        guard let id = shown, !notice.isNull else { return }
        hide(); answer(id, true, action)
    }
}

struct CanvasFloorLayer: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var presence: CanvasNoticePresence
    @ObservedObject var bar: CanvasBarPresence
    @State private var previewBar: CGRect?
    @State private var noticeBounds: CGRect?
    @State private var previewPresentation = JSON()
    @State private var previewBounds = JSON()
    @State private var previewHeight: CGFloat = 0
    @State private var previewKey = ""
    private static let previewWidth: CGFloat = 360
    private func placement(above bar: CGRect?, bubble: CGRect? = nil) -> JSON {
        DialogUI.resolve(["kind": "notice_above", "notice": store.snapshot["layout"]["notice"].raw,
            "bar": bar?.json as Any? ?? NSNull(), "bubble": bubble?.json as Any? ?? NSNull()])
    }
    var body: some View {
        let tools = store.state["layer_tools"]
        let preview: (view: JSON, operations: CanvasPreviewPanel.Operations)? = !tools["selection_resize"].isNull ? (tools["selection_resize"], .refine)
            : !tools["frequency_separation"].isNull ? (tools["frequency_separation"], .frequencySeparation) : nil
        GeometryReader { geometry in
            let height = min(geometry.size.height, max(0, store.snapshot["layout"]["viewport"][1].number - store.obscuredBottom))
            let visible = JSON(["x": 0, "y": 0, "width": geometry.size.width, "height": height])
            let key = JSON([visible.raw, store.snapshot["layout"]["phone"].raw,
                preview?.operations.identifier ?? "", previewHeight]).stableKey
            ZStack(alignment: .topLeading) {
                Color.clear.frame(width: 0, height: 0)
                if !presence.notice.isNull || preview != nil {
                    let below = preview == nil ? bar.bounds : previewBar
                    let placement = placement(above: below)
                    let width = CGFloat(placement["max_width"].number)
                    let palette = EditorPalette(source: store.state["palette"])
                    if !presence.notice.isNull {
                        notice(palette.glassy, width: width, minTextWidth: CGFloat(placement["min_text_width"].number))
                            .onGeometryChange(for: CGRect.self) { $0.frame(in: .named("editor-workspace")) } action: { noticeBounds = $0 }
                            .onDisappear { noticeBounds = nil }
                            .frame(width: width, height: max(0, placement["y"].number), alignment: .bottom)
                            .offset(x: placement["x"].number - width / 2)
                    }
                    if let preview, previewPresentation["kind"].string == "floating" {
                        let status = self.placement(above: below, bubble: presence.notice.isNull ? nil : noticeBounds)
                        CanvasPreviewPanel(store: store, view: preview.view, operations: preview.operations, palette: palette)
                            .frame(width: min(Self.previewWidth, width))
                            .frame(width: width, height: max(0, status["y"].number), alignment: .bottom)
                            .offset(x: status["x"].number - width / 2)
                    }
                }
                if let preview, previewPresentation["kind"].string == "canvas_panel", !previewBounds.isNull {
                    ScrollView {
                        CanvasPreviewPanel(store: store, view: preview.view, operations: preview.operations,
                            palette: EditorPalette(source: store.state["palette"]))
                            .frame(maxWidth: .infinity)
                            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { previewHeight = $0 }
                    }.placed(previewBounds)
                }
            }.onChange(of: key, initial: true) { _, expected in
                previewKey = expected
                guard preview != nil else { previewPresentation = JSON(); previewBounds = JSON(); return }
                store.query(["type": "dialog_presentation", "role": "canvas_panel", "visible": visible.raw]) { next in
                    guard previewKey == expected else { return }
                    previewPresentation = next
                    guard next["kind"].string == "canvas_panel" else { previewBounds = JSON(); return }
                    store.query(["type": "dialog_bounds", "role": "canvas_panel", "visible": visible.raw,
                        "measured": [Self.previewWidth, previewHeight]]) { bounds in
                        guard previewKey == expected else { return }; previewBounds = bounds
                    }
                }
            }
        }.onChange(of: preview == nil) { _, closed in if !closed { previewBar = bar.bounds } }
            .onChange(of: bar.bounds) { _, bounds in if let bounds { previewBar = bounds } }
    }
    private var textSize: CGFloat { store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3 }
    private func notice(_ palette: EditorPalette, width: CGFloat, minTextWidth: CGFloat) -> some View {
        let metrics = store.barMetrics, text = presence.notice["text"].string, actions = presence.notice["actions"].array
        let widths = actions.map { ceil(toolbarTextWidth($0["label"].string, size: textSize)) + 2 * (BarMetrics.textPadding + metrics.clear) }
        let actionsWidth = widths.reduce(0, +) + CGFloat(max(0, widths.count - 1)) * metrics.gap
        let caption = ceil(toolbarTextWidth(text, size: textSize))
        let beside = ToolbarUI.cached(["type": "notice_layout",
            "text": BarMetrics.captionLeading + caption + BarMetrics.captionTrailing, "content": width,
            "actions": actionsWidth, "gap": metrics.gap, "min_text_width": minTextWidth],
            language: store.interfaceLanguage)["beside"].bool
        return Group {
            if beside {
                HStack(spacing: metrics.gap) {
                    Text(text).lineLimit(actions.isEmpty ? nil : 1).fixedSize(horizontal: !actions.isEmpty, vertical: true)
                        .padding(.vertical, 6).padding(.leading, BarMetrics.captionLeading)
                        .padding(.trailing, actions.isEmpty ? BarMetrics.captionLeading : BarMetrics.captionTrailing)
                        .allowsHitTesting(false).accessibilityIdentifier("canvas-notice-text")
                    if !actions.isEmpty { actionRow(actions, palette: palette, metrics: metrics) }
                }.frame(minHeight: metrics.tile)
            } else {
                VStack(alignment: .trailing, spacing: metrics.gap) {
                    Text(text).fixedSize(horizontal: false, vertical: true)
                        .padding(.top, 10).padding(.horizontal, BarMetrics.captionLeading).padding(.bottom, 2)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .allowsHitTesting(false).accessibilityIdentifier("canvas-notice-text")
                    actionRow(actions, palette: palette, metrics: metrics)
                }.frame(width: min(width, max(2 * BarMetrics.captionLeading + caption, actionsWidth)))
            }
        }.foregroundStyle(palette["text"])
            .environment(\.editorPalette, palette)
            .background { SquircleShape.surface.fill(palette.glassPanel).allowsHitTesting(false) }
            .modifier(GlassRegistration(shape: SquircleShape.surface))
            .background { OutsideShadow(shape: SquircleShape.surface, opacity: 0.16, radius: 4, y: 2).accessibilityHidden(true) }
            .frame(maxWidth: width)
            .accessibilityElement(children: .contain).accessibilityLabel(presence.notice["name"].string).accessibilityIdentifier("canvas-notice")
    }
    private func actionRow(_ actions: [JSON], palette: EditorPalette, metrics: BarMetrics) -> some View {
        HStack(spacing: metrics.gap) {
            ForEach(actions.indices, id: \.self) { index in
                let action = actions[index], token = action["id"].string, enabled = action["enabled"].bool
                let accent = action["accent"].bool && enabled
                Button { presence.accept(token) } label: {
                    Text(action["label"].string).lineLimit(1).fixedSize()
                        .padding(.horizontal, BarMetrics.textPadding)
                        .foregroundStyle(accent ? palette.accentForeground : palette["text"])
                        .frame(maxHeight: .infinity)
                        .background(accent ? palette.accent : .clear, in: SquircleShape.tile)
                        .contentShape(Rectangle())
                }.buttonStyle(EditorControlButtonStyle(selected: false, corner: .half))
                    .frame(height: metrics.control).padding(metrics.clear)
                    .disabled(!enabled).opacity(enabled ? 1 : 0.36)
                    .help(action["reason"].string).accessibilityHint(action["reason"].string)
                    .accessibilityIdentifier("canvas-notice-action-\(token)")
            }
        }
    }
}
