import Foundation

/// Exercise actual shared menu payloads through the Apple editor and service.
/// No OS menu automation, user storage, renderer, or visible windows are needed.
@main struct WorkspaceMenuActionChecks {
    @MainActor static func query(_ store: EditorStore, _ request: [String: Any]) async -> JSON {
        await withCheckedContinuation { continuation in
            store.query(request) { continuation.resume(returning: $0) }
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
        store.workspaces?.busy == false && !store.state["requests"].array.contains {
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
            let workspaces = editor.workspaces!
            try await workspaces.started()
            // Give reset/history something to operate on and retain across
            // cancellation. This changes the workspace, not the document.
            try await editor.apply(["type": "set_brush_size", "value": 31])
            try await editor.apply(["type": "customize", "action": ["type": "set_panel_visible", "panel": "sizes", "visible": false]])
            let initial = try await workspaces.capture()
            let document = editor.state["layers"].stableKey
            let originalID = workspaces.view["id"].string
            let actions = await menuActions(editor)
            precondition(Set(actions.keys) == Set(routes), "Workspace menu service coverage drift: \(actions.keys.sorted())")
            for key in routes {
                let action = actions[key]!.first {
                    key != "switch" || $0["command"]["id"].string != originalID
                }!
                try await editor.apply(action.object)
                if let title = promptTitles[key] {
                    try await wait("\(key) prompt") { !workspaces.view["prompt"].isNull || workspaces.error != nil }
                    precondition(workspaces.view["prompt"]["title"].string == title,
                        "Wrong form for \(key): \(workspaces.view["prompt"].stableKey) \(workspaces.error ?? "")")
                    workspaces.cancelPrompt()
                } else if key == "switch" {
                    try await wait("switch applied") { workspaces.view["id"].string == action["command"]["id"].string || workspaces.error != nil }
                    precondition(!workspaces.presented && workspaces.view["id"].string != originalID)
                } else {
                    try await wait("\(key) view") {
                        !workspaces.busy && !workspaces.view["page"].isNull && !workspaces.view["rows"].array.isEmpty || workspaces.error != nil
                    }
                    precondition(workspaces.page == (key == "layout_history" ? "history" : key == "manage" ? "workspaces" : "this_workspace"))
                }
                try await wait("\(key) acknowledged") { acknowledged(editor) }
                precondition(workspaces.error == nil && editor.failure == nil, workspaces.error ?? editor.failure ?? "")
                try await workspaces.perform(["type": "dismiss"])
                try await wait("\(key) preview cleanup") { !workspaces.presented && !workspaces.busy }
                let after = try await workspaces.capture()
                precondition(editor.state["layers"].stableKey == document, "\(key) changed document layers")
                if key != "switch" {
                    precondition(workspaces.view["id"].string == originalID)
                    precondition(after.stableKey == initial.stableKey, "\(key) changed the workspace on cancellation/dismissal")
                }
            }
            // Return using the newly projected menu and verify the original
            // working tools/history survived switching away and back.
            let returning = await menuActions(editor)["switch"]!.first { $0["command"]["id"].string == originalID }!
            try await editor.apply(returning.object)
            try await wait("switch back") { workspaces.view["id"].string == originalID && acknowledged(editor) }
            let restored = try await workspaces.capture()
            // Storage timestamps newly committed revisions on their first
            // save. All existing timestamps and every other field must survive.
            let history = initial["history"]
            var revisions = history["revisions"].object
            for (id, raw) in revisions {
                let revision = JSON(raw)
                if revision["timestamp_ms"].string == "0" {
                    let saved = restored["history"]["revisions"][id]["timestamp_ms"]
                    precondition((UInt64(saved.string) ?? 0) > 0, "New history revision was not timestamped")
                    revisions[id] = revision.replacing("timestamp_ms", with: saved).raw
                }
            }
            let expected = initial.replacing("history", with: history.replacing("revisions", with: JSON(revisions)))
            precondition(restored.stableKey == expected.stableKey, "Switch/return changed stored tools or layout history")
            precondition(editor.state["layers"].stableKey == document && workspaces.error == nil && editor.failure == nil)
            try await workspaces.closed()
            print("PASS: platform \(platform), all nine shared workspace menu routes, form cancellation, view/history dismissal, request acknowledgement, switch/return and document preservation")
        }
    }
}
