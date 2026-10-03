import SwiftUI

@MainActor final class CanvasNoticePresence: ObservableObject {
    @Published private(set) var notice = JSON()
    private var shown: UInt64?
    private var timeout: DispatchWorkItem?
    var answer: (UInt64, Bool) -> Void = { _, _ in }
    func publish(_ next: JSON) {
        guard !next.isNull else { hide(); shown = nil; return }
        let id = next["id"].uint
        guard id != shown else { return }
        shown = id; notice = next
        timeout?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.shown == id, !self.notice.isNull else { return }
            self.notice = JSON(); self.answer(id, false)
        }
        timeout = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 4, execute: work)
    }
    func hide() {
        timeout?.cancel(); timeout = nil
        if !notice.isNull { notice = JSON() }
    }
    func accept() {
        guard let id = shown, !notice.isNull else { return }
        hide(); answer(id, true)
    }
}

struct CanvasFloorLayer: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var presence: CanvasNoticePresence
    @ObservedObject var bar: CanvasBarPresence
    @State private var previewBar: CGRect?
    private static let margin: CGFloat = 12, barReach: CGFloat = 72, maxWidth: CGFloat = 720, previewWidth: CGFloat = 360
    var body: some View {
        let tools = store.state["layer_tools"]
        let preview: (view: JSON, operations: CanvasPreviewPanel.Operations)? = !tools["selection_resize"].isNull ? (tools["selection_resize"], .refine)
            : !tools["frequency_separation"].isNull ? (tools["frequency_separation"], .frequencySeparation) : nil
        ZStack(alignment: .topLeading) {
            Color.clear.frame(width: 0, height: 0)
            if !presence.notice.isNull || preview != nil {
                let layout = store.snapshot["layout"], area = layout["work_area"].rect, status = layout["status"].rect
                let floor = min(area.maxY, status.height > 0 ? status.minY : .infinity)
                let below = preview == nil ? bar.bounds : previewBar
                let bottom = below.map { $0.maxY > floor - Self.barReach ? $0.minY : floor } ?? floor
                let width = min(Self.maxWidth, max(0, area.width - 2 * Self.margin))
                let palette = EditorPalette(source: store.state["palette"])
                VStack(spacing: Self.margin) {
                    if !presence.notice.isNull { notice(palette, width: width) }
                    if let preview {
                        CanvasPreviewPanel(store: store, view: preview.view, operations: preview.operations, palette: palette)
                            .frame(width: min(Self.previewWidth, width))
                    }
                }.frame(width: width, height: max(0, bottom - Self.margin), alignment: .bottom)
                    .offset(x: area.midX - width / 2)
            }
        }.onChange(of: preview == nil) { _, closed in if !closed { previewBar = bar.bounds } }
            .onChange(of: bar.bounds) { _, bounds in if let bounds { previewBar = bounds } }
    }
    private func notice(_ palette: EditorPalette, width: CGFloat) -> some View {
        HStack(spacing: 12) {
            Text(presence.notice["text"].string).fixedSize(horizontal: false, vertical: true)
                .allowsHitTesting(false).accessibilityIdentifier("canvas-notice-text")
            if !presence.notice["action"].isNull {
                Button(presence.notice["action"]["label"].string) { presence.accept() }
                    .focusable(false).accessibilityIdentifier("canvas-notice-action")
            }
        }.padding(.vertical, 8).padding(.horizontal, 14)
            .foregroundStyle(palette["text"])
            .background {
                SquircleShape.surface.fill(palette["panel"]).shadow(color: .black.opacity(0.27), radius: 4, y: 2)
                    .allowsHitTesting(false)
            }
            .frame(maxWidth: width)
            .accessibilityElement(children: .contain).accessibilityIdentifier("canvas-notice")
    }
}
