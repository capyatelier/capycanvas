import Foundation
import AppKit
import SwiftUI

@main struct WorkspaceManagerChecks {
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
                if let error { continuation.resume(throwing: HostFailure(message: error)) } else { continuation.resume() }
            }
        }
    }
    @MainActor static func form(_ manager: WorkspaceManager, _ action: [String: Any], name: String? = nil,
        choice: String? = nil, retryName: String? = nil, cancel: Bool = false) async throws {
        var finished = false
        let task = Task { @MainActor in defer { finished = true }; try await manager.run(JSON(action)) }
        try await wait("workspace form") { manager.prompt != nil || finished }
        if finished { try await task.value; throw HostFailure(message: "Expected a workspace form") }
        precondition(!(manager.prompt?["message"].string ?? "").isEmpty)
        if let name { manager.formName = name }; if let choice { manager.formChoice = choice }
        manager.answer(confirm: !cancel)
        if let retryName {
            try await wait("inline validation") { manager.prompt != nil && manager.formError != nil || finished }
            precondition(!finished && manager.formName == name, "Validation must retain the entered name")
            manager.formName = retryName; manager.answer(confirm: true)
        }
        try await task.value
    }
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-manager-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let storage = EditorPersistence(root: root)
            let editor = EditorStore(platform: platform, persistence: storage)
            try await wait("workspace startup") { editor.workspaceLibrary?.ready == true || editor.workspaceLibrary?.error != nil }
            let library = editor.workspaceLibrary!, manager = editor.workspaceManager
            precondition(library.ready, library.error ?? "Startup failed")
            let original = library.status["active_id"].string
            let defaults = library.status["default_workspaces"].array
            precondition(defaults.map { $0["name"].string } == ["Painter", "Illustrator", "Photographer"])
            precondition(original == defaults[1]["id"].string)
            // Exercise the actual host request, rather than assuming the menu's
            // visible label means the editor/service action is connected.
            try await edit(editor, ["type": "workspace_manager", "command": ["type": "manage"]])
            try await wait("manager request acknowledgement") {
                manager.presented && !manager.view.isNull && !editor.state["requests"].array.contains { $0["kind"]["type"].string == "workspace" }
            }
            precondition(manager.selection == original)
            try await capture(manager, platform: platform, phase: "browser")
            let currentButton = manager.view["details"]["actions"].array.first { $0["action"]["type"].string == "switch" }!
            precondition(!currentButton["enabled"].bool)
            try await form(manager, ["type": "new"], name: "Cancelled", cancel: true)
            precondition(library.status["active_id"].string == original)
            let newPrompt = try await library.read(["type": "prompt", "action": ["type": "new"]])
            precondition(newPrompt["prompt"]["choices"].array.isEmpty)
            try await form(manager, ["type": "new"], name: "Inking")
            let inking = library.status["active_id"].string
            precondition(inking != original && !manager.presented)
            try await edit(editor, ["type": "set_brush_size", "value": 31])
            try await edit(editor, ["type": "customize", "action": ["type": "set_panel_visible", "panel": "sizes", "visible": false]])
            let source = try await captureSession(editor)
            try await form(manager, ["type": "new"], name: "Inking", retryName: "Sketching")
            let sketching = library.status["active_id"].string
            let created = try await captureSession(editor)
            precondition(SnapshotProjection.equal(created["capture"]["working"].raw, source["capture"]["working"].raw))
            let sourceRevision = source["capture"]["history"]["current"].string
            let createdRevision = created["capture"]["history"]["current"].string
            precondition(SnapshotProjection.equal(created["capture"]["history"]["revisions"][createdRevision]["layout"].raw,
                source["capture"]["history"]["revisions"][sourceRevision]["layout"].raw))
            precondition(created["capture"]["history"]["undo"].array.isEmpty, "A new workspace starts independent history")
            try await manager.run(JSON(["type": "switch", "value": original]))
            try await edit(editor, ["type": "set_brush_size", "value": 67])
            try await edit(editor, ["type": "customize", "action": ["type": "set_panel_visible", "panel": "navigator", "visible": false]])
            try await selectionPreview(editor, page: "workspaces", target: inking, cancel: true)
            try await manager.show("workspaces")
            // Editing library metadata must remain available while a canvas
            // contact is held, and must not cancel that interaction.
            editor.input(["type": "pointer", "id": 900, "phase": "down", "kind": "pen", "button": "primary", "position": [500, 400]])
            let held = try await captureSession(editor)
            precondition(!held["idle"].bool)
            try await form(manager, ["type": "rename", "value": inking], name: "Inking v2")
            let afterRename = try await captureSession(editor)
            precondition(!afterRename["idle"].bool)
            do { _ = try await library.operation(["type": "new", "name": "During Contact"]); preconditionFailure("Switching must require an idle canvas") }
            catch { }
            editor.input(["type": "pointer", "id": 900, "phase": "up", "kind": "pen", "button": "primary", "position": [500, 400]])
            let ended = try await captureSession(editor)
            precondition(ended["idle"].bool)
            try await manager.run(JSON(["type": "metadata", "value": inking]))
            precondition(!manager.history["rows"].array.isEmpty && !manager.history["restore"].isNull)
            manager.historyAction()
            try await wait("metadata restore form") { manager.prompt != nil }
            manager.answer(confirm: true)
            try await wait("metadata restored") { !manager.processing && manager.prompt == nil }
            let restored = try await library.read(["type": "load", "id": inking])
            precondition(restored["entity"]["metadata"]["name"].string == "Inking")
            manager.back()
            try await form(manager, ["type": "reset", "value": original])
            precondition(editor.state["brush"]["diameter"].number == 67)
            try await liveHistory(editor, platform: platform)
            let beforeReset = try await captureSession(editor)
            let layersBeforeReset = editor.state["layers"].stableKey
            precondition(!beforeReset["capture"]["working"]["tools"]["overrides"].object.isEmpty)
            try await form(manager, ["type": "reset_brushes"], cancel: true)
            let canceledReset = try await captureSession(editor)
            precondition(SnapshotProjection.equal(canceledReset["capture"].raw, beforeReset["capture"].raw))
            try await form(manager, ["type": "reset_brushes"])
            let afterReset = try await captureSession(editor)
            precondition(afterReset["capture"]["working"]["tools"]["overrides"].object.isEmpty)
            precondition(SnapshotProjection.equal(afterReset["capture"]["history"].raw, beforeReset["capture"]["history"].raw))
            precondition(editor.state["layers"].stableKey == layersBeforeReset)
            let storedReset = try await library.read(["type": "load", "id": original])
            precondition(storedReset["entity"]["working"]["tools"]["overrides"].object.isEmpty)
            let untouched = try await library.read(["type": "load", "id": inking])["entity"]["working"]
            precondition(untouched["tools"]["overrides"][String(untouched["preset"].uint)]["size"].number == 31)
            try await form(manager, ["type": "rename", "value": defaults[0]["id"].raw], name: "Paint")
            precondition(library.status["default_workspaces"][0]["name"].string == "Paint")
            // A live source window's most recent edit must be copied before
            // its debounce timer writes. The source remains independently open.
            let other = EditorStore(platform: platform, persistence: storage, managedWorkspaces: true)
            try await wait("other window") { other.workspaceLibrary!.ready }
            _ = try await other.workspaceLibrary!.operation(["type": "switch", "id": defaults[0]["id"].raw])
            let otherID = other.workspaceLibrary!.status["active_id"].string
            try await edit(other, ["type": "set_brush_size", "value": 113])
            var focused = false
            other.focusWindow = { focused = true }
            try await manager.run(JSON(["type": "switch_to_window", "value": otherID]))
            precondition(focused)
            focused = false
            try await manager.run(JSON(["type": "switch", "value": otherID]))
            precondition(focused && library.status["active_id"].string == original,
                "The header switch action must focus a claimed default workspace without taking it over")
            try await form(manager, ["type": "duplicate", "value": otherID], name: "Other Window Copy")
            precondition(editor.state["brush"]["diameter"].number == 113 && other.workspaceLibrary!.status["active_id"].string == otherID)
            try await manager.run(JSON(["type": "switch", "value": original]))
            try await form(manager, ["type": "new_toolbar"], name: "Ink Tools")
            try await manager.show("this_workspace")
            let toolbar = manager.view["rows"].array.first { $0["title"].string == "Ink Tools" }!
            let panel = try JSON.decode(toolbar["id"].string)
            try await form(manager, ["type": "save_toolbar", "value": panel.raw], name: "Saved Ink")
            let savedToolbar = manager.selection!
            precondition(manager.page == "toolbar_library")
            try await manager.run(JSON(["type": "add_toolbar", "value": savedToolbar]))
            try await form(manager, ["type": "replace_toolbar", "value": panel.raw], choice: savedToolbar)
            try await form(manager, ["type": "update_toolbar", "value": savedToolbar], choice: panel.stableKey)
            // The manager hands local toolbar edits to the existing shared
            // prompt after dismissing its sheet; the resulting actions matter.
            for action in ["rename_toolbar", "duplicate_toolbar", "delete_toolbar"] {
                try await manager.show("this_workspace")
                try await manager.run(JSON(["type": action, "value": panel.raw]))
                precondition(!manager.presented)
                manager.dismissed()
                try await wait("shared toolbar prompt") { !editor.snapshot["toolbar_prompt"].isNull }
                if action == "delete_toolbar" {
                    precondition(editor.snapshot["toolbar_prompt"]["message"].string.contains(library.status["name"].string))
                } else {
                    try await edit(editor, ["type": "customize", "action": ["type": "toolbar_name", "name": action == "rename_toolbar" ? "Renamed Ink" : "Copied Ink"]])
                }
                try await edit(editor, ["type": "customize", "action": ["type": "confirm_toolbar"]])
                precondition(editor.snapshot["toolbar_prompt"].isNull)
                try await library.flush()
                try await manager.show("this_workspace")
                let titles = manager.view["rows"].array.map { $0["title"].string }
                precondition(action == "rename_toolbar" ? titles.contains("Renamed Ink")
                    : action == "duplicate_toolbar" ? titles.contains("Copied Ink") : !titles.contains("Renamed Ink"))
            }
            try await form(manager, ["type": "delete", "value": sketching])
            precondition(manager.undoDeletion == sketching)
            try await manager.run(JSON(["type": "restore_deleted", "value": sketching]))
            precondition(manager.undoDeletion == nil)
            try await manager.run(JSON(["type": "switch", "value": sketching]))
            try await form(manager, ["type": "delete", "value": sketching], choice: inking)
            precondition(library.status["active_id"].string == inking)
            try await manager.run(JSON(["type": "restore_deleted", "value": sketching]))
            try await packageDelivery(editor: editor, root: root, toolbar: savedToolbar)
            let allowed: Bool = await withCheckedContinuation { continuation in editor.projectFiles.confirmClose { continuation.resume(returning: $0) } }
            precondition(allowed)
            let prepared: Bool = await withCheckedContinuation { continuation in editor.prepareClose { continuation.resume(returning: $0) } }
            precondition(prepared && !library.ready)
            await withCheckedContinuation { continuation in editor.cancelPreparedClose { continuation.resume() } }
            precondition(library.ready && !library.readOnly)
            try await edit(editor, ["type": "set_brush_size", "value": 89])
            try await library.flush()
            precondition(!library.hasUnsavedChanges)
            // UIKit's discard callback releases only its matching scene. The
            // other window keeps its claim and remains editable.
            other.systemSceneID = UUID().uuidString
            EditorStore.discardSceneSessions([other.systemSceneID!])
            try await wait("discarded scene release") { !other.workspaceLibrary!.ready }
            let released = try await library.read(["type": "load", "id": otherID])
            precondition(released["claim"].isNull && library.ready)
            try await edit(editor, ["type": "set_brush_size", "value": 95])
            await library.detach()
            let saved = try await other.workspaceLibrary!.read(["type": "load", "id": library.status["active_id"].raw])
            precondition(saved["claim"].isNull)
            let working = saved["entity"]["working"]
            precondition(working["tools"]["overrides"][String(working["preset"].uint)]["size"].number == 89,
                "Explicit teardown must preserve the last saved copy after discarding unsaved changes")
            print("PASS: platform \(platform), manager host commands, forms, inline validation, workspace previews/history, live-window copying, toolbar actions, deletion replacement and native package delivery")
        }
    }
    @MainActor static func selectionPreview(_ editor: EditorStore, page: String, target: String, cancel: Bool) async throws {
        let library = editor.workspaceLibrary!, manager = editor.workspaceManager
        let before = try await captureSession(editor), layout = editor.state["workspace"]["layout"]
        let active = library.status["active_id"].string
        try await manager.show(page)
        precondition(page == "workspaces" ? manager.selection == active : manager.selection == nil)
        manager.select(target)
        try await wait("selected layout preview") {
            library.previewingLayout && !SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, layout.raw)
        }
        let previewed = try await captureSession(editor)
        precondition(library.status["active_id"].string == active)
        precondition(SnapshotProjection.equal(previewed["capture"].raw, before["capture"].raw), "Selecting a row must not apply or persist its layout")
        if cancel {
            manager.query = "no matching workspace"; manager.search()
            try await wait("filtered preview cancellation") { !manager.selecting && !library.previewingLayout && !library.busy }
            precondition(manager.selection == nil && manager.view["details"].isNull && manager.view["rows"].array.isEmpty)
            precondition(SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, layout.raw))
            manager.query = ""; manager.search()
            try await wait("cleared search") { !manager.selecting && !manager.view["rows"].array.isEmpty }
            precondition(manager.selection == nil && manager.view["details"].isNull)
            manager.select(target); manager.select(active)
            try await wait("rapid selection") { !manager.selecting }
            precondition(manager.selection == active)
            precondition(SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, layout.raw))
            manager.select(target)
            manager.presented = false; manager.dismissed()
            try await wait("selection preview cancellation") { !manager.selecting && !library.previewingLayout && !library.busy }
            precondition(SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, layout.raw))
        }
    }
    @MainActor static func liveHistory(_ editor: EditorStore, platform: UInt32) async throws {
        let manager = editor.workspaceManager, library = editor.workspaceLibrary!
        let id = library.status["active_id"].string
        let before = try await captureSession(editor)
        let document = editor.state["layers"].stableKey
        let current = before["capture"]["history"]["current"].string
        let revisions = before["capture"]["history"]["revisions"].object
        let earlier = revisions.keys.first { key in
            key != current && !SnapshotProjection.equal(JSON(revisions[key]!)["layout"].raw, JSON(revisions[current]!)["layout"].raw)
        }!
        try await manager.run(JSON(["type": "history", "value": id]))
        precondition(library.previewingLayout && library.busy && manager.history["selected"].string == current)
        try await manager.refreshHistory(earlier)
        let preview = try await captureSession(editor)
        precondition(SnapshotProjection.equal(preview["capture"].raw, before["capture"].raw), "Preview must never enter the durable capture")
        precondition(!SnapshotProjection.equal(editor.state["workspace"]["layout"].raw, JSON(revisions[current]!)["layout"].raw))
        precondition(editor.state["layers"].stableKey == document)
        try await capture(manager, platform: platform, phase: "history")
        manager.presented = false; manager.dismissed()
        try await wait("history cancellation") { !library.previewingLayout && !library.busy }
        let canceled = try await captureSession(editor)
        precondition(SnapshotProjection.equal(canceled["capture"].raw, before["capture"].raw))
        try await manager.run(JSON(["type": "history", "value": id]))
        try await manager.refreshHistory(earlier)
        library.suspend()
        precondition(!manager.presented && library.readOnly)
        try await wait("suspended history cancellation") { !library.previewingLayout && !library.busy }
        try await library.resume()
        let resumed = try await captureSession(editor)
        precondition(SnapshotProjection.equal(resumed["capture"].raw, before["capture"].raw))
        try await manager.run(JSON(["type": "history", "value": id]))
        try await manager.refreshHistory(earlier)
        manager.historyAction()
        try await wait("history restoration") { !manager.presented && !library.busy && !manager.processing }
        let restored = try await captureSession(editor)
        precondition(restored["capture"]["history"]["undo"].array.count == before["capture"]["history"]["undo"].array.count + 1)
        precondition(SnapshotProjection.equal(restored["capture"]["working"].raw, before["capture"]["working"].raw))
        precondition(library.status["active_id"].string == id && editor.state["layers"].stableKey == document)
        try await edit(editor, ["type": "invoke", "command": "undo_workspace"])
        let undone = try await captureSession(editor)
        precondition(undone["capture"]["history"]["current"].string == current)
        manager.back()
    }
    @MainActor static func captureSession(_ store: EditorStore) async throws -> JSON {
        try await withCheckedThrowingContinuation { continuation in
            store.native!.workspaceSession(JSON(["type": "capture"])) { value, error in
                DispatchQueue.main.async {
                    if let error { continuation.resume(throwing: HostFailure(message: error)) }
                    else { continuation.resume(returning: value ?? JSON()) }
                }
            }
        }
    }
    @MainActor static func capture(_ manager: WorkspaceManager, platform: UInt32, phase: String) async throws {
        guard let path = ProcessInfo.processInfo.environment["CAPY_WORKSPACE_CAPTURES"], let library = manager.library else { return }
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
                let host = NSHostingView(rootView: WorkspaceManagerView(manager: manager, library: library)
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
    @MainActor static func packageDelivery(editor: EditorStore, root: URL, toolbar: String) async throws {
        let destination = root.appendingPathComponent("shared.capytoolbar")
        var open: URL? = destination, save: URL? = destination
        let files = WorkspacePackageFiles(dialogs: .init(open: { _, done in done(open) }, save: { _, _, done in done(save) }, export: nil))
        let manager = WorkspaceManager(store: editor, files: files)
        try await manager.run(JSON(["type": "export", "value": toolbar]))
        let bytes = try Data(contentsOf: destination)
        precondition(!bytes.isEmpty)
        try await manager.run(JSON(["type": "import_toolbar"]))
        precondition(manager.selection != toolbar && manager.page == "toolbar_library")
        let count = manager.view["rows"].array.count
        open = nil
        try await manager.run(JSON(["type": "import_toolbar"]))
        precondition(manager.view["rows"].array.count == count)
        save = nil
        try await manager.run(JSON(["type": "export", "value": toolbar]))
        let unchanged = try Data(contentsOf: destination)
        precondition(unchanged == bytes)
        open = destination
        try Data("broken package".utf8).write(to: destination)
        do { try await manager.run(JSON(["type": "import_toolbar"])); preconditionFailure("Corrupt import must fail") }
        catch { precondition(manager.view["rows"].array.count == count) }
        // A failed destination write leaves the old file bytes intact.
        save = root.appendingPathComponent("missing/failed.capytoolbar")
        do { try await manager.run(JSON(["type": "export", "value": toolbar])); preconditionFailure("Unavailable destination must fail") }
        catch { }
    }
}
