import AppKit
import SwiftUI

@main final class WorkspaceNativeInputChecks: NativeWorkspaceInputFixture {
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !ready() {
            try require(Date() < deadline, "Timed out: \(label)")
            try await drain(0.01)
        }
    }
    @MainActor static func run(_ platform: UInt32) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("Initial models") { !store.state.isNull }
        store.native?.resize(width: 1200, height: 870, scale: 1)
        let window = NSWindow(contentRect: CGRect(x: 60, y: 80, width: 1200, height: 870),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Workspace native input check"; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        defer { window.contentView = nil; window.close() }
        let host = NSHostingView(rootView: WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 870).coordinateSpace(name: "editor-workspace")
            .onPreferenceChange(WorkspaceTabFrames.self) { store.workspace.tabFrames = $0 }
            .onPreferenceChange(WorkspaceTabs.self) { frames in
                store.workspace.tabs = frames.compactMap { key, bounds in
                    let ids = key.split(separator: ":").compactMap { UInt64($0) }
                    return ids.count == 2 ? JSON(["group": ids[0], "index": ids[1], "bounds": JSON(bounds).raw]) : nil
                }
            })
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await drain(0.3)
        guard let marker = find(host), let input = marker.model as? WorkspaceReorderInteraction else {
            throw HostFailure(message: "The real workspace native input root must be mounted")
        }
        let workspace = store.workspace
        let tile = store.panel("toolbar")["tiles"].array.first { $0["control"]["command"].string == "eraser" }!
        let item = JSON(["kind": "tile", "panel": "toolbar", "tile": tile["id"].raw])
        try await wait("Toolbar native source") { workspace.sources[item.stableKey] != nil }
        let hold = try holdDuration(marker)
        var number = 1
        func send(_ type: NSEvent.EventType, _ point: CGPoint, pen: Bool = false) async throws {
            number += 1; try event(type, at: point, marker: marker, number: number, tablet: pen)
            try await drain(type == .leftMouseDown ? 0.02 : 0.08)
        }
        func center(_ item: JSON) -> CGPoint {
            let bounds = workspace.sources[item.stableKey]!.bounds
            return CGPoint(x: bounds.midX, y: bounds.midY)
        }
        func action(_ value: [String: Any]) async throws {
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                store.edit(value) { error in
                    if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
                }
            }
            try await drain()
        }
        var start = center(item)
        try await send(.leftMouseDown, start); try await send(.leftMouseUp, start)
        try await wait("Ordinary tile action") { store.panel("toolbar")["tiles"].array.first { $0["id"].uint == tile["id"].uint }?["selected"].bool == true }
        try await action(["type": "invoke", "command": "brush"])
        for pen in [false, true] {
            start = center(item)
            let before = store.state["workspace"].stableKey
            let neighbor = store.panel("toolbar")["tiles"].array.first {
                $0["id"].uint != tile["id"].uint && workspace.sources[JSON(["kind": "tile", "panel": "toolbar", "tile": $0["id"].raw]).stableKey] != nil
            }!
            let end = center(JSON(["kind": "tile", "panel": "toolbar", "tile": neighbor["id"].raw]))
            try await send(.leftMouseDown, start, pen: pen)
            try require(input.contact.device == (pen ? .pen : .mouse) && input.contact.requiresHold,
                "The native tile must classify its actual device and require a hold")
            try await send(.leftMouseDragged, end, pen: pen)
            try require(!input.contact.dragging && input.menu.isNull, "Early tile motion must not drag or open a menu")
            try await send(.leftMouseUp, end, pen: pen)
            try require(store.state["workspace"].stableKey == before, "Early tile motion must preserve history and working state")

            try await send(.leftMouseDown, start, pen: pen); try await drain(hold)
            try require(input.contact.held && !input.contact.dragging, "A native stationary hold must only arm the tile")
            try require(input.menu.isNull != pen, "Only the pen hold may open the tile menu")
            try await send(.leftMouseUp, start, pen: pen)
            try require(store.state["workspace"].stableKey == before, "A held release must not activate or reorder the tile")
            try require(input.menu.isNull != pen, "Pen release must retain the menu; mouse holds have none")
            if pen { try key("\u{1b}", code: 53, window: window); try await drain(); try require(input.menu.isNull, "Escape must close the native tile menu") }

            try await send(.leftMouseDown, start, pen: pen); try await drain(hold)
            try await send(.leftMouseDragged, end, pen: pen)
            try require(input.contact.dragging && input.menu.isNull, "The original held contact must close its menu and start dragging")
            try await wait("Shared tile insertion hint") { !workspace.dropHint["action"].isNull }
            try await send(.leftMouseUp, end, pen: pen)
            try await wait("Shared tile drop") { store.state["workspace"].stableKey != before }
            let after = store.state["workspace"].stableKey
            try await action(["type": "invoke", "command": "undo_workspace"])
            try require(store.state["workspace"].stableKey == before, "One Undo must restore the entire tile reorder")
            try await action(["type": "invoke", "command": "redo_workspace"])
            try require(store.state["workspace"].stableKey == after, "One Redo must restore the same tile reorder")
            try await action(["type": "invoke", "command": "undo_workspace"])
            note("PASS platform \(platform), \(pen ? "pen" : "mouse"): native tile gating, held menus/release, same-contact reorder and one-step Undo/Redo")
        }
        // The ribbon grip carries a panel payload but must remain immediate.
        let handle = JSON(["kind": "panel", "panel": "toolbar"])
        start = center(handle)
        let before = store.state["workspace"].stableKey
        try await send(.leftMouseDown, start, pen: true)
        try require(!input.contact.requiresHold, "The panel grip must remain distinct from a panel icon tile")
        try await send(.leftMouseDragged, CGPoint(x: 610, y: 380), pen: true)
        try require(input.contact.dragging, "The native pen grip must start without a hold")
        window.orderOut(nil); try await drain()
        try await wait("Cancelled panel movement") { store.state["workspace"].stableKey == before && input.contact.target == nil }
        window.makeKeyAndOrderFront(nil); try await drain()
        try await send(.leftMouseUp, start, pen: true)
        try require(store.state["workspace"].stableKey == before, "Focus loss and late pen-up must not commit the panel move")
        try require(store.failure == nil, store.failure ?? "")
        note("PASS platform \(platform): native immediate pen grip, capture across tear-off and focus-loss rollback")

        try await action(["type": "move_panel", "panel": "toolbar", "target": ["kind": "tab", "group": 6], "viewport": [1200, 870]])
        try await action(["type": "customize", "action": ["type": "set_column_collapsed", "group": 6, "collapsed": true]])
        func source(_ surface: ReorderSurface) -> (key: String, value: WorkspaceSource)? {
            workspace.sourceInstances.first { $0.value.item == handle.stableKey && $0.value.surface == surface && !$0.value.bounds.isEmpty }
        }
        try await wait("Collapsed toolbar icon") { source(.tile) != nil }
        let icon = source(.tile)!.value.bounds
        start = CGPoint(x: icon.midX, y: icon.midY)
        try await send(.leftMouseDown, start, pen: true); try await send(.leftMouseUp, start, pen: true)
        try await wait("Open drawer tab and retained column icon") { source(.handle) != nil && source(.tile) != nil }
        let iconID = source(.tile)!.key, tabID = source(.handle)!.key
        try require(iconID != tabID && workspace.hitSource(at: start)?.0 == iconID,
            "The column icon must keep its own hit target beside the open drawer tab for the same panel")
        let collapsed = store.state["workspace"].stableKey
        try await send(.leftMouseDown, start, pen: true)
        try require(input.contact.requiresHold, "A collapsed panel icon remains a tile, even while its drawer is open")
        try await send(.leftMouseDragged, CGPoint(x: 630, y: 390), pen: true)
        try require(!input.contact.dragging, "Early icon motion must not tear off a panel")
        try await send(.leftMouseUp, CGPoint(x: 630, y: 390), pen: true)
        try require(store.state["workspace"].stableKey == collapsed, "Early icon movement must not change layout history")

        try await send(.leftMouseDown, start, pen: true); try await drain(hold)
        try require(!input.menu.isNull && input.contact.held, "A native held column icon must open its panel menu")
        try await send(.leftMouseDragged, CGPoint(x: 630, y: 390), pen: true)
        try require(input.contact.dragging && input.menu.isNull, "The same icon contact must close its menu and tear off")
        try await send(.leftMouseUp, CGPoint(x: 630, y: 390), pen: true)
        try await wait("Collapsed icon drop") { store.state["workspace"].stableKey != collapsed }
        try await action(["type": "invoke", "command": "undo_workspace"])
        try require(store.state["workspace"].stableKey == collapsed, "One Undo must restore the collapsed panel arrangement")
        try await wait("Restored collapsed icon") { source(.tile) != nil }
        note("PASS platform \(platform): distinct icon/tab sources, icon hold gating, retained panel menu and same-contact tear-off/Undo")
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
