// Exercise the shared workspace controller through the Apple editor lifecycle
// with temporary native SQLite storage. No visible window or menu automation.
import Foundation

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
            try await desktopViewport(first)
            try await workspaces.perform(["type": "form", "action": ["type": "new"]])
            try languageReadiness(platform: platform, root: root, projection: workspaces.view)
            workspaces.cancelPrompt(); try await workspaces.settle()
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
            precondition(first.state["brush"]["diameter"].number == 44)
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
            let badStorage = EditorPersistence(root: root.appendingPathComponent("corrupt-library"))
            let badFile = badStorage.locations!.workspaces.appendingPathComponent("workspaces.sqlite3")
            try FileManager.default.createDirectory(at: badFile.deletingLastPathComponent(), withIntermediateDirectories: true)
            let badData = Data("invalid SQLite data".utf8)
            try badData.write(to: badFile)
            let recovered = EditorStore(platform: platform, persistence: badStorage)
            let recoveredWorkspaces = recovered.workspaces!
            try await recoveredWorkspaces.started("corrupt database recovery")
            try await uncover(recovered)
            precondition(recovered.failure == nil && !recovered.state["notice"].isNull)
            try require(await recoveredWorkspaces.workspaceRows().count == 3, "Recovered storage must initialize the shared defaults")
            try await recoveredWorkspaces.closed()
            let damaged = try FileManager.default.contentsOfDirectory(at: badFile.deletingLastPathComponent(), includingPropertiesForKeys: nil)
                .filter { $0.lastPathComponent.hasPrefix("workspaces.damaged.") && $0.pathExtension == "sqlite3" }
            try require(damaged.count == 1, "Recovery must set aside exactly one damaged library")
            try require(Data(contentsOf: damaged[0]) == badData, "Recovery must preserve the damaged library bytes")
            try TestWorkspaceDatabase(directory: badStorage.locations!.workspaces).execute("SELECT id FROM items")
            try await releasedStores(platform: platform, root: root.appendingPathComponent("released"))
            print("PASS: platform \(platform), SQLite startup, scene restoration, latest-edit switching, failure unlock, suspension, ownership recovery, restart, unreadable-store preservation, released 1.0.12 workspaces and damaged-store copy install")
        }
    }
    @MainActor static func releasedStores(platform: UInt32, root: URL) async throws {
        let tests = URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("crates/layer-workspace/tests")
        let released = try JSON.decode(String(contentsOf: tests.appendingPathComponent("fixtures/web-store-1.0.12.json"), encoding: .utf8))["items"]
        let probe = EditorPersistence(root: root.appendingPathComponent("version"))
        let probeEditor = EditorStore(platform: platform, persistence: probe)
        try await probeEditor.workspaces!.started("version marker")
        try await probeEditor.workspaces!.closed()
        let version = try JSON.decode(String(contentsOf: probe.locations!.workspaces.appendingPathComponent("last-version"), encoding: .utf8))["version"].string
        func stage(_ name: String, retained: Bool) throws -> EditorPersistence {
            let storage = EditorPersistence(root: root.appendingPathComponent(name))
            let directory = storage.locations!.workspaces, database = directory.appendingPathComponent("workspaces.sqlite3")
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try FileManager.default.copyItem(at: tests.appendingPathComponent("compat/1.0.15/workspaces.sqlite3"), to: database)
            var copies: [[String: Any]] = []
            if retained {
                let id = "workspaces.\(version).\(UUID().uuidString.lowercased()).sqlite3"
                try FileManager.default.copyItem(at: database, to: directory.appendingPathComponent(id))
                copies.append(["id": id, "bytes": try Data(contentsOf: database).count])
            }
            try JSON(["version": version, "unreadable_identity": NSNull(), "copies": copies]).encoded()
                .write(to: directory.appendingPathComponent("last-version"), atomically: true, encoding: .utf8)
            return storage
        }
        func quiet(_ workspaces: WorkspaceController, _ step: String) throws {
            try require(workspaces.error == nil && workspaces.view["status"].isNull && workspaces.view["prompt"].isNull,
                "\(step) asks nothing: \(workspaces.view["status"].stableKey) \(workspaces.view["prompt"].stableKey)")
        }
        func toolbars(_ layout: JSON) -> [String: [UInt64]] {
            Dictionary(uniqueKeysWithValues: layout["panels"].array.filter { $0["content"]["kind"].string == "toolbar" }.map { panel in
                (panel["id"].stableKey, panel["content"]["tiles"].array.filter {
                    !["opacity", "brush_opacity_slider"].contains($0["control"]["kind"].string) && $0["control"]["command"].string != "decoration"
                }.map { $0["id"].uint })
            })
        }
        let ids = ["builtin:workspace:painter", "builtin:workspace:illustrator", "builtin:workspace:photographer"]
        let storage = try stage("released", retained: false)
        let database = try TestWorkspaceDatabase(directory: storage.locations!.workspaces)
        for id in ids {
            let content = released[id]["entity"]["content"].stableKey.replacingOccurrences(of: "'", with: "''")
            try database.execute("UPDATE items SET content='\(content)' WHERE id='\(id)'")
        }
        func stored() throws -> [String] { try ids.flatMap { try database.strings("SELECT content FROM items WHERE id='\($0)'") } }
        let before = try stored()
        let editor = EditorStore(platform: platform, persistence: storage)
        let workspaces = editor.workspaces!
        try await workspaces.started("released 1.0.12 workspaces")
        try quiet(workspaces, "startup")
        for id in ids {
            try await workspaces.perform(["type": "switch", "id": id])
            try require(workspaces.view["id"].string == id, "\(id) opens")
            try quiet(workspaces, id)
            let history = released[id]["entity"]["content"]["history"]
            let current = toolbars(history["revisions"][history["current"].string]["layout"])
            try require(toolbars(editor.state["workspace"]["layout"]) == current,
                "\(id) keeps every released tile this build reads: \(toolbars(editor.state["workspace"]["layout"])) != \(current)")
        }
        try await workspaces.flushed()
        try await workspaces.closed()
        try require(try stored() == before, "Opening released workspaces rewrites none of them")
        let reopened = EditorStore(platform: platform, persistence: storage)
        try await reopened.workspaces!.started("released workspaces relaunch")
        try quiet(reopened.workspaces!, "relaunch")
        try require(reopened.workspaces!.view["id"].string == ids[2], "The last released workspace reopens")
        try await reopened.workspaces!.closed()

        let damaged = try stage("damaged", retained: true)
        let directory = damaged.locations!.workspaces
        func copies() throws -> Set<String> {
            Set(try FileManager.default.contentsOfDirectory(atPath: directory.path)
                .filter { $0.hasPrefix("workspaces.") && $0.hasSuffix(".sqlite3") && $0 != "workspaces.sqlite3" })
        }
        let retained = try copies()
        try Data("damaged SQLite store".utf8).write(to: directory.appendingPathComponent("workspaces.sqlite3"))
        let installed = EditorStore(platform: platform, persistence: damaged)
        try await installed.workspaces!.started("damaged store installs its newest copy")
        try quiet(installed.workspaces!, "damaged store")
        try await uncover(installed)
        try require(installed.state["notice"]["text"].string == "Your saved workspaces were damaged. Capy Canvas restored an earlier copy."
            && installed.state["notice"]["actions"].array.isEmpty, "The restored-copy notice has no buttons: \(installed.state["notice"].stableKey)")
        try require(installed.state["brush"]["diameter"].number == 37, "The retained copy's settings open")
        let after = try copies()
        try require(after.filter { $0.hasPrefix("workspaces.damaged.") }.count == 1 && retained.isDisjoint(with: after),
            "The damaged store is set aside and its installed copy is consumed: \(retained) \(after)")
        try await installed.workspaces!.flushed()
        try await installed.workspaces!.closed()
        let restarted = EditorStore(platform: platform, persistence: damaged)
        try await restarted.workspaces!.started("installed copy relaunch")
        try quiet(restarted.workspaces!, "installed copy relaunch")
        try await uncover(restarted)
        try require(restarted.state["notice"].isNull && restarted.state["brush"]["diameter"].number == 37 && (try copies()) == after,
            "A second start sets nothing aside and installs nothing")
        try await restarted.workspaces!.closed()
    }
    @MainActor static func languageReadiness(platform: UInt32, root: URL, projection: JSON) throws {
        let editor = EditorStore(platform: platform, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
        let controller = WorkspaceController(store: editor, directory: root.appendingPathComponent("language-publication"), scene: UUID().uuidString)
        _ = editor.snapshot.stagePatch(["canvas_ready": JSON(true)])
        controller.formName = "Unfinished name"; controller.formDescription = "Unfinished description"; controller.formChoice = "Unfinished choice"
        controller.receiveLanguage(projection)
        try require(!editor.recovery.restoring, "A ready language publication must start recovery before the ordinary workspace reply")
        try require(controller.formName == "Unfinished name" && controller.formDescription == "Unfinished description"
            && controller.formChoice == "Unfinished choice", "Language publication must preserve every open form draft")
        editor.recovery = ArtworkRecovery(store: editor)
        controller.receiveLanguage(projection)
        try require(editor.recovery.restoring, "A repeated ready publication must not replay the initialization transition")
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
        let database = try TestWorkspaceDatabase(directory: StorageLocations.within(root)!.workspaces)
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
        try await workspaces.perform(["type": "suspend"])
        try await wait("released claim") { !workspaces.busy && !workspaces.view["dirty"].bool }
        let successor = EditorStore(platform: platform, scene: scene,
            persistence: EditorPersistence(root: root), managedWorkspaces: true)
        try await successor.workspaces!.started("successor ownership")
        try await desktopViewport(successor)
        precondition(successor.workspaces!.view["id"].string == original)
        precondition(successor.state["brush"]["diameter"].number == 95, "Suspension must save before releasing the claim")
        try await successor.apply(["type": "set_brush_size", "value": 96])
        try await successor.workspaces!.answer(["type": "new_toolbar", "value": NSNull()], name: "Successor tools")
        try await successor.workspaces!.flushed()
        let successorLayout = successor.state["workspace"]["layout"].stableKey
        let retainedLayout = editor.state["workspace"]["layout"].stableKey
        precondition(successorLayout != retainedLayout && editor.state["brush"]["diameter"].number == 95)
        workspaces.resume()
        try await wait("stale owner revalidation and durable editing refresh") {
            workspaces.view["owner_lost"].bool && workspaces.error != nil && !workspaces.switcherBusy
                && editor.state["brush"]["diameter"].number == 96
        }
        precondition(workspaces.readOnly && editor.state["workspace"]["layout"].stableKey == retainedLayout)
        try await workspaces.answer(["type": "save_as_new"], name: "Ownership Recovery")
        let recovered = workspaces.view["id"].string
        try require(recovered != original && !workspaces.readOnly && workspaces.error == nil
            && editor.state["brush"]["diameter"].number == 96,
            "Save as New after losing ownership retains the refreshed durable brush: id \(recovered) from \(original), read-only \(workspaces.readOnly), error \(workspaces.error ?? "none"), brush \(editor.state["brush"]["diameter"].number)")
        try await successor.workspaces!.closed()
        try await workspaces.perform(["type": "focus"])
        precondition(editor.state["brush"]["diameter"].number == 96, "The peer's durable brush preference must be adopted before switching")
        let currentBrush = editor.state["brush"].stableKey
        try await workspaces.perform(["type": "switch", "id": original])
        precondition(editor.state["workspace"]["layout"].stableKey == successorLayout, "The other window's durable workspace must be adopted")
        precondition(editor.state["brush"].stableKey == currentBrush, "Switching workspaces preserves the current brush")
        try await workspaces.answer(["type": "delete", "value": recovered])
        try await editor.apply(["type": "set_brush_size", "value": 91])
        let flushed: Bool = await withCheckedContinuation { continuation in
            editor.flushPersistence { continuation.resume(returning: $0) }
        }
        precondition(!editor.snapshot["canvas_ready"].bool && editor.recovery.restoring)
        precondition(!flushed, "Unrestored artwork must fail the full editor lifecycle barrier")
        precondition(!workspaces.view["dirty"].bool && !workspaces.view["saving"].bool && workspaces.error == nil,
            "The editor lifecycle barrier must finish workspace storage before reporting artwork failure")
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
