import Foundation
import Observation

private final class Changes: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    func record() { lock.lock(); count += 1; lock.unlock() }
    var value: Int { lock.lock(); defer { lock.unlock() }; return count }
}

@main struct SnapshotProjectionChecks {
    @MainActor private static func watch(_ read: () -> Void) -> Changes {
        let changes = Changes()
        withObservationTracking(read, onChange: { changes.record() })
        return changes
    }
    @MainActor static func main() throws {
        checkWorkspaceMotion()
        let model = SnapshotProjection()
        func update(_ raw: Any) { model.stage(JSON(raw)).forEach { $0.publish() } }
        let initial = watch { _ = model.isNull }
        let missing = watch { _ = model["brush"]["size"].number }
        update(["brush": ["size": 24], "history": 0])
        precondition(initial.value == 1 && missing.value == 1)
        let retained = model.json
        let brush = watch { _ = model["brush"]["size"].number }
        let history = watch { _ = model["history"].uint }
        let nullness = watch { _ = model.isNull }
        let whole = watch { _ = model.json.raw }
        update(["brush": ["size": 24], "history": 1])
        precondition(brush.value == 0 && history.value == 1 && nullness.value == 0 && whole.value == 1)
        update(["brush": ["size": 48], "history": 1])
        precondition(brush.value == 1 && model["brush"]["size"].number == 48)
        precondition(retained["brush"]["size"].number == 24 && retained["history"].uint == 0,
            "Retained snapshots must never become live mutable references")
        let unchanged = watch { _ = model.json.raw }
        let decoded = try JSON.decode(model.json.encoded())
        model.stage(decoded).forEach { $0.publish() }
        precondition(unchanged.value == 0, "Equivalent Foundation and Swift values must not invalidate views")
        let removed = watch { _ = model["brush"].isNull }
        update(["history": 1])
        precondition(removed.value == 1 && nullness.value == 0)
        let restored = watch { _ = model["brush"].isNull }
        update(["brush": NSNull(), "history": 1])
        precondition(restored.value == 1)
        update(NSNull())
        precondition(nullness.value == 1 && model.isNull)

        let wholeOnly = SnapshotProjection()
        let addedField = watch { _ = wholeOnly.json }
        wholeOnly.stage(JSON(["unread": NSNull()])).forEach { $0.publish() }
        precondition(addedField.value == 1 && wholeOnly["unread"].isNull)
        let removedField = watch { _ = wholeOnly.json }
        wholeOnly.stage(JSON([String: Any]())).forEach { $0.publish() }
        precondition(removedField.value == 1, "Whole reads must observe key presence even when both values are null")
        let unobservedField = watch { _ = wholeOnly.json }
        wholeOnly.stage(JSON(["new": ["nested": true]])).forEach { $0.publish() }
        precondition(unobservedField.value == 1 && wholeOnly["new"]["nested"].bool,
            "Whole reads must observe fields that no individual reader requested")

        let commands = SnapshotProjection()
        func commandUpdate(_ values: [[String: Any]]) {
            commands.stage(SnapshotProjection.indexed(JSON(values))).forEach { $0.publish() }
        }
        commandUpdate([["id": "undo", "enabled": false], ["id": "redo", "enabled": true]])
        let undo = watch { _ = commands["undo"]["enabled"].bool }
        let redo = watch { _ = commands["redo"]["enabled"].bool }
        commandUpdate([["id": "redo", "enabled": true], ["id": "undo", "enabled": true]])
        precondition(undo.value == 1 && redo.value == 0)
        commandUpdate([["id": "undo", "enabled": true], ["id": "redo", "enabled": true], ["id": "redo", "enabled": false]])
        precondition(redo.value == 0 && commands["redo"]["enabled"].bool,
            "Indexed reads preserve first-match semantics and ignore ordering")
        commandUpdate([["id": "undo", "enabled": true]])
        precondition(redo.value == 1 && commands["redo"].isNull)

        for (before, after): (Any, Any) in [(true, 1), (UInt64.max, UInt64.max - 1),
            (0.0, -0.0), (1.0, Double(1).nextUp), ([1, 2], [2, 1]), (NSNull(), false)] {
            update(["value": before])
            let changes = watch { _ = model["value"].raw }
            update(["value": after])
            precondition(changes.value == 1, "A distinct JSON value must invalidate its readers")
        }
        let editor = EditorSnapshotState()
        let first = JSON(["state": ["revision": 1, "camera": ["zoom": 1], "brush": ["size": 24],
                                   "commands": [["id": "undo", "enabled": false]]],
                          "panels": [["id": "toolbar", "enabled": false], ["id": "color", "enabled": true]],
                          "application_menus": [["id": "edit", "enabled": false]], "layout": ["width": 1200]])
        editor.receive(first)
        let untouched = watch {
            _ = editor.panel("color").raw; _ = editor.state["brush"].raw; _ = editor.snapshot["layout"].raw
        }
        let coherent = Changes()
        withObservationTracking({ _ = editor.command("undo")["enabled"].bool }, onChange: {
            MainActor.assumeIsolated {
                if editor.state["revision"].uint == 2 && editor.command("undo")["enabled"].bool
                    && editor.panel("toolbar")["enabled"].bool && editor.applicationMenu("edit")["enabled"].bool {
                    coherent.record()
                }
            }
        })
        let next = JSON(["state": ["revision": 2, "camera": ["zoom": 1], "brush": ["size": 24],
                                  "commands": [["id": "undo", "enabled": true]]],
                         "panels": [["id": "toolbar", "enabled": true], ["id": "color", "enabled": true]],
                         "application_menus": [["id": "edit", "enabled": true]], "layout": ["width": 1200]])
        editor.receive(next)
        precondition(coherent.value == 1 && untouched.value == 0,
            "Readers must see a coherent update without invalidating unrelated views")
        let command = watch { _ = editor.command("undo").raw }
        let panel = watch { _ = editor.panel("toolbar").raw }
        let menu = watch { _ = editor.applicationMenu("edit").raw }
        let camera = watch { _ = editor.state["camera"]["zoom"].number }
        let previous = editor.snapshot.json
        editor.receive(JSON(["camera": ["zoom": 2], "revision": 3]))
        precondition(camera.value == 1 && editor.snapshot["state"]["camera"]["zoom"].number == 2)
        precondition(command.value == 0 && panel.value == 0 && menu.value == 0 && untouched.value == 0)
        precondition(editor.state["revision"].uint == 3 && previous["state"]["revision"].uint == 2)
        editor.receive(JSON(["unrecognized": true]))
        precondition(editor.state["revision"].uint == 3)

        var fixtureCount = 0
        for path in CommandLine.arguments.dropFirst() {
            let fixtures = try JSON.decode(String(contentsOfFile: path, encoding: .utf8))
            for fixture in fixtures.array {
                let old = model.json, oldKey = old.stableKey
                model.stage(fixture).forEach { $0.publish() }
                precondition(model.json.stableKey == fixture.stableKey && old.stableKey == oldKey)
                editor.receive(fixture)
                if !fixture["state"].isNull {
                    precondition(editor.snapshot.json.stableKey == fixture.stableKey)
                    precondition(editor.state.json.stableKey == fixture["state"].stableKey)
                    for value in fixture["state"]["commands"].array {
                        precondition(editor.command(value["id"].string).stableKey == value.stableKey)
                    }
                    for value in fixture["panels"].array {
                        precondition(editor.panel(value["id"].string).stableKey == value.stableKey)
                    }
                    for value in fixture["application_menus"].array {
                        precondition(editor.applicationMenu(value["id"].string).stableKey == value.stableKey)
                    }
                } else if !fixture["camera"].isNull {
                    precondition(editor.state["camera"].stableKey == fixture["camera"].stableKey)
                }
                fixtureCount += 1
            }
        }
        print("Snapshot observation checks passed: fields, whole reads, presence, indexes, numeric fidelity and \(fixtureCount) wire fixtures")
    }

