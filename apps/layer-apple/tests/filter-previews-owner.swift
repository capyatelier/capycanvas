import AppKit
import SwiftUI

/// Exercise the real asynchronous preview owner and controller while editing,
/// then save/reopen through the production file coordinator in isolated storage.
@main final class FilterPreviewOwnerChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-filter-preview-\(UUID())")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
            defer { try? FileManager.default.removeItem(at: root) }
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root), managedWorkspaces: false)
            let native = store.native!
            let surface = attachSurface(store, CGSize(width: 128, height: 128))
            defer { store.filterPreviews.hide("check"); native.detach(); withExtendedLifetime(surface) {} }
            func wait(_ label: String, _ ready: () -> Bool) async throws {
                try await CapyTest.wait(label, failure: { store.failure }, step: { await frame(native) }, ready)
                try require(store.failure == nil, store.failure ?? "")
            }
            func invoke(_ command: String) async throws { try await store.apply(["type": "invoke", "command": command]) }
            func idle() async throws {
                try await wait("Document completion") { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
                try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            }
            try await wait("Metal startup") { store.snapshot["shaders_ready"].bool }
            let saved = root.appendingPathComponent("Preview Check.capy")
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(
                open: { _, done in done([saved]) }, save: { _, _, done in done(saved) }, create: { _, done in
                    done(JSON(["extent": [512, 512], "color": ["space": "DisplayP3", "depth": "U16"], "background": "White"]))
                }))
            try await invoke("new_document"); try await idle()
            try await invoke("select_all")
            try await store.apply(["type": "layer", "action": ["op": "fill_selection"]])
            try await invoke("deselect")
            let flushed = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
            try require(flushed, "Complete initial artwork")
            try await wait("New drawing shaders") { store.snapshot["shaders_ready"].bool }

            store.filterPreviews.show(token: "check", id: "curves", width: 96, scale: 1)
            for step in 0..<6 {
                try await drain(0.21)
                try await store.apply(["type": "set_layer_opacity", "opacity": 0.4 + Double(step) * 0.1]); await frame(native)
                try require(store.failure == nil, store.failure ?? "Edits cannot raise a preview alert")
            }
            try await wait("Automatic preview retry") { store.filterPreviews.images["curves"] != nil }
            try await invoke("save_document_as"); try await idle()
            try require(FileManager.default.fileExists(atPath: saved.path), "Save As must create a local copy")
            let original = try Data(contentsOf: saved)
            try await invoke("open_document"); try await idle()
            try require(try Data(contentsOf: saved) == original && !store.state["document_file"]["modified"].bool,
                "Reopen must preserve the saved file and clean document")
            try await wait("Previews after reopen") { store.filterPreviews.images["curves"] != nil }
            try await store.apply(["type": "set_layer_opacity", "opacity": 0.3]); await frame(native)
            try require(store.state["document_file"]["modified"].bool, "Editing resumes after reopen")
            try await invoke("undo"); await frame(native)
            try await wait("Previews after Undo") { store.filterPreviews.images["curves"] != nil }
            try require(try Data(contentsOf: saved) == original, "Preview refresh and Undo never write the saved file")
            try require(store.failure == nil, "No background-preview failure")
            note("PASS platform \(platform): automatic retry during edits, local Save As/reopen, continued editing and Undo")
        }
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await run(); exit(0) } catch { note("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
