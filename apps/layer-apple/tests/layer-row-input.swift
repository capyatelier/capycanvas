// Actual app-local AppKit contacts against the shared LayerPanel and Rust owner.
// No system menu actions, AX traversal, desktop coordinates or artist storage.
import AppKit
import SwiftUI

@main final class LayerRowInputChecks: NativeWorkspaceInputFixture {
    @MainActor static func edit(_ store: EditorStore, _ value: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            store.edit(value) { error in
                if let error { continuation.resume(throwing: HostFailure(message: error)) }
                else { continuation.resume() }
            }
        }
    }
    @MainActor static func wait(_ label: String, until ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(10)
        while !ready() { try require(Date() < deadline, label); try await drain(0.01) }
    }
    @MainActor static func order(_ store: EditorStore) -> [UInt64] { store.state["layers"].array.map { $0["id"].uint } }
    @MainActor static func run(_ platform: UInt32) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("Layer startup timed out") { order(store).count == 2 }
        let original = order(store)[0]
        try await edit(store, ["type": "layer", "action": ["op": "new", "group": false, "clipped": false]])
        let initial = order(store), added = initial[0]
        let window = NSWindow(contentRect: CGRect(x: 160, y: 160, width: 380, height: 260),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Layer row input check"; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        let panel = JSON(["controls": [["control": "layers", "visible_in_panel": true]]])
        let host = NSHostingView(rootView: LayerPanel(store: store, panel: panel))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { window.contentView = nil; window.close() }
        try await drain(0.3); host.layoutSubtreeIfNeeded()
        guard let marker = find(host), let model = marker.model as? LayerRowInteraction else {
            throw HostFailure(message: "The native layer list marker was not mounted")
        }
        try await wait("Measured layer rows missing") { model.frames.count == 3 && model.enabled }
        func body(_ id: UInt64) -> CGPoint { let row = model.frames[id]!.name; return CGPoint(x: row.midX, y: row.midY) }
        let hold = try holdDuration(marker)
        var start = body(original), end = CGPoint(x: start.x, y: model.frames[added]!.row.minY + 2)
        // A floating panel can cover measured rows without unmounting them.
        // Window-root recognizers must respect the actual view under the contact.
        let blocker = NSView(frame: host.convert(model.frames[original]!.row, from: marker))
        host.addSubview(blocker)
        try event(.leftMouseDown, at: start, marker: marker, number: 1001); try await drain(0.02)
        let admittedCoveredRow = model.contact.target != nil
        try event(.leftMouseUp, at: start, marker: marker, number: 1002); try await drain()
        blocker.removeFromSuperview()
        try require(!admittedCoveredRow, "A covered layer row must not take another view's contact")
        try event(.leftMouseDown, at: start, marker: marker, number: 1, tablet: true); try await drain(0.02)
        try event(.leftMouseDragged, at: end, marker: marker, number: 2, tablet: true); try await drain(0.04)
        try event(.leftMouseUp, at: end, marker: marker, number: 3, tablet: true); try await drain()
        try require(order(store) == initial && model.drag == nil, "Pen body motion cannot reorder before a hold")
        try event(.leftMouseDown, at: start, marker: marker, number: 4, tablet: true); try await drain(hold)
        try await wait("The held pen row did not retain its context menu") { !model.menu.isNull }
        try require(model.contact.held && model.contact.device == .pen,
            "The menu must retain the pen contact: key=\(window.isKeyWindow), held=\(model.contact.held), target=\(String(describing: model.contact.target?.id))")
        try event(.leftMouseDragged, at: end, marker: marker, number: 5, tablet: true); try await drain()
        try require(model.contact.dragging && model.menu.isNull, "The same held contact must close the menu and drag the row")
        try event(.leftMouseUp, at: end, marker: marker, number: 6, tablet: true)
        try await wait("Pen body drop did not publish shared order") { order(store)[0] == original }
        let moved = order(store)
        try await edit(store, ["type": "invoke", "command": "undo"])
        try require(order(store) == initial, "One Undo must restore the completed row move")
        try await edit(store, ["type": "invoke", "command": "redo"])
        try require(order(store) == moved, "Redo must restore the same row move")
        try await drain()
        start = body(added); end = CGPoint(x: start.x, y: model.frames[original]!.row.minY + 2)
        try event(.leftMouseDown, at: start, marker: marker, number: 7); try await drain(0.02)
        try event(.leftMouseDragged, at: end, marker: marker, number: 8); try await drain()
        try require(model.contact.dragging && model.menu.isNull, "Mouse layer-row bodies must drag immediately")
        try event(.leftMouseUp, at: end, marker: marker, number: 9)
        try await wait("Mouse body drop did not publish shared order") { order(store) == initial }
        start = CGPoint(x: model.frames[added]!.grip.midX, y: model.frames[added]!.grip.midY)
        end = CGPoint(x: start.x, y: start.y + 25)
        try event(.leftMouseDown, at: start, marker: marker, number: 10, tablet: true); try await drain(0.02)
        try event(.leftMouseDragged, at: end, marker: marker, number: 11, tablet: true); try await drain()
        try require(model.contact.dragging && !model.contact.requiresHold, "The pen grip must drag immediately")
        window.orderOut(nil); try await drain()
        try require(model.contact.target == nil && model.drag == nil && order(store) == initial,
            "Focus loss must cancel a layer grip without committing")
        window.makeKeyAndOrderFront(nil); try await drain()
        let paper = initial.last!, paperPoint = body(paper)
        guard let paperTarget = model.source(at: paperPoint) else {
            throw HostFailure(message: "Paper must retain its context source")
        }
        model.contact.prepare(paperTarget, device: .pen, origin: paperPoint)
        model.contact.recognizeHold(openContext: false)
        try require(!model.contact.move(to: body(added)) && model.drag == nil,
            "Holding Paper must never make the background anchor draggable")
        model.cancel()
        try await edit(store, ["type": "layer", "action": ["op": "begin_rename", "id": added]])
        try require(model.source(at: body(added)) == nil, "Native name editing owns its contact")
        try await edit(store, ["type": "layer", "action": ["op": "cancel_rename"]])
        try await edit(store, ["type": "layer", "action": ["op": "select", "id": original, "mask": false]])
        try await edit(store, ["type": "layer", "action": ["op": "toggle_selection", "id": added]])
        let selected = Set(store.state["layers"].array.filter { $0["selected"].bool }.map { $0["id"].uint })
        model.openMenu(id: original, mask: false)
        try await wait("Shared context menu did not load") { !model.menu.isNull }
        try require(Set(store.state["layers"].array.filter { $0["selected"].bool }.map { $0["id"].uint }) == selected
            && store.state["layer_tools"]["editing_layer"]["id"].uint == original,
            "Opening a checked row's menu preserves multiselection and sets the drawing target")
        model.closeMenu()
        try await edit(store, ["type": "layer", "action": ["op": "toggle_selection", "id": added]])
        try await edit(store, ["type": "layer", "action": ["op": "delete_selected"]])
        model.openMenu(id: original, mask: false)
        try require(model.menu.isNull && model.menuSource == nil, "A removed row cannot open a menu")
        try require(store.failure == nil, store.failure ?? "")
        note("PASS platform \(platform): actual covered-row rejection, layer pen hold/menu/drag, immediate mouse body and pen grip, drop/Undo/Redo/focus; shared Paper, rename, multiselection and removed-menu policies")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { for platform: UInt32 in [0, 1] { try await run(platform) }; exit(0) }
            catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
