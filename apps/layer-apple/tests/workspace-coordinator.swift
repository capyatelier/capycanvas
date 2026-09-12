// Exercise the real Swift lifecycle coordinator with temporary native SQLite
// storage. No visible window or system-menu automation is needed.
import Foundation
import SQLite3

@main struct WorkspaceCoordinatorChecks {
    @MainActor static func wait(_ label: String, until ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !ready() {
            guard Date() < deadline else { throw HostFailure(message: "Timed out: \(label)") }
            try await Task.sleep(for: .milliseconds(5))
        }
    }
    @MainActor static func edit(_ store: EditorStore, _ action: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            store.edit(action) { error in
                if let error { continuation.resume(throwing: HostFailure(message: error)) }
                else { continuation.resume() }
            }
        }
    }
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-coordinator-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let scene = UUID().uuidString, otherScene = UUID().uuidString
            // Take a real, customized legacy snapshot from the old editor.
            let legacy = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            try await wait("legacy editor") { !legacy.state.isNull }
            try await edit(legacy, ["type": "customize", "action": ["type": "set_panel_visible", "panel": "navigator", "visible": false]])
            try await edit(legacy, ["type": "invoke", "command": "zen_mode"])
            let legacyData = try JSONSerialization.data(withJSONObject: legacy.state["workspace"].raw, options: [.sortedKeys])
            let sceneFile = root.appendingPathComponent("workspaces/\(scene).json")
            let otherFile = root.appendingPathComponent("workspaces/\(otherScene).json")
            let fallback = root.appendingPathComponent("workspace.json")
            for file in [sceneFile, otherFile, fallback] { try AtomicJSONFile.write(legacyData, to: file) }
            let storage = EditorPersistence(root: root)
            let first = EditorStore(platform: platform, scene: scene, persistence: storage, managedWorkspaces: true)
            let manager = first.workspaceLibrary!
            try await wait("managed startup: \(manager.error ?? "")") { manager.ready || manager.error != nil }
            precondition(manager.ready, manager.error ?? "Startup failed")
            let original = manager.status["active_id"].string
            precondition(!original.isEmpty && first.state["workspace"]["zen_mode"].bool)
            precondition(SnapshotProjection.equal(first.state["workspace"]["layout"].raw, legacy.state["workspace"]["layout"].raw))
            let initial = try await manager.read(["type": "view", "page": "workspaces", "query": "", "idle": true])
            precondition(initial["rows"].array.count == 5, "Three defaults plus distinct migrated scenes; fallback must alias a scene")
            // The scene files are immutable migration inputs, even after edits.
            try await edit(first, ["type": "set_brush_size", "value": 73])
            try await edit(first, ["type": "customize", "action": ["type": "set_panel_visible", "panel": "navigator", "visible": true]])
            try await manager.flush()
            for file in [sceneFile, otherFile, fallback] {
                let after = try Data(contentsOf: file)
                precondition(after == legacyData, "Managed editors must not dual-write legacy storage")
            }
            // Switching immediately after accepted edits must capture them,
            // without waiting for the autosave timer or restoring stale tools.
            try await edit(first, ["type": "set_brush_size", "value": 87])
            _ = try await manager.operation(["type": "new", "name": "Clean"])
            let clean = manager.status["active_id"].string
            precondition(clean != original && first.state["brush"]["diameter"].number == 87)
            try await edit(first, ["type": "set_brush_size", "value": 44])
            _ = try await manager.operation(["type": "switch", "id": original])
            precondition(first.state["brush"]["diameter"].number == 87)
            do {
                _ = try await manager.operation(["type": "new", "name": "Clean"])
                preconditionFailure("A conflicting workspace name must fail")
            } catch { precondition(manager.status["active_id"].string == original && !manager.busy) }
            try await edit(first, ["type": "set_brush_size", "value": 91]) // Failure released the interaction lock.
            let autosaveDeadline = Date().addingTimeInterval(10)
            while true {
                let saved = try await manager.read(["type": "load", "id": original])
                let working = saved["entity"]["working"]
                let preset = String(working["preset"].uint)
                if working["tools"]["overrides"][preset]["size"].number == 91 { break }
                guard Date() < autosaveDeadline else { throw HostFailure(message: "Autosave did not persist the latest brush setting") }
                try await Task.sleep(for: .milliseconds(20))
            }
            // Flush serializes with any already scheduled autosave.
            try await manager.flush()
            manager.suspend()
            precondition(manager.readOnly)
            try await manager.resume()
            precondition(!manager.readOnly && first.state["brush"]["diameter"].number == 91)
            try await libraryActions(manager, editor: first, root: root)
            try await ownershipAndStorage(manager, editor: first, root: root, scene: scene, platform: platform)
            let second = EditorStore(platform: platform, scene: otherScene, persistence: storage, managedWorkspaces: true)
            try await wait("second scene") { second.workspaceLibrary!.ready || second.workspaceLibrary!.error != nil }
            precondition(second.workspaceLibrary!.ready, second.workspaceLibrary!.error ?? "Second scene failed")
            precondition(second.workspaceLibrary!.status["active_id"].string != original)
            try await manager.close()
            // Acknowledged old files no longer gate startup, even if corrupted.
            try Data("obsolete legacy input".utf8).write(to: sceneFile)
            try Data("obsolete fallback".utf8).write(to: fallback)
            let reopened = EditorStore(platform: platform, scene: scene, persistence: storage, managedWorkspaces: true)
            try await wait("reopened scene") { reopened.workspaceLibrary!.ready || reopened.workspaceLibrary!.error != nil }
            let restored = reopened.workspaceLibrary!
            precondition(restored.ready, restored.error ?? "Reopen failed")
            precondition(restored.status["active_id"].string == original && reopened.state["brush"]["diameter"].number == 91)
            let rows = try await restored.read(["type": "view", "page": "workspaces", "query": "", "idle": true])
            precondition(rows["rows"].array.count == 6, "Scene restoration must not create an unused workspace beside another live window")
            try await restored.close(); try await second.workspaceLibrary!.close()
            // Unknown legacy data must remain intact and must not create a
            // replacement default workspace or acknowledge a partial import.
            let badRoot = root.appendingPathComponent("unknown")
            let badFile = badRoot.appendingPathComponent("workspace.json")
            let badData = try JSONSerialization.data(withJSONObject: legacy.state["workspace"].replacing("version", with: JSON(999)).raw)
            try AtomicJSONFile.write(badData, to: badFile)
            let blocked = EditorStore(platform: platform, persistence: EditorPersistence(root: badRoot), managedWorkspaces: true)
            try await wait("unknown migration failure") { blocked.workspaceLibrary!.error != nil }
            precondition(!blocked.workspaceLibrary!.ready)
            let preserved = try Data(contentsOf: badFile)
            precondition(preserved == badData)
            let blockedRows = try await blocked.workspaceLibrary!.read(["type": "view", "page": "workspaces", "query": "", "idle": true])
            precondition(blockedRows["rows"].array.isEmpty)
            let recovery = try await blocked.workspaceLibrary!.read(["type": "export"])
            precondition(recovery["extension"].string == "capyworkspace" && !recovery["text"].string.isEmpty)
            try await blocked.workspaceLibrary!.backup(to: badRoot.appendingPathComponent("original.sqlite3"))
            print("PASS: platform \(platform), coordinator migration, scene ownership, latest-edit switching, failure unlock, legacy isolation, resume and restart")
        }
    }
    @MainActor static func ownershipAndStorage(_ manager: WorkspaceLibrary, editor: EditorStore,
        root: URL, scene: String, platform: UInt32) async throws {
        try await manager.flush()
        let original = manager.status["active_id"].string
        let database = try TestWorkspaceDatabase(root: root)
        try database.execute("BEGIN IMMEDIATE")
        // This lock belongs to the fixture, not the app's storage connection.
        // Keep it until the editor has served another edit/query and the save
        // is observably still pending, with an emergency deadline for failures.
        let emergency = Task { @MainActor in
            try await Task.sleep(for: .seconds(3))
            try database.execute("ROLLBACK")
        }
        try await edit(editor, ["type": "set_brush_size", "value": 93])
        var completed = false
        let saving = Task { @MainActor in try await manager.flush(); completed = true }
        try await wait("save waiting for storage") { manager.status["dirty"].bool }
        let started = ContinuousClock.now
        try await edit(editor, ["type": "set_brush_size", "value": 94])
        let queried: JSON = await withCheckedContinuation { continuation in
            editor.query(["type": "catalog"]) { continuation.resume(returning: $0) }
        }
        precondition(!queried.isNull && !completed && started.duration(to: .now) < .seconds(1),
            "Blocked SQLite storage must leave MainActor and the drawing owner responsive")
        emergency.cancel()
        try database.execute("ROLLBACK")
        try await saving.value
        try await manager.flush()
        let latest = try await manager.read(["type": "load", "id": original])["entity"]["working"]
        precondition(latest["tools"]["overrides"][String(latest["preset"].uint)]["size"].number == 94,
            "A save acknowledgement must not clear edits accepted while storage was blocked")
        // Advance only the temporary database's lease state, then let another
        // real native owner claim it. The old window must preserve its dirty
        // in-memory tools and recover them as an independent workspace.
        try await edit(editor, ["type": "set_brush_size", "value": 95])
        manager.suspend()
        try database.execute("UPDATE items SET lease_until='0' WHERE lease_until IS NOT NULL")
        let successor = EditorStore(platform: platform, scene: scene,
            persistence: EditorPersistence(root: root), managedWorkspaces: true)
        try await wait("successor ownership") { successor.workspaceLibrary!.ready || successor.workspaceLibrary!.error != nil }
        precondition(successor.workspaceLibrary!.ready, successor.workspaceLibrary!.error ?? "Successor failed")
        precondition(successor.workspaceLibrary!.status["active_id"].string == original)
        do { try await manager.resume(); preconditionFailure("A stale owner must not resume editing") }
        catch { precondition(manager.readOnly && editor.state["brush"]["diameter"].number == 95) }
        _ = try await manager.operation(["type": "save_as_new", "name": "Ownership Recovery"])
        let recovered = manager.status["active_id"].string
        precondition(recovered != original && editor.state["brush"]["diameter"].number == 95)
        // Resume the recovered independent owner, then return after the other
        // window closes; its durable workspace must retain its own value.
        try await manager.resume()
        try await successor.workspaceLibrary!.close()
        _ = try await manager.operation(["type": "switch", "id": original])
        precondition(editor.state["brush"]["diameter"].number == 94)
        _ = try await manager.operation(["type": "delete", "id": recovered])
        _ = try await manager.operation(["type": "delete_permanently", "id": recovered])
        try await edit(editor, ["type": "set_brush_size", "value": 91])
        let flushed: Bool = await withCheckedContinuation { continuation in
            editor.flushPersistence { continuation.resume(returning: $0) }
        }
        precondition(flushed, "The editor lifecycle barrier must include workspace storage")
    }
    @MainActor static func libraryActions(_ manager: WorkspaceLibrary, editor: EditorStore, root: URL) async throws {
        let original = manager.status["active_id"].string
        let saved = try await manager.operation(["type": "save_toolbar", "panel": "toolbar", "name": "Studio"])
        let reusable = saved["selected"].string
        precondition(!reusable.isEmpty)
        let before = try await manager.read(["type": "load", "id": reusable])
        precondition(before["entity"]["working"].isNull)
        let version = before["entity"]["content"]["current"]["id"].string
        _ = try await manager.operation(["type": "rename", "id": reusable, "name": "Studio Tools", "description": "Independent toolbar"])
        let renamed = try await manager.read(["type": "load", "id": reusable])
        let metadata = renamed["entity"]["metadata"]["previous"][0]["id"].string
        precondition(!metadata.isEmpty)
        _ = try await manager.operation(["type": "restore_metadata", "id": reusable, "version": metadata])
        _ = try await manager.operation(["type": "reset", "id": original])
        precondition(editor.state["brush"]["diameter"].number == 91, "Layout reset must preserve current working values")
        _ = try await manager.operation(["type": "update_toolbar", "id": reusable, "panel": "toolbar"])
        _ = try await manager.operation(["type": "restore_version", "id": reusable, "version": version])
        let exported = try await manager.read(["type": "export", "id": reusable])
        let package = try JSON.decode(exported["text"].string)
        precondition(package["working"].isNull && package["owner"].isNull && exported["extension"].string == "capytoolbar")
        let imported = try await manager.perform(["type": "import", "kind": "toolbar", "text": exported["text"].string])
        let importedID = imported["selected"].string
        precondition(importedID != reusable)
        let duplicate = try await manager.operation(["type": "duplicate", "id": reusable, "name": "Studio Copy"])
        for id in [importedID, duplicate["selected"].string] {
            _ = try await manager.operation(["type": "delete", "id": id])
            _ = try await manager.operation(["type": "restore_deleted", "id": id])
            _ = try await manager.operation(["type": "delete", "id": id])
            _ = try await manager.operation(["type": "delete_permanently", "id": id])
        }
        // Explicit toolbar names reject collisions; ordinary library copies
        // allocate independent local identities and an available name.
        let panel = try await manager.installToolbar(name: "Pencil Tools")
        do {
            _ = try await manager.installToolbar(name: "Pencil Tools")
            preconditionFailure("Explicit toolbar name collisions must fail")
        } catch { precondition(!manager.busy) }
        let toolbar = try await manager.operation(["type": "save_toolbar", "panel": panel.raw, "name": "Pencil Library"])
        let toolbarID = toolbar["selected"].string
        let copy = try await manager.installToolbar(id: toolbarID)
        precondition(!SnapshotProjection.equal(panel.raw, copy.raw))
        _ = try await manager.operation(["type": "update_toolbar", "id": toolbarID, "panel": copy.raw])
        _ = try await manager.installToolbar(id: toolbarID, name: "Replaced Tools", replace: copy)
        let toolbarExport = try await manager.read(["type": "export", "id": toolbarID])
        _ = try await manager.perform(["type": "import", "kind": "toolbar", "text": toolbarExport["text"].string])
        let local = try await manager.read(["type": "view", "page": "this_workspace", "query": "", "selected": panel.stableKey, "idle": true])
        precondition(local["details"]["title"].string == "Pencil Tools")
        // Opening retained history creates independent workspaces and
        // never consumes the original workspace's navigation or current tools.
        let source = try await manager.read(["type": "load", "id": original])
        let revision = source["entity"]["content"]["history"]["current"].string
        for operation: [String: Any] in [
            ["type": "open_history", "id": original, "revision": revision, "name": "History Copy"],
            ["type": "save_as_new", "name": "Recovered Copy"]
        ] {
            _ = try await manager.operation(operation)
            let created = manager.status["active_id"].string
            precondition(created != original)
            _ = try await manager.operation(["type": "switch", "id": original])
            precondition(editor.state["brush"]["diameter"].number == 91)
            _ = try await manager.operation(["type": "delete", "id": created])
            _ = try await manager.operation(["type": "delete_permanently", "id": created])
        }
        // Export reads the latest accepted working edit even before autosave.
        try await edit(editor, ["type": "set_brush_size", "value": 92])
        let current = try await manager.read(["type": "export", "id": original])
        let backup = try JSON.decode(current["text"].string)
        let working = backup["working"]
        precondition(working["tools"]["overrides"][String(working["preset"].uint)]["size"].number == 92)
        try await edit(editor, ["type": "set_brush_size", "value": 91])
        _ = try await manager.read(["type": "storage", "clear_older": false, "apply": false])
        _ = try await manager.perform(["type": "storage", "clear_older": false, "apply": true])
        _ = try await manager.read(["type": "interrupted"])
        let destination = root.appendingPathComponent("consistent-backup.sqlite3")
        _ = try await manager.perform(["type": "backup", "path": destination.path])
        let bytes = try Data(contentsOf: destination)
        precondition(bytes.starts(with: Data("SQLite format 3\0".utf8)))
    }
}

private final class TestWorkspaceDatabase {
    private let handle: OpaquePointer
    init(root: URL) throws {
        var handle: OpaquePointer?
        guard sqlite3_open(root.appendingPathComponent("workspaces.sqlite3").path, &handle) == SQLITE_OK, let handle else {
            throw HostFailure(message: "Could not open fixture SQLite connection")
        }
        self.handle = handle
    }
    deinit { sqlite3_close(handle) }
    func execute(_ sql: String) throws {
        guard sqlite3_exec(handle, sql, nil, nil, nil) == SQLITE_OK else {
            throw HostFailure(message: String(cString: sqlite3_errmsg(handle)))
        }
    }
}