    @MainActor private static func checkWorkspaceMotion() {
        let editor = EditorSnapshotState()
        func update(_ revision: Int, model: Int = 10, x: Int = 100, dragging: Bool = true) -> JSON {
            let bounds: [String: Any] = ["x": x, "y": 50, "width": 200, "height": 300]
            let drag: Any = dragging ? ["group": ["id": 7, "bounds": bounds],
                "tab": ["group": 7, "panel": "brushes", "preview": ["bounds": bounds]],
                "drop_hint": ["bounds": bounds]] : NSNull()
            return JSON(["revision": revision, "model_revision": model, "drag": drag])
        }
        func full(_ motion: JSON) -> JSON {
            JSON(["state": ["revision": motion["revision"].raw, "camera": ["zoom": 1],
                            "commands": [["id": "undo", "enabled": true]]],
                  "panels": [["id": "brushes", "label": "Brushes"]],
                  "application_menus": [["id": "edit", "enabled": true]],
                  "layout": ["groups": [["id": 7]]], "workspace_update": motion.raw])
        }
        precondition(editor.receive(JSON(["workspace_update": update(11).raw])) == .ignored,
                     "A presentation cannot establish its own missing models")
        precondition(editor.receive(full(update(10))) == .full)
        let retained = editor.snapshot.json
        let models = watch {
            _ = editor.state["revision"].raw; _ = editor.snapshot["layout"].raw
            _ = editor.command("undo").raw; _ = editor.panel("brushes").raw; _ = editor.applicationMenu("edit").raw
        }
        let otherGroup = watch { _ = editor.workspace.position(8).raw }
        let moved = watch { _ = editor.workspace.position(7).raw }
        let tab = watch { _ = editor.workspace.tab.raw }
        let identity = watch { _ = editor.workspace.tabIdentity.raw }
        let camera = watch { _ = editor.state["camera"].raw }
        precondition(editor.receive(JSON(["workspace_update": update(11, x: 120).raw])) == .workspace)
        precondition(moved.value == 1 && tab.value == 1 && identity.value == 0 && otherGroup.value == 0)
        precondition(camera.value == 0 && models.value == 0 && editor.state["revision"].uint == 10)
        precondition(editor.workspace.position(7)["x"].uint == 120 && retained["workspace_update"]["revision"].uint == 10)
        precondition(editor.receive(JSON(["workspace_update": update(12, x: 130).raw, "camera": ["zoom": 2]])) == .workspace)
        precondition(camera.value == 1 && models.value == 0 && editor.state["revision"].uint == 10)
        precondition(editor.snapshot["state"]["camera"]["zoom"].uint == 2 && editor.workspace.revision == 12)
        precondition(editor.receive(JSON(["camera": ["zoom": 3], "revision": 12])) == .camera)
        precondition(editor.state["revision"].uint == 10 && models.value == 0 && editor.workspace.revision == 12,
                     "A camera-only update after motion must not promote the retained model revision")
        let beforeRejected = editor.snapshot.json.stableKey
        let rejected = watch { _ = editor.workspace.position(7).raw; _ = editor.state["camera"].raw }
        for invalid in [update(11, x: 0), update(13, model: 9), update(13, model: 11)] {
            precondition(editor.receive(JSON(["workspace_update": invalid.raw, "camera": ["zoom": 99]])) == .ignored)
        }
        precondition(editor.receive(full(update(11, model: 9))) == .ignored)
        precondition(editor.receive(JSON(["workspace_update": ["revision": 13]])) == .ignored)
        precondition(rejected.value == 0 && editor.snapshot.json.stableKey == beforeRejected)
        let coherent = Changes()
        withObservationTracking({ _ = editor.workspace.position(7).raw }, onChange: {
            MainActor.assumeIsolated {
                if editor.state["revision"].uint == 13 && editor.workspace.modelRevision == 13
                    && editor.workspace.tab.isNull && editor.workspace.dropHint.isNull
                    && editor.workspace.position(7).isNull { coherent.record() }
            }
        })
        precondition(editor.receive(full(update(13, model: 13, dragging: false))) == .full)
        precondition(coherent.value == 1 && identity.value == 1 && otherGroup.value == 0)
        print("Workspace observation checks passed: retained models, per-group placement, tab visibility, optional camera, rejected revisions and coherent completion")
    }
}
