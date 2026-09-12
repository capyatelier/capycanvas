import Foundation

/// Shared iPad/Mac presentation state. Rust still supplies every value; these
/// projections only control which native views need to reevaluate their bodies.
@MainActor final class EditorSnapshotState {
    private let stateFields = SnapshotProjection()
    private let snapshotFields = SnapshotProjection()
    private let commands = SnapshotProjection()
    private let panels = SnapshotProjection()
    private let menus = SnapshotProjection()
    var state: SnapshotProjection { stateFields }
    var snapshot: SnapshotProjection { snapshotFields }
    func command(_ id: String) -> JSON { commands[id] }
    func panel(_ id: String) -> JSON { panels[id] }
    func applicationMenu(_ id: String) -> JSON { menus[id] }

    func receive(_ next: JSON) {
        var changes: [SnapshotSignal]
        if !next["state"].isNull {
            changes = stateFields.stage(next["state"])
            changes += snapshotFields.stage(next)
            changes += commands.stage(SnapshotProjection.indexed(next["state"]["commands"]))
            changes += panels.stage(SnapshotProjection.indexed(next["panels"]))
            changes += menus.stage(SnapshotProjection.indexed(next["application_menus"]))
        } else if !next["camera"].isNull {
            // A camera patch must remain cheap and cannot acknowledge or alter
            // any unrelated command, panel, menu or persistent workspace value.
            changes = stateFields.stagePatch(["camera": next["camera"], "revision": next["revision"]])
            changes += snapshotFields.stagePatch(["state": stateFields.unobserved])
        } else { return }
        // All reads, including those in synchronous observation callbacks, now
        // see the same complete revision across the five projections.
        changes.forEach { $0.publish() }
    }
}
