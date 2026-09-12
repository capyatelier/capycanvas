// Exercise real SwiftUI geometry preferences and the serial Rust owner without
// XCTest, a visible editor, GPU rendering or system menu automation.
import AppKit
import SwiftUI

@main struct WorkspaceDrawerChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let store = EditorStore(platform: 1, persistence: EditorPersistence(root: nil))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 870),
            styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func wait(_ message: String, _ condition: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(10)
            while !condition() {
                window.contentView?.layoutSubtreeIfNeeded()
                guard Date() < deadline else {
                    print("Drawer state:", store.state["customization"]["column_drawers"].stableKey)
                    print("Tabs:", JSON(store.workspace.tabs.map(\.raw)).stableKey)
                    throw NSError(domain: message, code: 1)
                }
                try await Task.sleep(for: .milliseconds(5))
            }
            precondition(store.failure == nil, store.failure ?? "")
        }
        try await wait("Initial owner state") { !store.state.isNull }
        store.native?.resize(width: 1200, height: 870, scale: 1)
        store.dispatch(["type": "move_panel", "panel": "toolbar", "target": ["kind": "tab", "group": 6], "viewport": [1200,870]])
        store.customize(["type": "set_column_collapsed", "group": 6, "collapsed": true])
        let root = WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 870).coordinateSpace(name: "editor-workspace")
            .font(.system(size: 44 / 3))
            .onPreferenceChange(WorkspaceTabs.self) { bounds in
                store.workspace.tabs = bounds.compactMap { key, rect in
                    let parts = key.split(separator: ":").compactMap { UInt64($0) }
                    guard parts.count == 2 else { return nil }
                    return JSON(["group": parts[0], "index": parts[1], "bounds": JSON(rect).raw])
                }
            }
        window.contentView = NSHostingView(rootView: root)
        store.customize(["type": "toggle_column_drawer", "group": 6, "panel": "toolbar"])
        let item = JSON(["kind": "panel", "panel": "toolbar"])
        try await wait("Drawer sources and tab measurements") {
            store.workspace.sources[item.stableKey] != nil && store.workspace.tabs.filter { $0["group"].uint == 6 }.count == 2
        }
        let bounds = store.workspace.sources[item.stableKey]!.bounds
        let start = CGPoint(x: bounds.midX, y: bounds.midY)
        precondition(store.workspace.source(at: start)?.stableKey == item.stableKey)
        let groupItem = JSON(["kind": "group", "group": 6])
        try await wait("Drawer header animation and native measurements agree") {
            guard let measured = store.workspace.sources[groupItem.stableKey]?.bounds,
                let drawer = store.contentDrawers.items["4"] else { return false }
            let target = drawer.geometry["placement"]["bounds"].rect
            return target.width == 272 && abs(measured.minY - target.minY) < 0.5 && abs(measured.width - target.width) < 0.5
        }
        let drawerBounds = store.contentDrawers.items["4"]!.geometry["placement"]["bounds"].rect
        let emptyHeader = CGPoint(x: drawerBounds.maxX - 30, y: drawerBounds.minY + 18)
        precondition(store.workspace.source(at: emptyHeader)?.stableKey == groupItem.stableKey,
            "Unused drawer header space must drag the whole group")
        let first = store.workspace.tabs.first { $0["group"].uint == 6 && $0["index"].uint == 0 }!["bounds"].rect
        let end = CGPoint(x: first.minX + 12, y: first.midY)
        store.workspace.start(item, point: start)
        store.workspace.move(item, point: end)
        store.workspace.move(item, point: end, released: true)
        try await wait("Native drawer tab reorder") {
            store.state["customization"]["column_drawers"][0]["tabs"]["panels"][0].string == "toolbar"
        }
        try await wait("Reordered SwiftUI geometry") {
            guard let tools = store.workspace.sources[item.stableKey]?.bounds,
                let brushes = store.workspace.sources[JSON(["kind": "panel", "panel": "brushes"]).stableKey]?.bounds else { return false }
            return tools.midX < brushes.midX
        }
        // Coalesced close/reopen can preserve the exact SwiftUI rectangles while
        // invalidating Rust's transient drawer bounds. Starting a new drag must
        // republish those bounds before its down event.
        store.customize(["type": "toggle_column_drawer", "group": 6, "panel": "toolbar"])
        store.customize(["type": "toggle_column_drawer", "group": 6, "panel": "toolbar"])
        let _: JSON = await withCheckedContinuation { c in store.query(["type": "catalog"]) { c.resume(returning: $0) } }
        // Make the transient invalidation deterministic even if SwiftUI already
        // delivered another preference pass during the owner barrier.
        store.dispatch(["type": "measure_column_drawers", "measurements": []])
        let reordered = store.workspace.sources[item.stableKey]!.bounds
        let restart = CGPoint(x: reordered.midX, y: reordered.midY)
        store.workspace.start(item, point: restart)
        let append = CGPoint(x: drawerBounds.maxX - 10, y: reordered.midY)
        let hint: JSON = await withCheckedContinuation { c in
            store.query(["type": "drop", "item": item.raw, "position": [append.x, append.y],
                "tabs": store.workspace.tabs.map(\.raw)]) { c.resume(returning: $0) }
        }
        precondition(hint["target"]["kind"].string == "tab" && hint["target"]["group"].uint == 6,
            "A drag must restore the drawer's measured drop target after transient invalidation")
        store.workspace.cancel(item)
        let _: JSON = await withCheckedContinuation { c in store.query(["type": "catalog"]) { c.resume(returning: $0) } }
        precondition(store.failure == nil, store.failure ?? "")
        store.customize(["type": "toggle_column_drawer", "group": 6, "panel": "toolbar"])
        try await wait("Closing drawer stops intercepting input") {
            store.contentDrawers.items["4"]?.interactive != true
                && store.workspace.sources[item.stableKey] == nil
        }
        print("PASS: SwiftUI drawer geometry, tab/header drag sources, reorder, restored drop targets and closing hit-test retirement")
    }
}
