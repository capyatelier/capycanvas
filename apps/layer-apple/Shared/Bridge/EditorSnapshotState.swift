import Foundation

/// Shared iPad/Mac presentation state. Rust still supplies every value; these
/// projections only control which native views need to reevaluate their bodies.
@MainActor final class EditorSnapshotState {
    private let stateFields = SnapshotProjection()
    private let snapshotFields = SnapshotProjection()
    private let commands = SnapshotProjection()
    private let panels = SnapshotProjection()
    private let menus = SnapshotProjection()
    let workspace = WorkspaceMotion()
    enum Update { case full, reflow, workspace, camera, ignored }
    var state: SnapshotProjection { stateFields }
    var snapshot: SnapshotProjection { snapshotFields }
    func command(_ id: String) -> JSON { commands[id] }
    func panel(_ id: String) -> JSON { panels[id] }
    func applicationMenu(_ id: String) -> JSON { menus[id] }

    @discardableResult func receive(_ next: JSON) -> Update {
        var changes: [SnapshotSignal]
        let full = !next["state"].isNull, motion = next["workspace_update"]
        let kind: Update
        if full {
            guard workspace.accepts(motion, full: true) else { return .ignored }
            changes = stateFields.stage(next["state"])
            changes += snapshotFields.stage(next)
            changes += commands.stage(SnapshotProjection.indexed(next["state"]["commands"]))
            changes += panels.stage(SnapshotProjection.indexed(next["panels"]))
            changes += menus.stage(SnapshotProjection.indexed(next["application_menus"]))
            changes += workspace.stage(motion)
            kind = .full
        } else if !motion.isNull {
            if !next["layout"].isNull || !next["workspace_layout"].isNull {
                guard workspace.accepts(motion, full: false, reflow: true),
                    let updatedWorkspace = reflowWorkspace(next) else { return .ignored }
                // Preserve the content revision and all control indexes. The
                // live workspace dimensions and camera become visible together.
                changes = stateFields.stagePatch(["workspace": updatedWorkspace, "camera": next["camera"]])
                changes += snapshotFields.stagePatch(["state": stateFields.unobserved, "layout": next["layout"],
                    "panel_measurements": next["panel_measurements"], "workspace_update": motion])
                changes += workspace.stage(motion)
                kind = .reflow
            } else {
                guard workspace.accepts(motion, full: false) else { return .ignored }
                changes = workspace.stage(motion)
                var patch = ["workspace_update": motion]
                if !next["camera"].isNull {
                    changes += stateFields.stagePatch(["camera": next["camera"]])
                    patch["state"] = stateFields.unobserved
                }
                // Ordinary placement retains the complete layout model. Neither
                // kind of geometry patch promotes state.revision or wakes its
                // content readers, such as layer thumbnails.
                changes += snapshotFields.stagePatch(patch)
                kind = .workspace
            }
        } else if !next["camera"].isNull {
            // A camera patch must remain cheap and cannot acknowledge or alter
            // any unrelated command, panel, menu or persistent workspace value.
            var patch = ["camera": next["camera"]]
            if workspace.modelRevision == nil { patch["revision"] = next["revision"] }
            changes = stateFields.stagePatch(patch)
            changes += snapshotFields.stagePatch(["state": stateFields.unobserved])
            kind = .camera
        } else { return .ignored }
        // All reads, including those in synchronous observation callbacks, now
        // see coherent retained models and their matching presentation.
        changes.forEach { $0.publish() }
        return kind
    }
    private func reflowWorkspace(_ next: JSON) -> JSON? {
        let current = stateFields.unobserved["workspace"], layout = next["layout"]
        guard current["layout"].raw is NSDictionary, layout.raw is NSDictionary,
            next["camera"].raw is NSDictionary, next["panel_measurements"].raw is NSArray,
            ["groups", "dividers", "collapsed", "reveal_edges", "viewport"].allSatisfy({ layout[$0].raw is NSArray }),
            layout["work_area"].raw is NSDictionary, layout["status"].raw is NSDictionary,
            layout["tab_bar_height"].raw is NSNumber else { return nil }
        var dimensions = current["layout"].object
        for key in ["bands", "floating", "collapsed", "fit_tab_groups"] {
            guard next["workspace_layout"][key].raw is NSArray else { return nil }
            dimensions[key] = next["workspace_layout"][key].raw
        }
        return current.replacing("layout", with: JSON(dimensions))
    }
}
