import Foundation

/// Absolute presentation for retained Rust models. Each group has its own
/// observation identity; motion never rewrites the panel/content projections.
@MainActor final class WorkspaceMotion {
    private let fields = SnapshotProjection()
    private let positions = SnapshotProjection()
    private(set) var modelRevision: UInt64?
    private(set) var contentRevision: UInt64?
    private(set) var revision: UInt64?
    var tab: JSON { fields["tab"] }
    var tabIdentity: JSON { fields["tab_identity"] }
    var dropHint: JSON { fields["drop_hint"] }
    func position(_ group: UInt64) -> JSON { positions[String(group)] }

    func accepts(_ update: JSON, full: Bool, reflow: Bool = false) -> Bool {
        if update.isNull { return full }
        guard !update["model_revision"].isNull, !update["revision"].isNull else { return false }
        if let revision, update["revision"].uint < revision { return false }
        if !full, let contentRevision {
            guard !update["content_revision"].isNull, contentRevision == update["content_revision"].uint else { return false }
        }
        if reflow {
            guard let contentRevision, let modelRevision, !update["content_revision"].isNull else { return false }
            return contentRevision == update["content_revision"].uint && update["model_revision"].uint >= modelRevision
        }
        return full || modelRevision == update["model_revision"].uint
    }
    func stage(_ update: JSON) -> [SnapshotSignal] {
        modelRevision = update.isNull ? nil : update["model_revision"].uint
        contentRevision = update["content_revision"].isNull ? nil : update["content_revision"].uint
        revision = update.isNull ? nil : update["revision"].uint
        let drag = update["drag"], group = drag["group"], tab = drag["tab"]
        let identity = tab.isNull ? JSON() : JSON(["group": tab["group"].raw, "panel": tab["panel"].raw])
        let groupPositions: [String: Any] = group.isNull ? [:] : [String(group["id"].uint): group["bounds"].raw]
        var changes = positions.stage(JSON(groupPositions))
        changes += fields.stage(JSON(["tab": tab.raw, "tab_identity": identity.raw, "drop_hint": drag["drop_hint"].raw]))
        return changes
    }
}
