// Exercise the shared workspace controller through the Apple editor lifecycle
// with temporary native SQLite storage. No visible window or menu automation.
import Foundation
import SQLite3

@main struct WorkspaceCoordinatorChecks {
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-coordinator-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let scene = UUID().uuidString, otherScene = UUID().uuidString
            let storage = EditorPersistence(root: root)
            let first = EditorStore(platform: platform, scene: scene, persistence: storage, managedWorkspaces: true)
            let workspaces = first.workspaces!
            workspaces.suspend()
            workspaces.resume()
            try await workspaces.started()
            try await editable(first, 31)
            let original = workspaces.view["id"].string
            precondition(!original.isEmpty)
            try await first.apply(["type": "customize", "action": ["type": "set_panel_visible", "panel": "navigator", "visible": false]])
            try await first.apply(["type": "invoke", "command": "zen_mode"])
            precondition(first.state["workspace"]["zen_mode"].bool)
            try require(await workspaces.workspaceRows().count == 3, "Initialize only the shared default workspaces")
            try await first.apply(["type": "set_brush_size", "value": 73])
            try await first.apply(["type": "customize", "action": ["type": "set_panel_visible", "panel": "navigator", "visible": true]])
            try await workspaces.flushed()
            try await first.apply(["type": "set_brush_size", "value": 87])
            try await workspaces.create("Clean")
            let clean = workspaces.view["id"].string
            precondition(clean != original && first.state["brush"]["diameter"].number == 87)
            try await first.apply(["type": "set_brush_size", "value": 44])
            try await workspaces.perform(["type": "switch", "id": original])
            precondition(first.state["brush"]["diameter"].number == 87)
            try await workspaces.perform(["type": "form", "action": ["type": "new"]])
            workspaces.formName = "Clean"; workspaces.submit()
            try await wait("conflicting name") { workspaces.error != nil }
            workspaces.cancelPrompt()
            try await wait("failure unlock") { !workspaces.busy && workspaces.view["prompt"].isNull }
            precondition(workspaces.view["id"].string == original)
            try await first.apply(["type": "set_brush_size", "value": 91])
            try await workspaces.flushed()
            workspaces.suspend()
            try await wait("suspension") { !workspaces.busy }
            do {
                try await first.apply(["type": "set_brush_size", "value": 92])
                preconditionFailure("A suspended workspace must reject editor changes")
            } catch { precondition(first.state["brush"]["diameter"].number == 91) }
            workspaces.resume()
            try await editable(first, 91)
            try await libraryActions(workspaces, editor: first)
            try await ownershipAndStorage(workspaces, editor: first, root: root, scene: scene, platform: platform)
            let second = EditorStore(platform: platform, scene: otherScene, persistence: storage, managedWorkspaces: true)
            try await second.workspaces!.started("second scene")
            precondition(second.workspaces!.view["id"].string != original)
            try await workspaces.closed()
            let reopened = EditorStore(platform: platform, scene: scene, persistence: storage, managedWorkspaces: true)
            let restored = reopened.workspaces!
            try await restored.started("reopened scene")
            precondition(restored.view["id"].string == original && reopened.state["brush"]["diameter"].number == 91)
            try require(await restored.workspaceRows().count == 4,
                "Scene restoration must not create an unused workspace beside another live window")
            try await restored.closed(); try await second.workspaces!.closed()
            let badRoot = root.appendingPathComponent("corrupt-library")
            try FileManager.default.createDirectory(at: badRoot, withIntermediateDirectories: false)
            let badFile = badRoot.appendingPathComponent("workspaces.sqlite3")
            let badData = Data("invalid SQLite data".utf8)
            try badData.write(to: badFile)
            let blocked = EditorStore(platform: platform, persistence: EditorPersistence(root: badRoot))
            try await wait("corrupt database error") { blocked.failure != nil || blocked.workspaces?.error != nil }
            precondition(blocked.workspaces?.ready != true)
            try require(Data(contentsOf: badFile) == badData, "A corrupt library must not be replaced")
            print("PASS: platform \(platform), SQLite startup, scene restoration, latest-edit switching, failure unlock, suspension, ownership recovery and restart")
        }
    }
    @MainActor static func editable(_ editor: EditorStore, _ size: Double) async throws {
        var accepted = false
        try await wait("editable workspace", step: {
            accepted = (try? await editor.apply(["type": "set_brush_size", "value": size])) != nil
        }) { accepted }
    }
    @MainActor static func ownershipAndStorage(_ workspaces: WorkspaceController, editor: EditorStore,
        root: URL, scene: String, platform: UInt32) async throws {
        try await workspaces.flushed()
        let original = workspaces.view["id"].string
        let database = try TestWorkspaceDatabase(root: root)
        try database.execute("BEGIN IMMEDIATE")
        let emergency = Task { @MainActor in
            try await Task.sleep(for: .seconds(3))
            try database.execute("ROLLBACK")
        }
        try await editor.apply(["type": "set_brush_size", "value": 93])
        try await wait("save waiting for storage") { workspaces.view["saving"].bool }
        let started = ContinuousClock.now
        try await editor.apply(["type": "set_brush_size", "value": 94])
        let queried: JSON = await withCheckedContinuation { continuation in
            editor.query(["type": "catalog"]) { continuation.resume(returning: $0) }
        }
        precondition(!queried.isNull && workspaces.view["saving"].bool && started.duration(to: .now) < .seconds(1),
            "Blocked SQLite storage must leave MainActor and the drawing owner responsive")
        emergency.cancel()
        try database.execute("ROLLBACK")
        try await workspaces.flushed()
        try await editor.apply(["type": "set_brush_size", "value": 95])
        workspaces.suspend()
        try await wait("released claim") { !workspaces.busy && !workspaces.view["dirty"].bool }
        let successor = EditorStore(platform: platform, scene: scene,
            persistence: EditorPersistence(root: root), managedWorkspaces: true)
        try await successor.workspaces!.started("successor ownership")
        precondition(successor.workspaces!.view["id"].string == original)
        precondition(successor.state["brush"]["diameter"].number == 95, "Suspension must save before releasing the claim")
        try await successor.apply(["type": "set_brush_size", "value": 96])
        workspaces.resume()
        try await wait("stale owner") { workspaces.view["owner_lost"].bool }
        precondition(workspaces.readOnly && editor.state["brush"]["diameter"].number == 95)
        try await workspaces.answer(["type": "save_as_new"], name: "Ownership Recovery")
        let recovered = workspaces.view["id"].string
        precondition(recovered != original && !workspaces.readOnly && editor.state["brush"]["diameter"].number == 95)
        try await successor.workspaces!.closed()
        try await workspaces.perform(["type": "switch", "id": original])
        precondition(editor.state["brush"]["diameter"].number == 96, "The other window's durable workspace must be adopted")
        try await workspaces.answer(["type": "delete", "value": recovered])
        try await editor.apply(["type": "set_brush_size", "value": 91])
        let flushed: Bool = await withCheckedContinuation { continuation in
            editor.flushPersistence { continuation.resume(returning: $0) }
        }
        precondition(flushed && !workspaces.view["dirty"].bool, "The editor lifecycle barrier must include workspace storage")
    }
    @MainActor static func libraryActions(_ workspaces: WorkspaceController, editor: EditorStore) async throws {
        let original = workspaces.view["id"].string
        func saved(_ name: String) async throws -> String {
            try await workspaces.perform(["type": "open", "page": "toolbar_library"])
            let id = workspaces.view["rows"].array.first { $0["title"].string == name }?["id"].string
            try await workspaces.perform(["type": "dismiss"])
            guard let id else { throw HostFailure(message: "Missing saved toolbar \(name)") }
            return id
        }
        func toolbars() -> [JSON] {
            editor.state["workspace"]["layout"]["panels"].array.filter { $0["content"]["kind"].string == "toolbar" }
        }
        try await workspaces.answer(["type": "save_toolbar", "value": "toolbar"], name: "Studio")
        let reusable = try await saved("Studio")
        try await workspaces.answer(["type": "rename", "value": reusable], name: "Studio Tools", description: "Independent toolbar")
        try require(await saved("Studio Tools") == reusable, "Renaming a saved workspace keeps its identity")
        try await workspaces.answer(["type": "reset", "value": original])
        precondition(editor.state["brush"]["diameter"].number == 91, "Layout reset must preserve current working values")
        try await workspaces.answer(["type": "update_toolbar", "value": reusable])
        try await workspaces.answer(["type": "new_toolbar", "value": NSNull()], name: "Pencil Tools")
        let count = toolbars().count
        try await workspaces.perform(["type": "form", "action": ["type": "new_toolbar", "value": NSNull()]])
        workspaces.formName = "Pencil Tools"; workspaces.submit()
        try await wait("toolbar name collision") { workspaces.error != nil }
        workspaces.cancelPrompt()
        try await wait("collision unlock") { !workspaces.busy && workspaces.view["prompt"].isNull }
        precondition(toolbars().count == count, "Explicit toolbar name collisions must fail")
        try await workspaces.perform(["type": "action", "action": ["type": "add_toolbar", "value": reusable]])
        precondition(toolbars().count == count + 1, "Adding a saved toolbar installs an independent copy")
        try await workspaces.answer(["type": "save_as_new"], name: "Recovered Copy")
        let created = workspaces.view["id"].string
        precondition(created != original)
        try await workspaces.perform(["type": "switch", "id": original])
        precondition(editor.state["brush"]["diameter"].number == 91)
        try await workspaces.answer(["type": "delete", "value": created])
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
