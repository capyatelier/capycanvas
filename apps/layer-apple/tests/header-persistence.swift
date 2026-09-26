import Foundation

/// Real native owners and workspace files, confined to one temporary library.
@main struct HeaderPersistenceChecks {
    @MainActor static func ready(_ store: EditorStore) async throws {
        let deadline = Date().addingTimeInterval(20)
        while store.workspaceLibrary?.ready != true || store.workspaceLibrary?.busy == true || store.state.isNull {
            if let error = store.failure ?? store.workspaceLibrary?.error { throw HostFailure(message: error) }
            guard Date() < deadline else { throw HostFailure(message: "Title-bar workspace startup timed out") }
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func edit(_ store: EditorStore, _ value: [String: Any]) async throws {
        try await store.apply(["type":"customize", "action":["type":"header", "action":value]])
    }
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-header-persistence-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            var store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            try await ready(store)
            let choices = store.workspaceLibrary!.status["default_workspaces"].array
            for workspace in choices {
                let id = workspace["id"].string
                try await store.workspaceManager.run(JSON(["type":"switch", "value":id]))
                let original = store.state["workspace"]["layout"].stableKey
                let brush = store.state["brush"].stableKey
                try await store.apply(["type":"invoke", "command":"customize_workspace_ui"])
                try await edit(store, ["type":"set_size", "size":"large"])
                try await edit(store, ["type":"canvas_info", "visible":!store.state["workspace"]["layout"]["canvas_info"]["visible"].bool])
                try await edit(store, ["type":"add", "zone":"center", "before":NSNull(), "item":["kind":"space"]])
                let accepted = store.state["workspace"]["layout"].stableKey
                precondition(accepted != original)
                try await edit(store, ["type":"edit", "editing":false])
                try await store.workspaceLibrary!.flush()
                let other = choices.first { $0["id"].string != id }!["id"].string
                try await store.workspaceManager.run(JSON(["type":"switch", "value":other]))
                try await store.workspaceManager.run(JSON(["type":"switch", "value":id]))
                precondition(store.state["workspace"]["layout"].stableKey == accepted)
                precondition(store.state["brush"].stableKey == brush, "Header editing must preserve the working tool")
                try await store.apply(["type":"invoke", "command":"customize_workspace_ui"])
                let capy = store.snapshot["header"]["model"]["zones"].array.flatMap(\.array).first { $0["item"]["kind"].string == "capy" }!
                try await edit(store, ["type":"remove", "id":capy["id"].raw])
                precondition(store.state["workspace"]["layout"].stableKey != accepted)
                // Closing must persist the accepted workspace, not live preview.
                try await store.workspaceLibrary!.flush()
                try await store.workspaceLibrary!.close()
                store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
                try await ready(store)
                precondition(store.workspaceLibrary!.status["active_id"].string == id)
                precondition(!store.snapshot["header"]["editing"].bool)
                precondition(store.state["workspace"]["layout"].stableKey == accepted, "Restart discards unfinished customization")
                try await store.apply(["type":"invoke", "command":"undo_workspace"])
                precondition(store.state["workspace"]["layout"].stableKey == original, "The accepted edit is one persisted history step")
                try await store.apply(["type":"invoke", "command":"redo_workspace"])
                precondition(store.state["workspace"]["layout"].stableKey == accepted)
                print("PASS platform \(platform), \(workspace["name"].string): Done, switch, unfinished-preview close/restart and persisted Undo/Redo")
            }
            try await store.workspaceLibrary!.close()
        }
    }
}
