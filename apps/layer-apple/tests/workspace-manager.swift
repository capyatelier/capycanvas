import Foundation
import AppKit
import SwiftUI

@main struct WorkspaceManagerChecks {
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-manager-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let storage = EditorPersistence(root: root)
            let editor = EditorStore(platform: platform, persistence: storage)
            let workspaces = editor.workspaces!
            try await workspaces.started()
            let original = workspaces.view["id"].string
            let defaults = try await workspaces.workspaceRows()
            precondition(defaults.map { $0["title"].string } == ["Sketch", "Paint", "Photo"])
            precondition(original == defaults[1]["id"].string)
            try await editor.apply(["type": "workspace_manager", "command": ["type": "manage"]])
            try await wait("manager request acknowledgement") {
                workspaces.page == "workspaces" && !workspaces.busy
                    && !editor.state["requests"].array.contains { $0["kind"]["type"].string == "workspace" }
            }
            precondition(workspaces.view["selected"].string == original)
            try await capture(workspaces, platform: platform, phase: "browser")
            precondition(!workspaces.view["rows"].array.contains { row in
                row["actions"].array.contains { ["rename", "delete"].contains($0["action"]["type"].string) && $0["enabled"].bool }
            }, "Included workspaces keep their names and cannot be deleted")
            try await workspaces.perform(["type": "form", "action": ["type": "new"]])
            precondition(workspaces.view["prompt"]["choices"].array.isEmpty && !workspaces.view["prompt"]["message"].string.isEmpty)
            workspaces.formName = "Cancelled"; workspaces.cancelPrompt()
            try await workspaces.settle()
            precondition(workspaces.view["id"].string == original && workspaces.view["prompt"].isNull)
            try await workspaces.create("Inking")
            let inking = workspaces.view["id"].string
            precondition(inking != original && !workspaces.presented)
            try await editor.apply(["type": "set_brush_size", "value": 31])
            try await editor.apply(["type": "customize", "action": ["type": "set_panel_visible", "panel": "sizes", "visible": false]])
            let source = try await workspaces.capture()
            try await workspaces.perform(["type": "form", "action": ["type": "new"]])
            workspaces.formName = "Inking"; workspaces.submit()
            try await wait("inline validation") { workspaces.error != nil && !workspaces.busy }
            precondition(!workspaces.view["prompt"].isNull && workspaces.formName == "Inking", "Validation must retain the entered name")
            workspaces.formName = "Sketching"; workspaces.submit()
            try await workspaces.settle()
            let sketching = workspaces.view["id"].string
            precondition(sketching != inking && workspaces.view["prompt"].isNull)
            let created = try await workspaces.capture()
            precondition(SnapshotProjection.equal(created["working"].raw, source["working"].raw))
            let sourceRevision = source["history"]["current"].string, createdRevision = created["history"]["current"].string
            precondition(SnapshotProjection.equal(created["history"]["revisions"][createdRevision]["layout"].raw,
                source["history"]["revisions"][sourceRevision]["layout"].raw))
            precondition(created["history"]["undo"].array.isEmpty, "A new workspace starts independent history")
            try await workspaces.perform(["type": "switch", "id": original])
            try await editor.apply(["type": "set_brush_size", "value": 67])
            try await editor.apply(["type": "customize", "action": ["type": "set_panel_visible", "panel": "navigator", "visible": false]])
            try await selectionPreview(editor, target: inking)
            editor.input(["type": "pointer", "id": 900, "phase": "down", "kind": "pen", "button": "primary", "position": [500, 400]])
            try await workspaces.answer(["type": "rename", "value": inking], name: "Inking v2")
            try require(await workspaces.workspaceRows().contains { $0["title"].string == "Inking v2" }, "Renaming during a pen contact keeps the new name")
            editor.input(["type": "pointer", "id": 900, "phase": "up", "kind": "pen", "button": "primary", "position": [500, 400]])
            try await startingLayoutPreview(editor)
            precondition(editor.state["brush"]["diameter"].number == 67)
            try await liveHistory(editor, platform: platform)
            let beforeReset = try await workspaces.capture()
            let layersBeforeReset = editor.state["layers"].stableKey
            precondition(!beforeReset["working"]["tools"]["overrides"].object.isEmpty)
            try await workspaces.perform(["type": "form", "action": ["type": "reset_brushes"]])
            workspaces.cancelPrompt(); try await workspaces.settle()
            try require(SnapshotProjection.equal(await workspaces.capture().raw, beforeReset.raw), "Cancelling Reset Brushes leaves the workspace unchanged")
            try await workspaces.answer(["type": "reset_brushes"])
            let afterReset = try await workspaces.capture()
            precondition(afterReset["working"]["tools"]["overrides"].object.isEmpty)
            precondition(SnapshotProjection.equal(afterReset["history"].raw, beforeReset["history"].raw))
            precondition(editor.state["layers"].stableKey == layersBeforeReset)
            try await workspaces.flushed()
            let other = EditorStore(platform: platform, persistence: storage, managedWorkspaces: true)
            try await other.workspaces!.started("other window")
            try await other.workspaces!.perform(["type": "switch", "id": defaults[0]["id"].string])
            let otherID = other.workspaces!.view["id"].string
            var focused = false
            other.focusWindow = { focused = true }
            try await workspaces.perform(["type": "action", "action": ["type": "switch_to_window", "value": otherID]])
            try await wait("focus other window") { focused }
            focused = false
            try await workspaces.perform(["type": "switch", "id": otherID])
            try await wait("focus claimed workspace") { focused }
            precondition(workspaces.view["id"].string == original,
                "The header switch action must focus a claimed workspace without taking it over")
            try await toolbarActions(editor)
            try await workspaces.perform(["type": "switch", "id": sketching])
            try await workspaces.perform(["type": "form", "action": ["type": "delete", "value": sketching]])
            let deletion = workspaces.view["prompt"]
            precondition(deletion["choices"].array.isEmpty && deletion["message"].string.contains("permanent"))
            let orderBeforeDelete = workspaces.view["order"].array.map(\.string)
            workspaces.submit(); try await workspaces.settle()
            precondition(workspaces.view["id"].string == "builtin:workspace:illustrator")
            precondition(workspaces.view["order"].array.map(\.string) == orderBeforeDelete.filter { $0 != sketching },
                "Deleting the active workspace must not create an extra replacement")
            try await invalidPackage(workspaces, root: root)
            let allowed: Bool = await withCheckedContinuation { continuation in editor.projectFiles.confirmClose { continuation.resume(returning: $0) } }
            precondition(allowed)
            let prepared: Bool = await withCheckedContinuation { continuation in editor.prepareClose { continuation.resume(returning: $0) } }
            precondition(prepared && workspaces.view["closed"].bool, "A prepared close releases the workspace")
            await withCheckedContinuation { continuation in editor.cancelPreparedClose { continuation.resume() } }
            other.systemSceneID = UUID().uuidString
            EditorStore.discardSceneSessions([other.systemSceneID!])
            try await wait("discarded scene release") { other.workspaces!.view["closed"].bool }
            let successor = EditorStore(platform: platform, persistence: storage, managedWorkspaces: true)
            try await successor.workspaces!.started("successor window")
            try await successor.workspaces!.perform(["type": "switch", "id": otherID])
            precondition(successor.workspaces!.view["id"].string == otherID, "A discarded scene must release its claim")
            try await successor.workspaces!.closed()
            print("PASS: platform \(platform), manager host commands, forms, inline validation, workspace previews/history, window focus, toolbar actions, deletion and scene release")
        }
    }
    @MainActor static func selectionPreview(_ editor: EditorStore, target: String) async throws {
        let workspaces = editor.workspaces!
        let before = try await workspaces.capture(), layout = editor.state["workspace"]["layout"]
        let active = workspaces.view["id"].string
        try await workspaces.perform(["type": "open", "page": "workspaces"])
        precondition(workspaces.view["selected"].string == active)
        try await workspaces.perform(["type": "select", "id": target])
        precondition(!SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, layout.raw), "Selecting a row previews its layout")
        precondition(workspaces.view["id"].string == active)
        try require(SnapshotProjection.equal(await workspaces.capture().raw, before.raw), "Selecting a row must not apply or persist its layout")
        try await workspaces.perform(["type": "search", "query": "no matching workspace"])
        precondition(workspaces.view["selected"].isNull && workspaces.view["details"].isNull && workspaces.view["rows"].array.isEmpty)
        precondition(SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, layout.raw))
        try await workspaces.perform(["type": "search", "query": ""])
        precondition(workspaces.view["selected"].isNull && !workspaces.view["rows"].array.isEmpty)
        workspaces.select(target); workspaces.select(active)
        try await workspaces.settle("rapid selection")
        precondition(workspaces.view["selected"].string == active)
        workspaces.select(target)
        try await workspaces.perform(["type": "dismiss"])
        precondition(SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, layout.raw))
    }
    @MainActor static func liveHistory(_ editor: EditorStore, platform: UInt32) async throws {
        let workspaces = editor.workspaces!
        let id = workspaces.view["id"].string
        let before = try await workspaces.capture()
        let document = editor.state["layers"].stableKey
        let current = before["history"]["current"].string
        let revisions = before["history"]["revisions"].object
        let earlier = revisions.keys.first { key in
            key != current && !SnapshotProjection.equal(JSON(revisions[key]!)["layout"].raw, JSON(revisions[current]!)["layout"].raw)
        }!
        func browse() async throws {
            try await workspaces.perform(["type": "action", "action": ["type": "history", "value": id]])
            precondition(workspaces.page == "history" && workspaces.view["selected"].string == current)
            try await workspaces.perform(["type": "select", "id": earlier])
        }
        try await browse()
        try require(SnapshotProjection.equal(await workspaces.capture().raw, before.raw), "Preview must never enter the durable capture")
        precondition(!SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, JSON(revisions[current]!)["layout"].raw))
        precondition(editor.state["layers"].stableKey == document)
        try await capture(workspaces, platform: platform, phase: "history")
        try await workspaces.perform(["type": "dismiss"])
        try require(SnapshotProjection.equal(await workspaces.capture().raw, before.raw), "Dismissing history leaves the workspace unchanged")
        try await browse()
        workspaces.suspend()
        try await wait("suspended history cancellation") { !workspaces.presented && !workspaces.busy }
        workspaces.resume()
        try await workspaces.settle()
        try require(SnapshotProjection.equal(await workspaces.capture().raw, before.raw), "Suspending history browsing leaves the workspace unchanged")
        try await browse()
        try await workspaces.perform(["type": "confirm"])
        precondition(!workspaces.presented)
        let restored = try await workspaces.capture()
        precondition(restored["history"]["undo"].array.count == before["history"]["undo"].array.count + 1)
        precondition(SnapshotProjection.equal(restored["working"].raw, before["working"].raw))
        precondition(workspaces.view["id"].string == id && editor.state["layers"].stableKey == document)
        try await editor.apply(["type": "invoke", "command": "undo_workspace"])
        try require(await workspaces.capture()["history"]["current"].string == current, "Undo Workspace returns to the current history entry")
    }
    @MainActor static func startingLayoutPreview(_ editor: EditorStore) async throws {
        let workspaces = editor.workspaces!
        let id = workspaces.view["id"].string
        let before = try await workspaces.capture()
        let original = editor.state["workspace"]["layout"].stableKey
        let layers = editor.state["layers"].stableKey
        var baseline = ""
        for response in ["cancel", "dismiss", "restore"] {
            try await workspaces.perform(["type": "form", "action": ["type": "reset", "value": id]])
            precondition(workspaces.view["prompt"]["confirm"].string == "Restore")
            baseline = editor.state["workspace"]["layout"].stableKey
            precondition(baseline != original, "The starting layout must be visible before its confirmation")
            try require(SnapshotProjection.equal(await workspaces.capture().raw, before.raw),
                "Starting-layout preview must not enter persistence or history")
            precondition(editor.state["layers"].stableKey == layers)
            if response == "cancel" { workspaces.cancelPrompt() }
            else if response == "dismiss" { workspaces.dismiss() }
            else { workspaces.submit() }
            try await workspaces.settle()
            precondition(!workspaces.presented)
            if response != "restore" {
                precondition(editor.state["workspace"]["layout"].stableKey == original)
                try require(SnapshotProjection.equal(await workspaces.capture().raw, before.raw), "Declining the history prompt leaves the workspace unchanged")
            }
        }
        let restored = try await workspaces.capture()
        precondition(editor.state["workspace"]["layout"].stableKey == baseline)
        precondition(restored["history"]["undo"].array.count == before["history"]["undo"].array.count + 1)
        precondition(SnapshotProjection.equal(restored["working"].raw, before["working"].raw))
        try await editor.apply(["type": "invoke", "command": "undo_workspace"])
        precondition(editor.state["workspace"]["layout"].stableKey == original)
        try await editor.apply(["type": "invoke", "command": "redo_workspace"])
        precondition(editor.state["workspace"]["layout"].stableKey == baseline)
        precondition(editor.state["layers"].stableKey == layers)
    }
    @MainActor static func toolbarActions(_ editor: EditorStore) async throws {
        let workspaces = editor.workspaces!
        try await workspaces.answer(["type": "new_toolbar", "value": NSNull()], name: "Ink Tools")
        try await workspaces.perform(["type": "open", "page": "this_workspace"])
        let toolbar = workspaces.view["rows"].array.first { $0["title"].string == "Ink Tools" }!
        let panel = try JSON.decode(toolbar["id"].string)
        try await workspaces.answer(["type": "save_toolbar", "value": panel.raw], name: "Saved Ink")
        try await workspaces.perform(["type": "open", "page": "toolbar_library"])
        let savedToolbar = workspaces.view["rows"].array.first { $0["title"].string == "Saved Ink" }!["id"].string
        try await workspaces.perform(["type": "action", "action": ["type": "add_toolbar", "value": savedToolbar]])
        try await workspaces.answer(["type": "replace_toolbar", "value": panel.raw], choice: savedToolbar)
        try await workspaces.answer(["type": "update_toolbar", "value": savedToolbar], choice: panel.stableKey)
        for action in ["rename_toolbar", "duplicate_toolbar", "delete_toolbar"] {
            try await workspaces.perform(["type": "open", "page": "this_workspace"])
            try await workspaces.perform(["type": "action", "action": ["type": action, "value": panel.raw]])
            precondition(!workspaces.presented)
            try await wait("shared toolbar prompt") { !editor.snapshot["toolbar_prompt"].isNull }
            if action == "delete_toolbar" {
                precondition(editor.snapshot["toolbar_prompt"]["message"].string.contains(workspaces.view["name"].string))
            } else {
                try await editor.apply(["type": "customize", "action": ["type": "toolbar_name", "name": action == "rename_toolbar" ? "Renamed Ink" : "Copied Ink"]])
            }
            try await editor.apply(["type": "customize", "action": ["type": "confirm_toolbar"]])
            precondition(editor.snapshot["toolbar_prompt"].isNull)
            try await workspaces.flushed()
            try await workspaces.perform(["type": "open", "page": "this_workspace"])
            let titles = workspaces.view["rows"].array.map { $0["title"].string }
            precondition(action == "rename_toolbar" ? titles.contains("Renamed Ink")
                : action == "duplicate_toolbar" ? titles.contains("Copied Ink") : !titles.contains("Renamed Ink"))
        }
        try await workspaces.perform(["type": "dismiss"])
    }
    @MainActor static func invalidPackage(_ workspaces: WorkspaceController, root: URL) async throws {
        let destination = root.appendingPathComponent("broken.capytoolbar")
        let bytes = Data("broken package".utf8)
        try bytes.write(to: destination)
        workspaces.openURL(destination, kind: .toolbar)
        try await wait("invalid external toolbar import") { workspaces.error != nil && !workspaces.busy }
        try require(Data(contentsOf: destination) == bytes, "Import must not modify the opened package")
        workspaces.dismiss()
        workspaces.send(["type": "cancel"])
        try await workspaces.settle()
    }
    @MainActor static func capture(_ workspaces: WorkspaceController, platform: UInt32, phase: String) async throws {
        guard let path = ProcessInfo.processInfo.environment["CAPY_WORKSPACE_CAPTURES"] else { return }
        let directory = URL(fileURLWithPath: path, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        _ = NSApplication.shared
        for width: CGFloat in [360, 920] {
            for dark in [false, true] {
                let size = CGSize(width: width, height: 680)
                let window = NSWindow(contentRect: CGRect(origin: .zero, size: size), styleMask: [.borderless], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
                defer { window.close() }
                let host = NSHostingView(rootView: WorkspaceManagerView(workspaces: workspaces)
                    .environment(\.colorScheme, dark ? .dark : .light)
                    .frame(width: size.width, height: size.height).background(Color(nsColor: .windowBackgroundColor)))
                window.contentView = host
                try await Task.sleep(for: .milliseconds(150))
                host.layoutSubtreeIfNeeded()
                guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { throw HostFailure(message: "No manager bitmap") }
                window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                let name = "manager-\(platform)-\(phase)-\(Int(width))-\(dark ? "dark" : "light").png"
                try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent(name))
            }
        }
    }
}
