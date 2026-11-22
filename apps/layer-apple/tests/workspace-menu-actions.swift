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
        if action["type"].string == "invoke", ["manage_toolbars", "reset_layout"].contains(action["command"].string) {
            return action["command"].string
        }
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
    @MainActor static func contextualRows(_ store: EditorStore) async throws {
        func rows() -> [JSON] { editorApplicationMenu(store)["sections"].array.flatMap(\.array) }
        func stableRows(_ rows: [JSON]) -> [JSON] {
            rows.filter { $0["action"]["command"].string != "zoom_out" }.map { row in
                guard let sections = row.object["sections"] as? [[Any]] else { return row }
                return row.replacing("sections", with: JSON(sections.map { stableRows($0.map { JSON($0) }).map(\.raw) }))
            }
        }
        func drawing(_ row: JSON) -> Bool { row["action"]["command"].string == "drawings" }
        func switcher(_ row: JSON) -> Bool { row["label"].string == store.workspaces!.view["switcher_menu"]["title"].string }
        func header(_ action: [String: Any]) async throws {
            try await store.apply(["type": "customize", "action": ["type": "header", "action": action]])
        }
        let original = stableRows(rows()).map(\.stableKey)
        let originalHeader = store.snapshot["header"]["model"]["zones"].stableKey
        let originalExtras = store.snapshot["main_menu"]["extras"].stableKey
        let file = editorApplicationMenu(store, id: "file").stableKey
        let entries = store.snapshot["header"]["model"]["zones"].array.flatMap(\.array)
        let title = entries.first { $0["item"]["kind"].string == "document_title" }!
        let workspaces = entries.first { $0["item"]["kind"].string == "workspaces" }!
        precondition(!rows().contains(where: drawing) && !rows().contains(where: switcher))
        try await header(["type": "edit", "editing": true])
        try await header(["type": "remove", "id": title["id"].raw])
        let drawings = rows().filter(drawing)
        precondition(drawings.count == 1 && !drawings[0]["enabled"].bool,
            "Main Menu must expose the single drawing when its title is removed")
        precondition(drawings[0].stableKey == store.snapshot["main_menu"]["drawing"].stableKey)
        precondition(!rows().contains(where: switcher))
        try await header(["type": "remove", "id": workspaces["id"].raw])
        let choices = rows().filter(switcher)
        precondition(choices.count == 1 && choices[0]["sections"].stableKey == store.workspaces!.view["switcher_menu"]["sections"].stableKey,
            "Main Menu must expose the shared workspace switcher when its header is removed")
        precondition(editorApplicationMenu(store, id: "file").stableKey == file,
            "Contextual Main Menu rows must not change the File menu")
        try await header(["type": "cancel"])
        precondition(store.snapshot["header"]["model"]["zones"].stableKey == originalHeader,
            "Cancelling header editing must restore its exact entries")
        let restored = stableRows(rows()).map(\.stableKey)
        precondition(restored == original,
            "Restoring the header preserves every menu row except the viewport-dependent Zoom out state\nBefore: \(original)\nAfter: \(restored)")
        precondition(store.snapshot["main_menu"]["extras"].stableKey == originalExtras
            && !rows().contains(where: drawing) && !rows().contains(where: switcher),
            "Restoring the header must remove its contextual rows")
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
            try await desktopViewport(editor)
            try await workspaces.perform(["type": "switch", "id": "builtin:workspace:illustrator"])
            try await contextualRows(editor)
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
            var current = history["current"]
            if current["timestamp_ms"].string == "0" {
                let saved = restored["history"]["current"]["timestamp_ms"]
                precondition((UInt64(saved.string) ?? 0) > 0, "New history revision was not timestamped")
                current = current.replacing("timestamp_ms", with: saved)
            }
            let expected = initial.replacing("history", with: history.replacing("current", with: current))
            precondition(restored.stableKey == expected.stableKey, "Switch/return changed stored tools or layout history")
            precondition(editor.state["layers"].stableKey == document && workspaces.error == nil && editor.failure == nil)
            try await workspaces.closed()
            print("PASS: platform \(platform), all nine shared workspace menu routes, form cancellation, view/history dismissal, request acknowledgement, switch/return and document preservation")
        }
    }
}
