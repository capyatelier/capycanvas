import Foundation

/// Actual workspace files and native owners, isolated from artist libraries.
@main struct ColumnStackPersistenceChecks {
    @MainActor static func ready(_ store: EditorStore) async throws {
        let deadline = Date().addingTimeInterval(20)
        while store.workspaces?.ready != true || store.workspaces?.busy == true || store.state.isNull {
            if let error = store.failure ?? store.workspaces?.error { throw HostFailure(message: error) }
            guard Date() < deadline else { throw HostFailure(message: "Column-stack workspace startup timed out") }
            try await Task.sleep(for: .milliseconds(10))
        }
        try await desktopViewport(store)
    }
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-column-persistence-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            var store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            try await ready(store)
            let paint = "builtin:workspace:illustrator"
            try await store.workspaces!.perform(["type":"switch", "id":paint])
            func layout() -> String { store.state["workspace"]["layout"].stableKey }
            func member(_ id: UInt64) -> JSON { store.snapshot["layout"]["collapsed"].array.first { $0["id"].uint == id } ?? JSON() }
            func customize(_ value: [String: Any]) async throws { try await store.apply(["type":"customize", "action":value]) }
            try await customize(["type":"set_column_collapsed", "group":4, "collapsed":true])
            precondition([4, 12].allSatisfy { id in
                store.state["workspace"]["layout"]["collapsed"].array.contains { $0["root"].uint == UInt64(id) }
            })
            try await customize(["type":"set_column_drawers", "column":4, "drawers":false])
            let beforeStack = layout()
            try await store.apply(["type":"move_column", "column":12,
                "target":["kind":"stack_column", "column":4, "before":false], "viewport":[1200,900]])
            let stacked = layout()
            precondition(stacked != beforeStack)
            try await customize(["type":"set_column_auto_hide", "column":4, "auto_hide":true])
            let preferred = layout()
            precondition(preferred != stacked)
            try await customize(["type":"toggle_column_drawer", "group":16, "panel":"layers"])
            let opened = member(12)["open"], bounds = opened["bounds"].rect
            precondition(bounds.width > 0)
            guard let divider = store.snapshot["layout"]["dividers"].array.first(where: {
                !$0["fixed"].bool && $0["bounds"].rect.height > $0["bounds"].rect.width
                    && $0["bounds"].rect.minY < bounds.maxY && $0["bounds"].rect.maxY > bounds.minY
                    && (opened["direction"].string == "right"
                        ? abs($0["bounds"].rect.minX - bounds.maxX)
                        : abs(bounds.minX - $0["bounds"].rect.maxX)) < 0.5
            }) else { throw HostFailure(message: "The open column must expose its resize divider") }
            try await store.apply(["type":"nudge_divider", "id":divider["id"].raw,
                "forward":true, "viewport":store.snapshot["layout"]["viewport"].raw])
            let resized = layout()
            precondition(resized != preferred)
            try await store.apply(["type":"set_brush_size", "value":73])
            let brush = store.state["brush"].stableKey
            try await store.workspaces!.flushed()
            let other = store.workspaces!.view["order"].array.first { $0.string != paint }!.string
            try await store.workspaces!.perform(["type":"switch", "id":other])
            try await store.workspaces!.perform(["type":"switch", "id":paint])
            precondition(layout() == resized && store.state["brush"].stableKey == brush)
            precondition(member(12)["open"].isNull, "Saved custom stacks start closed")
            try await customize(["type":"toggle_column_drawer", "group":16, "panel":"layers"])
            precondition(!member(12)["open"].isNull)
            try await store.workspaces!.flushed()
            try await store.workspaces!.closed()
            store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            try await ready(store)
            precondition(store.workspaces!.view["id"].string == paint)
            precondition(layout() == resized && store.state["brush"].stableKey == brush)
            precondition(member(12)["open"].isNull, "Restart does not restore transient opening")
            for expected in [preferred, stacked, beforeStack] {
                try await store.apply(["type":"invoke", "command":"undo_workspace"])
                try await wait("Width, preference and membership each retain one history step", failure: { store.failure ?? store.workspaces?.error }) { layout() == expected }
            }
            for expected in [stacked, preferred, resized] {
                try await store.apply(["type":"invoke", "command":"redo_workspace"])
                try await wait("Redo restores one stored revision", failure: { store.failure ?? store.workspaces?.error }) { layout() == expected }
            }
            precondition(store.state["brush"].stableKey == brush, "Workspace history preserves working brush settings")
            try await store.workspaces!.closed()
            print("PASS platform \(platform): fresh Paint, stack/preference/width persistence, switching, relaunch, transient opening and persisted Undo/Redo")
        }
    }
}
