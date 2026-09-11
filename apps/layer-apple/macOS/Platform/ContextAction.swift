import SwiftUI
import AppKit

extension View {
    func editorContextAction(_ action: @escaping () -> Void) -> some View {
        overlay(ContextClick(action: action))
    }
}
private struct ContextClick: NSViewRepresentable {
    let action: () -> Void
    func makeNSView(context: Context) -> ContextClickView { ContextClickView() }
    func updateNSView(_ view: ContextClickView, context: Context) { view.action = action }
}
private final class ContextClickView: NSView {
    var action: () -> Void = {}
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? {
        guard NSApp.currentEvent?.type == .rightMouseDown else { return nil }
        return super.hitTest(point)
    }
    override func rightMouseDown(with event: NSEvent) { action() }
}
