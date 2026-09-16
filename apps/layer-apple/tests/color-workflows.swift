import Foundation
import QuartzCore

/// Production document coordinator, owner queues and durable palette/settings
/// storage in disposable sessions. UI delivery is a separate XCTest workflow.
@main struct ColorWorkflowChecks {
    @MainActor static func require(_ value: Bool, _ message: String) throws {
        if !value { throw HostFailure(message: message) }
    }
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !ready() {
            try require(Date() < deadline, "Timed out: \(label)")
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func edit(_ store: EditorStore, _ action: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            store.edit(action) { error in
                if let error { done.resume(throwing: HostFailure(message: error)) }
                else { done.resume() }
            }
        }
    }
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-color-workflows-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let scene = UUID().uuidString
            let store = EditorStore(platform: platform, scene: scene, persistence: EditorPersistence(root: root))
            try await wait("Workspace startup") { store.workspaceLibrary?.ready == true || store.failure != nil }
            try require(store.failure == nil, store.failure ?? "")
            let surface = CAMetalLayer(); surface.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
            store.native!.attach(surface, width: 128, height: 128, scale: 1)
            let deadline = Date().addingTimeInterval(45)
            while !store.snapshot["shaders_ready"].bool {
                try require(Date() < deadline && store.failure == nil, store.failure ?? "Metal startup timed out")
                let prepared = await withCheckedContinuation { done in store.native!.flushPersistence { done.resume(returning: $0) } }
                try require(prepared, "Canvas preparation failed")
                let now = FrameTrace.now()
                await withCheckedContinuation { done in store.native!.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() } }
                try await Task.sleep(for: .milliseconds(10))
            }
            let options = JSON(["extent": [63, 47], "color": ["space": "DisplayP3", "depth": "U16"], "background": "Transparent"])
            try await edit(store, ["type": "new_document_settings", "settings": ["defaults": options.raw, "presets": []]])
            try await edit(store, ["type": "invoke", "command": "new_document"])
            try await wait("Creation choices") { store.projectFiles.creating }
            try require(store.projectFiles.newDocumentSpec["creation"]["options"].stableKey == options.stableKey, "New must expose saved defaults")
            let epoch = store.state["document_file"]["epoch"].uint
            let savedOptions = options.replacing("extent", with: JSON([79, 53]))
            store.projectFiles.created(NewDrawingChoice(options: savedOptions, presetName: "Wide color", useAsDefaults: true))
            try await wait("Duplicate preset error") { store.projectFiles.creationError != nil }
            try require(store.projectFiles.creating && store.state["document_file"]["epoch"].uint == epoch,
                "Failed preset validation must retain the form and drawing")
            try require(store.state["settings"]["new_document"]["defaults"].stableKey == options.stableKey, "Invalid presets must not change defaults")
            store.projectFiles.created(NewDrawingChoice(options: savedOptions, presetName: " Studio P3 ", useAsDefaults: true))
            try await wait("New drawing") { store.state["document_file"]["epoch"].uint == epoch + 1 && !store.projectFiles.busy }
            try require(store.state["settings"]["new_document"]["defaults"].stableKey == savedOptions.stableKey, "Save full options as defaults")
            try require(store.state["settings"]["new_document"]["presets"][0]["name"].string == "Studio P3", "Save the trimmed preset name")
            try require(store.state["colors"]["rgb_space"].string == "DisplayP3", "New must adopt the selected working space")
            try await edit(store, ["type": "invoke", "command": "new_document"])
            try await wait("Reopened creation") { store.projectFiles.creating }
            try require(store.projectFiles.newDocumentSpec["creation"]["options"].stableKey == savedOptions.stableKey, "Reopen with current defaults")
            store.projectFiles.created(nil)
            try await wait("New cancellation") { !store.projectFiles.busy }
            try require(store.state["document_file"]["epoch"].uint == epoch + 1, "Cancel must preserve the drawing")

            func color(_ action: [String: Any]) async throws { try await edit(store, ["type": "color", "action": action]) }
            func library(_ action: [String: Any]) async throws { try await color(["op": "library", "action": action]) }
            try await library(["op": "create_palette", "name": "Studio colors"])
            let palette = store.state["colors"]["library"]["palettes"][1]["id"].uint
            for space in ["Srgb", "DisplayP3", "AdobeRgb", "ProPhoto"] {
                let definition = JSON(["space": space, "rgba": [1.125, -0.125, 0.34567891, 213.0 / 65535]])
                try await color(["op": "set_slot", "slot": "foreground", "color": definition.raw])
                let tagged = store.state["colors"]["foreground"]
                try await library(["op": "store", "palette": palette, "name": space, "color": tagged.raw])
                let swatch = store.state["colors"]["library"]["palettes"][1]["swatches"].array.last!
                try require(swatch["color"].stableKey == tagged.stableKey, "Store must retain extended values and precise alpha in \(space)")
                try await library(["op": "rename", "id": swatch["id"].uint, "name": "Saved " + space])
                try await color(["op": "set_slot", "slot": "foreground", "color": ["space": "Srgb", "rgba": [0, 0, 0, 1]]])
                try await library(["op": "use", "id": swatch["id"].uint])
                try require(store.state["colors"]["foreground"].stableKey == tagged.stableKey, "Use must restore exact tagged values in \(space)")
            }
            try await library(["op": "rename_palette", "id": palette, "name": "Retained colors"])
            let savedLibrary = store.state["colors"]["library"].stableKey
            var rejected = false
            do {
                try await library(["op": "rename_palette", "id": palette, "name": ""])
            } catch {
                rejected = true
                try require(store.state["colors"]["library"].stableKey == savedLibrary, "Invalid palette edits must be atomic")
            }
            try require(rejected, "An empty palette name must be rejected")
            // A new document changes wheel space but retains workspace palettes.
            try await edit(store, ["type": "invoke", "command": "new_document"])
            try await wait("Next document choices") { store.projectFiles.creating }
            let next = JSON(["extent": [67, 43], "color": ["space": "ProPhoto", "depth": "U16"], "background": "White"])
            store.projectFiles.created(NewDrawingChoice(options: next, presetName: "", useAsDefaults: false))
            try await wait("Next document") { store.state["document_file"]["epoch"].uint == epoch + 2 && !store.projectFiles.busy }
            try require(store.state["colors"]["library"].stableKey == savedLibrary && store.state["colors"]["rgb_space"].string == "ProPhoto",
                "Document adoption must preserve palettes while updating the wheel gamut")
            let settings = store.state["settings"]["new_document"].stableKey
            let flushed = await withCheckedContinuation { done in store.flushPersistence { done.resume(returning: $0) } }
            try require(flushed, "Settings and palettes must flush durably")
            try await store.workspaceLibrary!.close()
            let restored = EditorStore(platform: platform, scene: scene, persistence: EditorPersistence(root: root))
            try await wait("Fresh owner restoration") { restored.workspaceLibrary?.ready == true || restored.failure != nil }
            try require(restored.state["colors"]["library"].stableKey == savedLibrary, "Fresh owner must restore palette IDs, names, tags and samples")
            try require(restored.state["settings"]["new_document"].stableKey == settings, "Fresh owner must restore creation presets and defaults")
            let removal = restored.state["colors"]["library"]["palettes"][1]["swatches"][0]["id"].uint
            for action: [String: Any] in [["op": "remove", "id": removal], ["op": "remove_palette", "id": palette]] {
                try await edit(restored, ["type": "color", "action": ["op": "library", "action": action]])
            }
            try require(restored.state["colors"]["library"]["palettes"].array.count == 1, "Swatch and palette removal must preserve the remaining palette")
            try await restored.workspaceLibrary!.close()
            print("PASS: platform \(platform), creation defaults/preset validation/retry/Cancel, all tagged palette spaces, document adoption and fresh-owner durable restoration")
            withExtendedLifetime(surface) {}
        }
    }
}
