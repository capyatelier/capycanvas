import Foundation
import QuartzCore
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

/// Production photo transactions, file coordination and serial document owner.
/// Uses temporary files and dialog results; native picker delivery is separate.
@main struct ImageImportOwnerChecks {
    static func require(_ condition: Bool, _ message: String) throws {
        guard condition else { throw HostFailure(message: message) }
    }
    @MainActor static func wait(_ label: String, native: NativeOwner, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !ready() {
            try require(Date() < deadline, "Timed out: \(label)")
            let prepared = await withCheckedContinuation { done in
                native.flushPersistence { done.resume(returning: $0) }
            }
            try require(prepared, "Prepare pending photo work: \(label)")
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
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-photo-owner-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("Imported image.png")
        let pixels = Data([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0])
        let image = CGImage(width: 2, height: 2, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: 8, space: CGColorSpace(name: CGColorSpace.displayP3)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: CGDataProvider(data: pixels as CFData)!, decode: nil,
            shouldInterpolate: false, intent: .relativeColorimetric)!
        let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, image, nil)
        try require(CGImageDestinationFinalize(destination), "Write the synthetic P3 photo")
        let sourceBytes = try Data(contentsOf: url)
        let invalid = root.appendingPathComponent("invalid.png")
        try Data("not an image".utf8).write(to: invalid)
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform,
                persistence: EditorPersistence(root: root.appendingPathComponent("state-\(platform)")), managedWorkspaces: false)
            let native = store.native!
            let surface = CAMetalLayer(); surface.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
            native.attach(surface, width: 128, height: 128, scale: 1)
            defer { native.detach(); withExtendedLifetime(surface) {} }
            let deadline = Date().addingTimeInterval(30)
            while !store.snapshot["shaders_ready"].bool {
                try require(Date() < deadline && store.failure == nil, store.failure ?? "Canvas startup")
                let now = FrameTrace.now()
                await withCheckedContinuation { done in
                    native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() }
                }
                try await Task.sleep(for: .milliseconds(10))
            }
            var selection: URL? = url
            var delayedPicker: ((URL?) -> Void)?
            var holdPicker = false
            let saved = root.appendingPathComponent("Saved-\(platform).capy")
            var savePanels = 0
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { done in
                if holdPicker { delayedPicker = done } else { done(selection) }
            }, save: { _, type, done in
                precondition(type == .capyProject); savePanels += 1; done(saved)
            }, create: { _, done in
                done(JSON(["extent": [64, 48], "color": ["space": "Srgb", "depth": "U8"], "background": "White"]))
            }, paste: { done in done(.success(sourceBytes)) }))
            let files = store.projectFiles
            func invoke(_ command: String) async throws { try await edit(store, ["type": "invoke", "command": command]) }
            func settled(_ label: String) async throws {
                try await wait(label, native: native) {
                    !files.busy && !store.state["requests"].array.contains { $0["kind"]["type"].string == "document" }
                }
                try require(store.failure == nil, store.failure ?? "")
            }
            func replace(_ action: () async throws -> Void) async throws {
                let epoch = store.state["document_file"]["epoch"].uint
                try await action()
                try await wait("Replacement or unsaved choice", native: native) {
                    files.confirming || files.error != nil || store.state["document_file"]["epoch"].uint != epoch
                }
                if files.confirming { files.choose("discard") }
                try await settled("Replace drawing")
                try require(files.error == nil && store.state["document_file"]["epoch"].uint == epoch + 1, files.error ?? "Replacement must complete")
            }
            try await replace { try await invoke("new_document") }
            let blank = store.state["layers"].stableKey
            selection = nil
            try await invoke("import_image"); try await settled("Cancel photo selection")
            try require(store.state["layers"].stableKey == blank, "Picker cancellation must preserve all layers")
            selection = url
            // Hold an actual coordinated writer over incomplete bytes. The read
            // must wait until it publishes the complete encoded image.
            let writing = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0)
            let writer = Task.detached {
                try ProjectFileIO.coordinate(url, writing: true) { location in
                    try Data("incomplete image".utf8).write(to: location)
                    writing.signal()
                    guard release.wait(timeout: .now() + 20) == .success else { throw HostFailure(message: "Writer was not released") }
                    try sourceBytes.write(to: location)
                }
            }
            defer { release.signal() }
            let started = await withCheckedContinuation { done in
                DispatchQueue.global().async { done.resume(returning: writing.wait(timeout: .now() + 10) == .success) }
            }
            try require(started, "Coordinated writer must hold the file")
            try await invoke("import_image")
            try await Task.sleep(for: .milliseconds(250))
            let premature = files.error != nil || store.state["layers"].stableKey != blank
            release.signal(); try await writer.value
            try require(!premature, "Photo read must wait for coordinated replacement")
            try await settled("Place photo")
            try require(files.error == nil && store.state["layers"].array.count == 3, files.error ?? "Place must add one layer")
            try require(store.state["layer_tools"]["editing_layer"]["label"].string == "Imported image", "Use the photo name without its extension")
            try require(store.state["colors"]["rgb_space"].string == "Srgb", "Place must preserve receiving working space")
            let placed = store.state["layers"].stableKey
            try await invoke("undo"); try await wait("Place Undo", native: native) { store.state["layers"].stableKey == blank }
            try await invoke("redo"); try await wait("Place Redo", native: native) { store.state["layers"].stableKey == placed }
            try await invoke("paste_image"); try await settled("Paste encoded photo")
            try require(files.error == nil && store.state["layers"].array.count == 4, files.error ?? "Paste must add one retained photo")
            let pasted = store.state["layers"].stableKey
            try await invoke("undo"); try await wait("Paste Undo", native: native) { store.state["layers"].stableKey == placed }
            try await invoke("redo"); try await wait("Paste Redo", native: native) { store.state["layers"].stableKey == pasted }
            for failedURL in [root.appendingPathComponent("missing.png"), invalid] {
                selection = failedURL
                try await invoke("import_image"); try await settled("Failed read")
                try require(files.error != nil && store.state["layers"].stableKey == pasted, "Read/decode failure must preserve layers and report an error")
                files.error = nil
            }
            // A picker result must not silently import into a subsequently edited
            // drawing. Invoke the real owner directly while the UI is blocked.
            holdPicker = true
            try await invoke("import_image")
            try await wait("Delayed picker", native: native) { delayedPicker != nil }
            try await invoke("add_layer")
            let changed = store.state["layers"].stableKey
            delayedPicker?(url); delayedPicker = nil; holdPicker = false
            try await settled("Reject stale picker")
            try require(files.error != nil && store.state["layers"].stableKey == changed, "Late picker must preserve intervening edits")
            files.error = nil
            try await replace { files.openURL(url) }
            try require(store.state["colors"]["rgb_space"].string == "DisplayP3", "Photo Open must adopt its P3 working space")
            try require(store.state["document_file"]["location"].isNull, "Opening a photo must not give Save its source destination")
            func reopenedLayers() -> String {
                JSON(store.state["layers"].array.map {
                    // GPU cache generations are new after native reopening.
                    $0.replacing("paint_revision", with: JSON(0)).replacing("mask_revision", with: JSON(0)).raw
                }).stableKey
            }
            let opened = reopenedLayers()
            try await invoke("save_document"); try await settled("Save photo as project")
            try require(files.error == nil && savePanels == 1 && FileManager.default.fileExists(atPath: saved.path), files.error ?? "Save must choose a native project")
            try require(try Data(contentsOf: url) == sourceBytes, "Open/Place/Paste/Save must leave original photo bytes unchanged")
            try await replace { try await invoke("new_document") }
            try await replace { files.openURL(saved) }
            try require(reopenedLayers() == opened && store.state["colors"]["rgb_space"].string == "DisplayP3", "Native project must reopen retained P3 photo layers")
            // Untagged 2×1 RGBA PNG: prompt cancellation, invalid profile retry,
            // and an explicit assumption through the real Swift coordinator.
            let untagged = root.appendingPathComponent("Untagged.png")
            try Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAD0lEQVR4nGNQcGi4ygAEAAnyAbb4RS9rAAAAAElFTkSuQmCC")!.write(to: untagged)
            try await edit(store, ["type": "preferences", "action": ["type": "edit", "id": "missing_profile", "value": 1]])
            selection = untagged
            let beforePrompt = store.state["layers"].stableKey
            try await invoke("import_image")
            try await wait("Missing profile prompt", native: native) { files.pendingProfile != nil || files.error != nil }
            try require(files.pendingProfile?["profile_assumed"].bool == true, files.error ?? "Untagged image must ask before adoption")
            files.chooseProfile(nil); try await settled("Cancel interpretation")
            try require(store.state["layers"].stableKey == beforePrompt, "Cancelling interpretation must preserve the drawing")
            try await invoke("import_image")
            try await wait("Retry interpretation", native: native) { files.pendingProfile != nil }
            files.chooseProfile(JSON(["Icc": [UInt8]()]))
            try await wait("Invalid profile", native: native) { files.profileError != nil }
            try require(files.pendingProfile != nil && store.state["layers"].stableKey == beforePrompt, "Invalid profile must keep the form and document for retry")
            files.chooseProfile(JSON(["Builtin": "AdobeRgb"]))
            try await settled("Apply explicit interpretation")
            try require(files.error == nil && files.pendingProfile == nil && store.state["layers"].array.count == 3, files.error ?? "Explicit interpretation must finish the pending Place")
            try await invoke("undo"); try await wait("Interpreted Place Undo", native: native) { store.state["layers"].stableKey == beforePrompt }
            print("PASS platform \(platform): coordinated Place, cancellation, encoded Paste, history, stale/read failure preservation, P3 Open, safe Save and reopen")
        }
    }
}
