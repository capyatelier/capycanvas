// Actual Swift routing, Rust manager, SQLite and preview ownership on both
// presets. Native gesture delivery and UIKit pixels are separate checks.
import Foundation
import CoreGraphics

@main struct WorkspaceSwitcherChecks {
    @MainActor static func measuredRows() {
        let model = WorkspaceRowInteraction()
        let rows = ["a", "b", "c", "d"].map { JSON(["id": $0, "title": $0]) }
        model.update(items: rows, enabled: true)
        model.viewport = CGRect(x: 0, y: 32, width: 300, height: 64)
        model.frames = ["b": WorkspaceRowFrame(row: CGRect(x: 0, y: 32, width: 300, height: 32),
            grip: CGRect(x: 0, y: 32, width: 20, height: 32), options: CGRect(x: 274, y: 32, width: 26, height: 32)),
            "c": WorkspaceRowFrame(row: CGRect(x: 0, y: 64, width: 300, height: 32))]
        var drops: [(String, String?)] = []
        model.commit = { drops.append(($0, $1)) }
        precondition(model.source(at: CGPoint(x: 285, y: 48)) == nil, "Options own their ordinary click")
        precondition(model.source(at: CGPoint(x: 10, y: 48))?.surface == .handle)
        let origin = CGPoint(x: 150, y: 48), destination = CGPoint(x: 150, y: 94)
        model.contact.prepare(model.source(at: origin)!, device: .pen, origin: origin)
        precondition(!model.contact.move(to: destination), "Pen rows must not steal scrolling before a hold")
        model.contact.recognizeHold()
        precondition(model.menu == "b" && drops.isEmpty)
        model.contact.move(to: destination)
        precondition(model.menu == nil && model.hint?.before == "d", "An unmounted successor remains the insertion target")
        model.frames.removeValue(forKey: "b") // The captured source scrolls out of the lazy stack.
        model.contact.release(at: destination)
        precondition(drops.count == 1 && drops[0].0 == "b" && drops[0].1 == "d")
        precondition(model.contact.consumeClick(), "A held drop must suppress row selection")
        let next = CGPoint(x: 150, y: 80)
        model.contact.prepare(model.source(at: next)!, device: .touch, origin: next)
        model.contact.recognizeHold(); model.contact.move(to: destination)
        model.update(items: rows.filter { $0["id"].string != "c" }, enabled: true)
        model.contact.release(at: destination)
        precondition(drops.count == 1 && model.contact.target == nil && model.drag == nil && model.menu == nil,
            "Removing the source must retire capture, feedback and the pending drop")
        print("PASS: measured rows retain offscreen drop order/contact, grip priority, child clicks and source invalidation")
    }
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(20)
        while !ready() {
            guard Date() < deadline else { throw HostFailure(message: "Timed out: \(label)") }
            try await Task.sleep(for: .milliseconds(5))
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
    @MainActor static func preference(_ manager: WorkspaceManager, row: String, action: String) async throws {
        let item = manager.view["rows"].array.first { $0["id"].string == row }!
        let control = item["switcher_actions"].array.first { $0["id"].string == action }!
        precondition(control["enabled"].bool)
        try await manager.run(control["action"])
    }
    @MainActor static func main() async throws {
        measuredRows()
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-switcher-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let scene = UUID().uuidString
            let editor = EditorStore(platform: platform, scene: scene, persistence: EditorPersistence(root: root))
            let library = editor.workspaceLibrary!, manager = editor.workspaceManager
            try await wait("startup") { library.ready || library.error != nil }
            precondition(library.ready, library.error ?? "")
            let original = library.status["active_id"].string
            let painter = library.status["default_workspaces"][0]["id"].string
            let before = try await capture(editor)
            let document = editor.state["layers"].stableKey
            try await manager.show("workspaces")
            manager.select(painter)
            try await wait("preview") { !manager.selecting && library.previewingLayout }
            let preview = editor.state["workspace"]["layout"].stableKey
            try await preference(manager, row: original, action: "pin")
            precondition(library.previewingLayout && library.busy && manager.selection == painter)
            precondition(editor.state["workspace"]["layout"].stableKey == preview)
            precondition(!library.status["switcher"].array.contains { $0["id"].string == original })
            precondition(library.status["switcher_display"][0]["id"].string == original)
            try await preference(manager, row: original, action: "down")
            precondition(library.previewingLayout && manager.selection == painter)
            precondition(library.status["order"].array.last?.string == original)
            let during = try await capture(editor)
            precondition(SnapshotProjection.equal(during["capture"].raw, before["capture"].raw))
            precondition(editor.state["layers"].stableKey == document)
            manager.presented = false; manager.dismissed()
            try await wait("preview Cancel") { !library.previewingLayout && !library.busy }
            let cancelled = try await capture(editor)
            precondition(SnapshotProjection.equal(cancelled["capture"].raw, before["capture"].raw))
            try await manager.run(JSON(["type": "switch", "value": painter]))
            precondition(!library.status["switcher_display"].array.contains { $0["id"].string == original })
            try await manager.run(JSON(["type": "switch", "value": original]))
            precondition(library.status["switcher_display"][0]["id"].string == original)

            // A second real window coordinator shares preference notifications,
            // but retains its own active identity and its own canvas preview.
            let other = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            let second = other.workspaceLibrary!, secondManager = other.workspaceManager
            try await wait("second owner") { second.ready || second.error != nil }
            precondition(second.ready, second.error ?? "")
            let secondID = second.status["active_id"].string
            precondition(secondID != original)
            try await wait("created workspace notification") { library.status["switcher"].array.contains { $0["id"].string == secondID } }
            try await secondManager.show("workspaces")
            secondManager.select(painter)
            try await wait("second preview") { !secondManager.selecting && second.previewingLayout }
            let secondPreview = other.state["workspace"]["layout"].stableKey
            try await manager.show("workspaces")
            try await preference(manager, row: original, action: "pin")
            try await wait("pin broadcast") { second.status["switcher"].array.contains { $0["id"].string == original } }
            precondition(second.previewingLayout && secondManager.selection == painter)
            precondition(other.state["workspace"]["layout"].stableKey == secondPreview)
            precondition(second.status["active_id"].string == secondID)
            let revision = second.status["switcher_revision"].uint
            try await second.refreshSwitcher()
            precondition(second.status["switcher_revision"].uint == revision, "Refresh must not rebroadcast a preference write")
            do {
                try await library.editSwitcher(JSON(["type": "move", "id": "missing", "before": NSNull()]))
                preconditionFailure("A removed source must fail")
            } catch { precondition(!library.readOnly, "Preference failures must not revoke workspace ownership") }
            secondManager.presented = false; secondManager.dismissed()
            try await wait("second Cancel") { !second.previewingLayout && !second.busy }
            try await second.close()
            let pins = library.status["switcher"].stableKey, order = library.status["order"].stableKey
            manager.presented = false; manager.dismissed()
            try await library.close()
            let reopened = EditorStore(platform: platform, scene: scene, persistence: EditorPersistence(root: root))
            let restored = reopened.workspaceLibrary!
            try await wait("restart") { restored.ready || restored.error != nil }
            precondition(restored.ready, restored.error ?? "")
            precondition(restored.status["active_id"].string == original)
            precondition(restored.status["switcher"].stableKey == pins && restored.status["order"].stableKey == order)
            try await restored.close()
            print("PASS platform \(platform): pin/order actions, unpinned current, preview/Cancel, unchanged document/history, cross-window notifications, invalid drops and restart")
        }
    }
}
