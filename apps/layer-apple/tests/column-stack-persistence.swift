import Foundation

/// Actual workspace files and native owners, isolated from artist libraries.
@main struct ColumnStackPersistenceChecks {
    @MainActor static func ready(_ store: EditorStore) async throws {
        let deadline = Date().addingTimeInterval(20)
        while store.workspaceLibrary?.ready != true || store.workspaceLibrary?.busy == true || store.state.isNull {
            if let error = store.failure ?? store.workspaceLibrary?.error { throw HostFailure(message: error) }
            guard Date() < deadline else { throw HostFailure(message: "Column-stack workspace startup timed out") }
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func action(_ store: EditorStore, _ value: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            store.edit(value) { error in
                if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
            }
        }
    }
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-column-persistence-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            var store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            try await ready(store)
            store.native?.resize(width: 1200, height: 900, scale: 1)
            let paint = "builtin:workspace:illustrator"
            try await store.workspaceManager.run(JSON(["type":"switch", "value":paint]))
            func layout() -> String { store.state["workspace"]["layout"].stableKey }
            func member(_ id: UInt64) -> JSON { store.snapshot["layout"]["collapsed"].array.first { $0["id"].uint == id } ?? JSON() }
            func customize(_ value: [String: Any]) async throws { try await action(store, ["type":"customize", "action":value]) }
            try await customize(["type":"set_column_collapsed", "group":16, "collapsed":true])
            try await customize(["type":"set_column_drawers", "column":4, "drawers":false])
            let beforeStack = layout()
            try await action(store, ["type":"move_column", "column":12,
                "target":["kind":"stack_column", "column":4, "before":false], "viewport":[1200,900]])
            let stacked = layout()
            precondition(stacked != beforeStack)
            try await customize(["type":"set_column_auto_hide", "column":4, "auto_hide":true])
            let preferred = layout()
            precondition(preferred != stacked)
            try await customize(["type":"toggle_column_drawer", "group":16, "panel":"layers"])
            let bounds = member(12)["open"]["bounds"].rect
            precondition(bounds.width > 0)
            let divider = store.snapshot["layout"]["dividers"].array.first {
                !$0["fixed"].bool && $0["bounds"].rect.height > $0["bounds"].rect.width
                    && min(abs($0["bounds"].rect.minX - bounds.maxX - 6),
                        abs(bounds.minX - $0["bounds"].rect.maxX - 6)) < 0.5
            }!
            try await action(store, ["type":"nudge_divider", "id":divider["id"].raw,
                "forward":true, "viewport":store.snapshot["layout"]["viewport"].raw])
            let resized = layout()
            precondition(resized != preferred)
            try await action(store, ["type":"set_brush_size", "value":73])
            let brush = store.state["brush"].stableKey
            try await store.workspaceLibrary!.flush()
            let other = store.workspaceLibrary!.status["default_workspaces"].array.first { $0["id"].string != paint }!["id"].string
            try await store.workspaceManager.run(JSON(["type":"switch", "value":other]))
            try await store.workspaceManager.run(JSON(["type":"switch", "value":paint]))
            precondition(layout() == resized && store.state["brush"].stableKey == brush)
            precondition(member(12)["open"].isNull, "Saved custom stacks start closed")
            try await customize(["type":"toggle_column_drawer", "group":16, "panel":"layers"])
            precondition(!member(12)["open"].isNull)
            try await store.workspaceLibrary!.flush()
            try await store.workspaceLibrary!.close()
            store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            try await ready(store)
            precondition(store.workspaceLibrary!.status["active_id"].string == paint)
            precondition(layout() == resized && store.state["brush"].stableKey == brush)
            precondition(member(12)["open"].isNull, "Restart does not restore transient opening")
            for expected in [preferred, stacked, beforeStack] {
                try await action(store, ["type":"invoke", "command":"undo_workspace"])
                precondition(layout() == expected, "Width, preference and membership each retain one history step")
            }
            for expected in [stacked, preferred, resized] {
                try await action(store, ["type":"invoke", "command":"redo_workspace"])
                precondition(layout() == expected)
            }
            precondition(store.state["brush"].stableKey == brush, "Workspace history preserves working brush settings")
            try await store.workspaceLibrary!.close()
            print("PASS platform \(platform): fresh Paint, stack/preference/width persistence, switching, relaunch, transient opening and persisted Undo/Redo")
        }
    }
}
