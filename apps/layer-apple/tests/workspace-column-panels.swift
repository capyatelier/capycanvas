// Actual SwiftUI resize sources and serial Rust actions on both Apple presets.
// Invisible AppKit surfaces establish component geometry and behavior, not
// physical input recognition, UIKit pixels or whole-editor visual parity.
import AppKit
import SwiftUI

@main struct WorkspaceColumnPanelChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        for platform: UInt32 in [0, 1] {
            for panel in ["brushes", "layers"] { try await check(platform, panel: panel) }
        }
    }
    @MainActor static func check(_ platform: UInt32, panel: String) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 1200, height: 870),
            styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func wait(_ label: String, _ ready: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(10)
            while !ready() {
                window.contentView?.layoutSubtreeIfNeeded()
                guard store.failure == nil else { throw HostFailure(message: store.failure!) }
                guard Date() < deadline else { throw HostFailure(message: "Timed out: " + label) }
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        func action(_ value: [String: Any]) async throws {
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                store.edit(value) { error in
                    if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
                }
            }
        }
        func customize(_ value: [String: Any]) async throws { try await action(["type": "customize", "action": value]) }
        func query(_ value: [String: Any]) async -> JSON {
            await withCheckedContinuation { done in store.query(value) { done.resume(returning: $0) } }
        }
        func objects(_ value: JSON) -> [JSON] {
            if !value.object.isEmpty { return [value] + value.object.values.flatMap { objects(JSON($0)) } }
            return value.array.flatMap(objects)
        }
        try await wait("initial owner") { !store.state.isNull }
        store.native?.resize(width: 1200, height: 870, scale: 1)
        try await action(["type": "invoke", "command": "fit_canvas"])
        let group = store.snapshot["layout"]["groups"].array.first {
            $0["panels"].array.contains { $0.string == panel }
        }!["id"].uint
        // Left-side coverage uses an inner column with a neighboring panel.
        if panel == "brushes" {
            try await action(["type": "move_panel", "panel": "properties",
                "target": ["kind": "split", "group": group, "edge": "right"], "viewport": [1200, 870]])
        }
        try await action(["type": "move_panel", "panel": "sizes",
            "target": ["kind": "tab", "group": group], "viewport": [1200, 870]])
        try await customize(["type": "set_column_collapsed", "group": group, "collapsed": true])
        let column = store.snapshot["layout"]["collapsed"].array.first {
            $0["groups"].array.contains { $0["group"].uint == group }
        }!["id"].uint
        let menu = await query(["type": "context", "target": ["kind": "column", "column": column]])
        let commands = objects(menu).filter { $0["action"]["type"].string == "customize" }
        for type in ["set_column_mode", "set_column_auto_hide", "apply_column_settings"] {
            precondition(commands.contains { $0["action"]["action"]["type"].string == type }, "Missing Apple column menu action: \(type)")
        }
        let groupMode = commands.first { $0["action"]["action"]["type"].string == "set_column_mode" && $0["action"]["action"]["mode"].string == "group_panel" }!
        try await action(groupMode["action"].object)
        let root = WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 870).coordinateSpace(name: "editor-workspace")
            .font(.system(size: 44 / 3))
        window.contentView = NSHostingView(rootView: root)
        func open() async throws { try await customize(["type": "toggle_column_drawer", "group": group, "panel": panel]) }
        func projection() -> JSON {
            store.snapshot["layout"]["collapsed"].array.first { $0["id"].uint == column }?["group_panel"] ?? JSON()
        }
        func resize(_ after: JSON) -> JSON { JSON(["type": "resize_column_panel", "column": column, "after": after.raw]) }
        func settled() async throws {
            try await wait("mounted attached panel handles") {
                let p = projection()
                guard !p.isNull, let drawer = store.contentDrawers.items[String(column)], drawer.isGroupPanel,
                    drawer.geometry["placement"]["bounds"].rect == p["bounds"].rect else { return false }
                let items = [resize(JSON())] + p["dividers"].array.indices.map { resize(p["panels"][$0]["panel"]) }
                return items.allSatisfy { store.workspace.sources[$0.stableKey] != nil }
            }
            // Measurements can trigger an intrinsic dock reflow after mounting.
            try await Task.sleep(for: .milliseconds(200))
        }
        try await open(); try await settled()
        precondition(projection()["panels"].array.count >= 2 && projection()["dividers"].array.count >= 1)
        precondition(projection()["direction"].string == (panel == "brushes" ? "right" : "left"))
        for after in [JSON(), projection()["panels"][0]["panel"]] {
            let item = resize(after), initial = store.state["workspace"].stableKey
            let geometry = projection(), expected = after.isNull ? geometry["resize"].rect : geometry["dividers"][0].rect
            let actual = store.workspace.sources[item.stableKey]!.bounds
            precondition(abs(actual.minX - expected.minX) < 0.5 && abs(actual.minY - expected.minY) < 0.5
                && abs(actual.width - expected.width) < 0.5 && abs(actual.height - expected.height) < 0.5,
                "The mounted resize handle must match shared geometry")
            let start = CGPoint(x: actual.midX, y: actual.midY)
            if store.workspace.source(at: start)?.stableKey != item.stableKey {
                print("Expected source", item.stableKey, actual)
                print("Actual source", store.workspace.source(at: start)?.stableKey ?? "nil")
                print("Covering sources", store.workspace.sourceInstances.values.filter { $0.bounds.contains(start) })
                print("Drawer", store.contentDrawers.items[String(column)]!.geometry.stableKey)
                fflush(stdout)
            }
            precondition(store.workspace.source(at: start)?.stableKey == item.stableKey,
                "Attached content must not cover its resize handle")
            let end = CGPoint(x: start.x + (after.isNull ? (panel == "brushes" ? 42 : -42) : 0),
                y: start.y + (after.isNull ? 0 : 42))
            let source = store.workspace.input.source(at: start)!
            store.workspace.input.contact.prepare(source, device: .pen, origin: start)
            precondition(!store.workspace.input.contact.requiresHold, "A resize grip must be immediate for Pencil")
            precondition(store.workspace.input.contact.move(to: end))
            try await wait("live resize publication") { store.state["workspace"].stableKey != initial }
            precondition(store.workspace.input.contact.validate(), "Geometry changes must retain this resize contact")
            store.workspace.input.contact.cancel()
            try await wait("cancel restores layout") { store.state["workspace"].stableKey == initial }
            try await settled()
            let bounds = store.workspace.sources[item.stableKey]!.bounds
            let restart = CGPoint(x: bounds.midX, y: bounds.midY)
            store.workspace.start(item, point: restart)
            store.workspace.move(item, point: CGPoint(x: restart.x + end.x - start.x, y: restart.y + end.y - start.y), released: true)
            // The owner barrier follows release, so this observes its committed
            // publication rather than treating an earlier move as completion.
            _ = await query(["type": "catalog"])
            try await wait("committed resize") { store.state["workspace"].stableKey != initial }
            let committed = store.state["workspace"].stableKey
            try await action(["type": "invoke", "command": "undo_workspace"])
            precondition(store.state["workspace"].stableKey == initial, "One Undo restores the complete resize")
            try await action(["type": "invoke", "command": "redo_workspace"])
            precondition(store.state["workspace"].stableKey == committed, "One Redo restores its exact sizes")
            if projection().isNull { try await open() }
            try await settled()
        }
        let saved = store.state["workspace"]["layout"]["column_settings"].stableKey
        try await customize(["type": "close_column", "column": column])
        try await wait("closed handles retire") { !store.workspace.contains(resize(JSON())) }
        try await open(); try await settled()
        precondition(store.state["workspace"]["layout"]["column_settings"].stableKey == saved,
            "Reopening preserves committed widths and panel heights")
        try await customize(["type": "set_column_auto_hide", "column": column, "auto_hide": true])
        store.workspace.chrome(["kind": "contact", "position": [600, 450], "canvas": true])
        try await wait("outside canvas contact auto-hides attached panel") { projection().isNull }
        try await customize(["type": "set_column_mode", "column": column, "mode": "drawers"])
        try await open()
        try await wait("legacy drawer mode remains available") {
            store.contentDrawers.items[String(column)]?.interactive == true
                && store.contentDrawers.items[String(column)]?.isGroupPanel == false
        }
        precondition(projection().isNull, "A regular drawer must not consume attached workspace width")
        precondition(store.failure == nil, store.failure ?? "")
        print("PASS platform \(platform), \(panel): native column modes/handles, nested placement, immediate resize, cancellation, Undo/Redo, saved sizes and auto-hide")
    }
}
