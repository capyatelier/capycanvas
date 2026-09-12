import Foundation

/// Exercise actual shared menu payloads through the Apple editor and service.
/// No OS menu automation, user storage, renderer, or visible windows are needed.
@main struct WorkspaceMenuActionChecks {
    @MainActor static func wait(_ label: String, until ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !ready() {
            guard Date() < deadline else { throw HostFailure(message: "Timed out: \(label)") }
            try await Task.sleep(for: .milliseconds(5))
        }
    }
    @MainActor static func edit(_ store: EditorStore, _ action: JSON) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            store.edit(action.object) { error in
                if let error { continuation.resume(throwing: HostFailure(message: error)) }
                else { continuation.resume() }
            }
        }
    }
    @MainActor static func query(_ store: EditorStore, _ request: [String: Any]) async -> JSON {
        await withCheckedContinuation { continuation in
            store.query(request) { continuation.resume(returning: $0) }
        }
    }
    @MainActor static func capture(_ store: EditorStore) async throws -> JSON {
        try await withCheckedThrowingContinuation { continuation in
            store.native!.workspaceSession(JSON(["type": "capture"])) { value, error in
                DispatchQueue.main.async {
                    if let error { continuation.resume(throwing: HostFailure(message: error)) }
                    else { continuation.resume(returning: value ?? JSON()) }
                }
            }
        }
    }
    static func objects(_ value: JSON) -> [JSON] {
        if !value.object.isEmpty { return [value] + value.object.values.flatMap { objects(JSON($0)) } }
        return value.array.flatMap(objects)
    }
    static func route(_ action: JSON) -> String? {
        if action["type"].string == "workspace_manager" { return action["command"]["type"].string }
        // The application's Toolbars item uses a command alias; its result
        // must still enter the workspace service and acknowledge the request.
        if action["type"].string == "invoke", action["command"].string == "manage_toolbars" { return "manage_toolbars" }
        return nil
    }
    @MainActor static func menuActions(_ store: EditorStore) async -> [String: [JSON]] {
        var menus = store.snapshot["application_menus"].array
        let layout = store.state["workspace"]["layout"]
        var targets = [JSON(["kind": "zen_mode"])]
        targets += layout["panels"].array.map { JSON(["kind": "panel", "panel": $0["id"].raw]) }
        targets += objects(layout).filter { $0["kind"].string == "tabs" }
            .map { JSON(["kind": "group", "group": $0["id"].raw]) }
        for target in targets {
            menus.append(await query(store, ["type": "context", "target": target.raw]))
        }
        var result: [String: [JSON]] = [:]
        for item in menus.flatMap(objects) where item["enabled"].bool {
            let action = item["action"]
            if let key = route(action) { result[key, default: []].append(action) }
        }
        return result
    }
    @MainActor static func acknowledged(_ store: EditorStore) -> Bool {
        !store.workspaceManager.processing && !store.state["requests"].array.contains {
            $0["kind"]["type"].string == "workspace"
        }
    }
    @MainActor static func main() async throws {
        let promptTitles = ["new": "New Workspace", "reset_brushes": "Reset All Brushes?",
            "reset_layout": "Restore Starting Layout", "save_toolbar": "Save to Toolbar Library", "new_toolbar": "New Toolbar"]
        let routes = ["new", "reset_brushes", "reset_layout", "save_toolbar", "new_toolbar",
            "manage", "manage_toolbars", "layout_history", "switch"]
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-menu-actions-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let editor = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            try await wait("workspace startup") { editor.workspaceLibrary?.ready == true || editor.workspaceLibrary?.error != nil }
            let library = editor.workspaceLibrary!, manager = editor.workspaceManager
            precondition(library.ready, library.error ?? "Startup failed")
            // Give reset/history something to operate on and retain across
            // cancellation. This changes the workspace, not the document.
            try await edit(editor, JSON(["type": "set_brush_size", "value": 31]))
            try await edit(editor, JSON(["type": "customize", "action": ["type": "set_panel_visible", "panel": "sizes", "visible": false]]))
            let initial = try await capture(editor)
            let document = editor.state["layers"].stableKey
            let originalID = library.status["active_id"].string
            let actions = await menuActions(editor)
            precondition(Set(actions.keys) == Set(routes), "Workspace menu service coverage drift: \(actions.keys.sorted())")
            for key in routes {
                let action = actions[key]!.first {
                    key != "switch" || $0["command"]["id"].string != originalID
                }!
                try await edit(editor, action)
                if let title = promptTitles[key] {
                    try await wait("\(key) prompt") { manager.prompt != nil || manager.error != nil }
                    precondition(manager.prompt?["title"].string == title, "Wrong form for \(key): \(manager.prompt?.stableKey ?? manager.error ?? "nil")")
                    manager.answer(confirm: false)
                } else if key == "switch" {
                    try await wait("switch applied") { library.status["active_id"].string == action["command"]["id"].string || manager.error != nil }
                    precondition(!manager.presented && library.status["active_id"].string != originalID)
                } else {
                    try await wait("\(key) view") {
                        manager.presented && (key == "layout_history" ? !manager.history.isNull : !manager.view.isNull) || manager.error != nil
                    }
                    if key == "layout_history" {
                        precondition(library.previewingLayout && !manager.history["rows"].array.isEmpty)
                    } else {
                        precondition(manager.page == (key == "manage" ? "workspaces" : "this_workspace"))
                        precondition(!manager.view["rows"].array.isEmpty)
                    }
                }
                try await wait("\(key) acknowledged") { acknowledged(editor) }
                precondition(manager.error == nil && editor.failure == nil, manager.error ?? editor.failure ?? "")
                manager.presented = false; manager.dismissed()
                try await wait("\(key) preview cleanup") { !library.previewingLayout && !library.busy }
                let after = try await capture(editor)
                precondition(editor.state["layers"].stableKey == document, "\(key) changed document layers")
                if key != "switch" {
                    precondition(library.status["active_id"].string == originalID)
                    precondition(after["capture"].stableKey == initial["capture"].stableKey, "\(key) changed the workspace on cancellation/dismissal")
                }
            }
            // Return using the newly projected menu and verify the original
            // working tools/history survived switching away and back.
            let returning = await menuActions(editor)["switch"]!.first { $0["command"]["id"].string == originalID }!
            try await edit(editor, returning)
            try await wait("switch back") { library.status["active_id"].string == originalID && acknowledged(editor) }
            let restored = try await capture(editor)
            // Storage timestamps newly committed revisions on their first
            // save. All existing timestamps and every other field must survive.
            let history = initial["capture"]["history"]
            var revisions = history["revisions"].object
            for (id, raw) in revisions {
                let revision = JSON(raw)
                if revision["timestamp_ms"].string == "0" {
                    let saved = restored["capture"]["history"]["revisions"][id]["timestamp_ms"]
                    precondition((UInt64(saved.string) ?? 0) > 0, "New history revision was not timestamped")
                    revisions[id] = revision.replacing("timestamp_ms", with: saved).raw
                }
            }
            let expected = initial["capture"].replacing("history", with: history.replacing("revisions", with: JSON(revisions)))
            precondition(restored["capture"].stableKey == expected.stableKey, "Switch/return changed stored tools or layout history")
            precondition(editor.state["layers"].stableKey == document && manager.error == nil && editor.failure == nil)
            try await library.close()
            print("PASS: platform \(platform), all nine shared workspace menu routes, form cancellation, view/history dismissal, request acknowledgement, switch/return and document preservation")
        }
    }
}
