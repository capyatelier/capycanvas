import AppKit
import SwiftUI

/// Real native capture with the shared dock views and owner. Temporary windows
/// and stores keep these mouse/tablet events separate from artist documents.
@main final class ColumnStackInputChecks: NativeWorkspaceInputFixture {
    @MainActor static func run(_ platform: UInt32) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("Initial models") { !store.state.isNull }
        store.native?.resize(width: 1200, height: 870, scale: 1)
        func action(_ value: [String: Any]) async throws {
            try await store.apply(value)
            try await drain()
        }
        func edit(_ value: [String: Any]) async throws { try await action(["type":"customize", "action":value]) }
        for group in [6, 16] { try await edit(["type":"set_column_collapsed", "group":group, "collapsed":true]) }
        for column in [4, 12] { try await edit(["type":"set_column_drawers", "column":column, "drawers":false]) }
        let baseline = store.state["workspace"]
        let window = NSWindow(contentRect: CGRect(x: 60, y: 80, width: 1200, height: 870),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Column stack input check"; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        defer { window.contentView = nil; window.close() }
        let host = NSHostingView(rootView: WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 870).coordinateSpace(name: "editor-workspace")
            .modifier(EditorPopoverHost()).environment(\.editorPopupStore, store)
            .onPreferenceChange(WorkspaceTabFrames.self) { store.workspace.tabFrames = $0 }
            .onPreferenceChange(WorkspaceTabs.self) { frames in
                store.workspace.tabs = frames.compactMap { key, bounds in
                    let ids = key.split(separator: ":").compactMap { UInt64($0) }
                    return ids.count == 2 ? JSON(["group":ids[0], "index":ids[1], "bounds":JSON(bounds).raw]) : nil
                }
            })
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await drain(0.3)
        guard let marker = find(host), let input = marker.model as? WorkspaceReorderInteraction else {
            throw HostFailure(message: "The native workspace input root must be mounted")
        }
        let workspace = store.workspace
        func member(_ id: UInt64) -> JSON { store.snapshot["layout"]["collapsed"].array.first { $0["id"].uint == id } ?? JSON() }
        func grip(_ id: UInt64) -> JSON { JSON(["kind":"column", "column":id]) }
        func source(_ item: JSON, surface: ReorderSurface = .handle) -> WorkspaceSource? {
            workspace.sourceInstances.values.first { $0.item == item.stableKey && $0.surface == surface }
        }
        func center(_ value: WorkspaceSource) -> CGPoint { CGPoint(x: value.bounds.midX, y: value.bounds.midY) }
        func layout() -> String { store.state["workspace"]["layout"].stableKey }
        var counter = 1
        func send(_ type: NSEvent.EventType, _ point: CGPoint, pen: Bool = false) async throws {
            counter += 1
            try event(type, at: point, marker: marker, number: platform == 0 ? counter : 0, tablet: pen)
            try await drain(type == .leftMouseDown ? 0.02 : 0.08)
        }
        func click(_ panel: String) async throws {
            let item = JSON(["kind":"panel", "panel":panel])
            try await wait("Collapsed icon \(panel)") { source(item, surface: .tile) != nil }
            let point = center(source(item, surface: .tile)!)
            try await send(.leftMouseDown, point); try await send(.leftMouseUp, point)
        }
        for pen in [false, true] {
            try await action(["type":"restore_workspace", "workspace":baseline.raw])
            try await wait("Both member grips") { source(grip(4)) != nil && source(grip(12)) != nil }
            let before = layout()
            let start = center(source(grip(12))!), end = center(source(grip(4))!)
            try await send(.leftMouseDown, start, pen: pen)
            try require(!input.contact.requiresHold && input.contact.device == (pen ? .pen : .mouse), "Column grips classify the actual device and remain immediate")
            try await send(.leftMouseDragged, end, pen: pen)
            try await wait("Stack member insertion") { workspace.dropHint["target"]["kind"].string == "stack_column" }
            try await send(.leftMouseUp, end, pen: pen)
            try await wait("Member stack committed") { member(4)["stack"].uint == member(12)["stack"].uint && layout() != before }
            let stacked = layout()
            try await action(["type":"invoke", "command":"undo_workspace"])
            try require(layout() == before, "Stacking has one undo step")
            try await action(["type":"invoke", "command":"redo_workspace"])
            try require(layout() == stacked, "Redo restores the same member boundaries")
            for divider in store.snapshot["layout"]["dividers"].array where divider["fixed"].bool {
                try require(source(JSON(["type":"drag_divider", "id":divider["id"].raw])) == nil,
                    "Closed stack widths have no native resize source")
            }
            try await click("brushes")
            try await wait("Ordinary left column") { !member(4)["open"].isNull }
            try require(store.state["customization"]["column_drawers"].array.isEmpty, "Opening a complete member must not build a second drawer model")
            try require(member(4)["open"]["connections"].array.count == member(4)["groups"].array.count, "Every visible active group has its connector")
            try await click("layers")
            try await wait("Member switch") { member(4)["open"].isNull && !member(12)["open"].isNull }
            let opened = member(12)["open"]["bounds"].rect
            let divider = store.snapshot["layout"]["dividers"].array.first {
                !$0["fixed"].bool && $0["bounds"].rect.height > $0["bounds"].rect.width
                    && abs($0["bounds"].rect.minX - opened.maxX - 6) < 0.5
            }!
            let resize = JSON(["type":"drag_divider", "id":divider["id"].raw])
            try await wait("Open member resize handle") { source(resize) != nil }
            let resizeStart = center(source(resize)!), resizeEnd = CGPoint(x: resizeStart.x + 40, y: resizeStart.y)
            let beforeResize = layout()
            try await send(.leftMouseDown, resizeStart, pen: pen)
            try await send(.leftMouseDragged, resizeEnd, pen: pen)
            try await send(.leftMouseUp, resizeEnd, pen: pen)
            try await wait("Open member width changed") { layout() != beforeResize }
            let resized = layout()
            try await action(["type":"invoke", "command":"undo_workspace"])
            try require(layout() == beforeResize, "Open-member resizing has one undo step")
            try await action(["type":"invoke", "command":"redo_workspace"])
            try require(layout() == resized, "Redo restores the member width")
            let cancelStart = center(source(resize)!)
            try await send(.leftMouseDown, cancelStart, pen: pen)
            try await send(.leftMouseDragged, CGPoint(x: cancelStart.x + 30, y: cancelStart.y), pen: pen)
            window.orderOut(nil)
            try await wait("Focus-loss resize cancellation") { input.contact.target == nil && layout() == resized }
            window.makeKeyAndOrderFront(nil); try await drain()
            try await send(.leftMouseUp, cancelStart, pen: pen)
            try require(layout() == resized, "Late release cannot commit a canceled resize")
            try await click("layers")
            try await wait("Selected icon closes its member") { member(12)["open"].isNull }
            try require(layout() == resized, "Opening and closing are transient")
            note("PASS platform \(platform), \(pen ? "pen" : "mouse"): immediate stacking, selected-member opening/switching, fixed closed width, resize history and focus cancellation")

            let arranged = store.state["workspace"]
            let layersGroup = member(12)["groups"].array.first {
                $0["icons"].array.contains { $0["panel"].string == "layers" }
            }!["group"].uint
            for (label, item, surface, openedSource, appendGroup) in [
                ("held icon", JSON(["kind":"panel", "panel":"properties"]), ReorderSurface.tile, false, false),
                ("panel tab", JSON(["kind":"panel", "panel":"layers"]), .handle, true, false),
                ("whole group", JSON(["kind":"group", "group":layersGroup]), .handle, true, false),
                ("toolbar grip", JSON(["kind":"panel", "panel":"toolbar"]), .handle, false, false),
                ("trailing group target", JSON(["kind":"panel", "panel":"layers"]), .tile, false, true),
            ] {
                try await action(["type":"restore_workspace", "workspace":arranged.raw])
                if openedSource { try await click("layers") }
                try await wait("\(label) source") { source(item, surface: surface) != nil }
                let beforeDrop = layout(), from = center(source(item, surface: surface)!)
                let last = member(4)["groups"].array.last!["bounds"].rect
                let destination = appendGroup ? CGPoint(x: last.midX, y: last.maxY + 2) : center(source(grip(4))!)
                try await send(.leftMouseDown, from, pen: pen)
                try require(input.contact.requiresHold == (surface == .tile), "\(label) must retain its visible surface policy")
                if surface == .tile { try await drain(try holdDuration(marker)) }
                try await send(.leftMouseDragged, destination, pen: pen)
                try await wait("\(label) insertion target") {
                    workspace.dropHint["target"]["kind"].string == (appendGroup ? "split" : "stack_column")
                }
                try require(input.contact.dragging && input.menu.isNull, "Dragging retains the contact and closes its held menu")
                try await send(.leftMouseUp, destination, pen: pen)
                try await wait("\(label) committed") { layout() != beforeDrop }
                let afterDrop = layout()
                let stack = store.state["workspace"]["layout"]["column_stacks"].array.first {
                    $0["members"].array.contains { $0.uint == 4 }
                }!
                try require(stack["members"].array.count == (appendGroup ? 2 : 3), "The target distinguishes a group from a new stack member")
                try await action(["type":"invoke", "command":"undo_workspace"])
                try require(layout() == beforeDrop, "\(label) drop has one undo step")
                try await action(["type":"invoke", "command":"redo_workspace"])
                try require(layout() == afterDrop, "\(label) redo restores the same grouping")
            }
            note("PASS platform \(platform), \(pen ? "pen" : "mouse"): held icons, panel tabs, group/toolbar grips, member insertion and trailing group targets with exact history")
        }
        try require(store.failure == nil, store.failure ?? "")
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
