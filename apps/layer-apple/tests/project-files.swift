import Foundation
import AppKit
import QuartzCore
import ImageIO

@main struct ProjectFileChecks {
    @MainActor static var prepareFrame: (() async throws -> Void)?
    @MainActor static func wait(_ description: String, _ condition: () -> Bool) async throws {
        let end = Date().addingTimeInterval(45)
        while !condition() {
            precondition(Date() < end, description)
            if let prepareFrame { try await prepareFrame() }
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func startupFrame(_ store: EditorStore) async throws {
        // No display link drives this fixture. Prepare document work without a
        // drawable, then run the host's bundled-catalog/startup completion path.
        let prepared = await withCheckedContinuation { continuation in
            store.native!.flushPersistence { continuation.resume(returning: $0) }
        }
        precondition(prepared, "Offscreen startup preparation failed")
        let now = FrameTrace.now()
        await withCheckedContinuation { continuation in
            store.native!.frame(now: now, target: now + 16_666_667) { _, _, _ in continuation.resume() }
        }
        try await Task.sleep(for: .milliseconds(10))
    }
    @MainActor static func main() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-files-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: root) }
        let atomic = root.appendingPathComponent("atomic")
        try Data([1,2,3]).write(to: atomic)
        do {
            try ProjectFileIO.atomicWrite(to: atomic) { fd in
                _ = [UInt8](repeating: 7, count: 16).withUnsafeBytes { Darwin.write(fd, $0.baseAddress, $0.count) }
                throw HostFailure(message: "Injected write failure")
            }
            preconditionFailure("Failed write must throw")
        } catch {}
        precondition((try? Data(contentsOf: atomic)) == Data([1,2,3]))
        do {
            try ProjectFileIO.atomicWrite(to: atomic, beforeCommit: { throw HostFailure(message: "Cancelled") }) { fd in
                _ = [UInt8](repeating: 9, count: 16).withUnsafeBytes { Darwin.write(fd, $0.baseAddress, $0.count) }
            }
            preconditionFailure("Cancelled publication must throw")
        } catch {}
        precondition((try? Data(contentsOf: atomic)) == Data([1,2,3]))
        precondition((try? FileManager.default.contentsOfDirectory(atPath: root.path)) == ["atomic"])
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform,
                persistence: EditorPersistence(root: root.appendingPathComponent("files-\(platform)")), managedWorkspaces: false)
            prepareFrame = { try await startupFrame(store) }
            defer { prepareFrame = nil }
            let target = root.appendingPathComponent("drawing-\(platform).capy")
            let invalid = root.appendingPathComponent("invalid-\(platform).capy")
            try Data("incomplete".utf8).write(to: invalid)
            var saveLocation: URL? = target
            var openLocation: URL? = target
            var choices = 0
            var creationOptions: JSON? = JSON(["extent": [63, 47], "color": ["space": "Srgb", "depth": "U8"], "background": "White"])
            var beforeOpen: (() -> Void)?
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(
                open: { _, done in choices += 1; beforeOpen?(); done(openLocation.map { [$0] } ?? []) },
                save: { _, _, callback in choices += 1; callback(saveLocation) },
                create: { spec, callback in
                    precondition(spec["extent"][0].uint == 2048 && spec["maximum"].uint == 8192)
                    callback(creationOptions)
                },
                export: platform == 0 ? { staging, callback in
                    choices += 1
                    if let url = saveLocation {
                        do { try FileManager.default.copyItem(at: staging, to: url); callback(url) }
                        catch { preconditionFailure("Provider copy failed: \(error)") }
                    } else { callback(nil) }
                } : nil, exportOptions: { $0.choose($0.recipe) }))
            let layer = CAMetalLayer(); layer.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
            store.native!.attach(layer, width: 128, height: 128, scale: 1)
            let startupDeadline = Date().addingTimeInterval(45)
            while !store.snapshot["shaders_ready"].bool && store.failure == nil {
                precondition(Date() < startupDeadline, "Native canvas startup did not finish")
                try await startupFrame(store)
            }
            precondition(store.failure == nil, store.failure ?? "Attach failed")
            func invoke(_ command: String) async throws {
                store.invoke(command)
                try await wait("Document operation did not finish: \(command)") {
                    !store.projectFiles.busy && store.state["requests"].array.isEmpty
                }
                // Ensure the owner has actually received the action before
                // checking its completion, rather than observing old idle state.
                _ = await withCheckedContinuation { continuation in
                    store.native!.submit(2, JSON(["type":"catalog"])) { continuation.resume(returning: $0 != nil) }
                }
                try await wait("Document operation did not settle: \(command)") { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
            }
            try await invoke("save_document_as")
            precondition(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            precondition(FileManager.default.fileExists(atPath: target.path))
            precondition(store.state["document_file"]["location"]["name"].string == target.lastPathComponent)
            let mode = try FileManager.default.attributesOfItem(atPath: target.path)[.posixPermissions] as! NSNumber
            precondition(mode.intValue & 0o777 == 0o600)
            store.invoke("add_layer")
            try await wait("Layer edit not applied") { store.state["layers"].array.count == 3 }
            let id = store.state["layer_tools"]["editing_layer"]["id"].uint
            store.layer(["op":"rename", "id":id, "name":"Saved layer"])
            try await wait("Rename not applied") { store.state["layers"].array.contains { $0["label"].string == "Saved layer" } }
            try await invoke("save_document")
            precondition(!store.state["document_file"]["modified"].bool)
            var savedData = try Data(contentsOf: target)
            // A file can launch the app before its first state publication or
            // before the canvas attaches. Retain it through normal startup.
            for published in [false, true] {
                let cold = EditorStore(platform: platform, persistence: EditorPersistence(root:
                    root.appendingPathComponent("startup-\(platform)-\(published)")))
                cold.projectFiles = ProjectFiles(store: cold, dialogs: .init(
                    open: { _, _ in preconditionFailure("An external URL must not open a picker") },
                    save: { _, _, _ in preconditionFailure("Startup must not save a blank drawing") }))
                if published {
                    try await wait("Initial editor state missing") { !cold.state.isNull }
                    try await wait("Workspace must initialize before Metal in this ordering") { cold.workspaceLibrary?.ready == true }
                } else { precondition(cold.state.isNull) }
                precondition(!cold.snapshot["gpu_ready"].bool)
                cold.projectFiles.openURL(target)
                if let error = cold.projectFiles.error {
                    throw HostFailure(message: "Startup Open rejected the supplied file: \(error)")
                }
                let surface = CAMetalLayer(); surface.bounds = layer.bounds
                cold.native!.attach(surface, width: 128, height: 128, scale: 1)
                if published {
                    // Metal attachment publishes an enabled Open command before
                    // first-frame bundled-filter validation has even started.
                    // Drain the owner/UI handoff while retaining that ordering.
                    for _ in 0..<2 {
                        _ = await withCheckedContinuation { continuation in
                            cold.native!.submit(2, JSON(["type": "catalog"])) { continuation.resume(returning: $0 != nil) }
                        }
                    }
                    precondition(cold.snapshot["gpu_ready"].bool && !cold.snapshot["shaders_ready"].bool)
                    precondition(!cold.projectFiles.busy && cold.state["requests"].array.isEmpty
                        && cold.state["document_file"]["epoch"].uint == 0,
                        "External Open must wait for first-frame startup, not just Metal attachment")
                }
                // Drive the actual first-frame/catalog sequence. A recovery
                // flush alone omits the startup publication that can race Open.
                let deadline = Date().addingTimeInterval(45)
                while cold.state["document_file"]["epoch"].uint != 1 || cold.projectFiles.busy {
                    if cold.projectFiles.error != nil || cold.failure != nil { break }
                    precondition(Date() < deadline, "Startup Open did not finish")
                    try await startupFrame(cold)
                }
                if let error = cold.projectFiles.error ?? cold.failure {
                    throw HostFailure(message: "Startup Open failed after preparation: \(error)")
                }
                precondition(cold.state["document_file"]["location"]["name"].string == target.lastPathComponent)
                precondition(cold.state["layers"].array.contains { $0["label"].string == "Saved layer" })
                precondition(!cold.state["document_file"]["modified"].bool)
                precondition(cold.workspaceLibrary?.ready == true)
                await cold.workspaceLibrary?.detach()
                withExtendedLifetime(surface) {}
            }
            print("Startup Open passes for Apple platform \(platform)")
            // Sequential external delivery opens independent drawings without
            // a location picker. Concurrent delivery is covered by drawing-tabs.swift.
            for _ in 0..<2 {
                let epoch = store.state["document_file"]["epoch"].uint
                let originalChoices = choices
                store.projectFiles.openURL(target)
                try await wait("External Open did not settle") {
                    store.state["document_file"]["epoch"].uint == epoch + 1 && !store.projectFiles.busy
                }
                precondition(store.state["document_file"]["location"]["name"].string == target.lastPathComponent)
                precondition(choices == originalChoices && store.failure == nil && store.projectFiles.error == nil)
            }
            // A URL arriving alongside a cancelled New request waits for the
            // first file operation, then opens its own drawing.
            let extent = creationOptions
            creationOptions = nil
            let queuedEpoch = store.state["document_file"]["epoch"].uint
            store.invoke("new_document")
            store.projectFiles.openURL(target)
            let queuedDeadline = Date().addingTimeInterval(45)
            while store.state["document_file"]["epoch"].uint != queuedEpoch + 1 || store.projectFiles.busy {
                guard Date() < queuedDeadline else { throw HostFailure(message: "Queued Open: epoch=\(store.state["document_file"]["epoch"].uint), expected=\(queuedEpoch + 1), busy=\(store.projectFiles.busy), ready=\(store.snapshot["shaders_ready"].bool), error=\(store.projectFiles.error ?? store.failure ?? "none")") }
                try await startupFrame(store)
            }
            precondition(store.projectFiles.error == nil && store.failure == nil)
            creationOptions = extent
            print("External Open admission passes for Apple platform \(platform)")
            saveLocation = nil
            try await invoke("save_document_as")
            precondition((try? Data(contentsOf: target)) == savedData)
            let choicesAfterCancellation = choices
            let savedLayerName = "Saved after cancelling Save As"
            store.layer(["op":"rename", "id":id, "name":savedLayerName])
            try await wait("Rename after cancelled Save As") {
                store.state["layers"].array.contains { $0["label"].string == savedLayerName }
            }
            try await invoke("save_document")
            precondition(choices == choicesAfterCancellation && store.projectFiles.error == nil,
                "Save after cancelling Save As must retain the original destination without another picker")
            precondition(!store.state["document_file"]["modified"].bool)
            savedData = try Data(contentsOf: target)
            store.invoke("add_layer")
            try await wait("Unsaved edit not applied") { store.state["layers"].array.count == 4 }
            let epoch = store.state["document_file"]["epoch"].uint
            let newOptions = creationOptions
            creationOptions = nil
            try await invoke("new_document")
            precondition(store.state["document_file"]["epoch"].uint == epoch && store.state["layers"].array.count == 4)
            creationOptions = newOptions
            try await invoke("new_document")
            precondition(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            precondition(store.state["layers"].array.count == 2 && store.state["document_file"]["epoch"].uint == epoch + 1)
            let createdEpoch = store.state["document_file"]["epoch"].uint
            creationOptions = nil
            try await invoke("new_document")
            precondition(store.state["document_file"]["epoch"].uint == createdEpoch, "Cancelled size choice must keep the drawing")
            let png = root.appendingPathComponent("export-\(platform).png")
            saveLocation = png
            let projectLocation = store.state["document_file"]["location"].raw
            try await invoke("export_document")
            precondition(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            let source = CGImageSourceCreateWithURL(png as CFURL, nil)!
            let image = CGImageSourceCreateImageAtIndex(source, 0, nil)!
            precondition(image.width == 63 && image.height == 47)
            precondition(!store.state["document_file"]["modified"].bool)
            precondition(store.state["document_file"]["location"].isNull && projectLocation is NSNull)
            let exported = try Data(contentsOf: png)
            saveLocation = nil
            try await invoke("export_document")
            precondition((try? Data(contentsOf: png)) == exported, "Cancelled PNG destination must preserve the existing file")
            openLocation = invalid
            try await invoke("open_document")
            precondition(store.projectFiles.error != nil)
            precondition(store.state["layers"].array.count == 2)
            store.projectFiles.error = nil
            store.invoke("add_layer")
            try await wait("Unsaved failed-open fixture") { store.state["layers"].array.count == 3 }
            let unsavedLayers = store.state["layers"].stableKey
            let unsavedFile = store.state["document_file"].stableKey
            store.invoke("open_document")
            try await wait("Invalid replacement must finish with an error") {
                !store.projectFiles.busy && store.projectFiles.error != nil && store.state["requests"].array.isEmpty
            }
            precondition(store.state["layers"].stableKey == unsavedLayers
                && store.state["document_file"].stableKey == unsavedFile,
                "Failed Open must retain the original unsaved tab")
            store.projectFiles.error = nil
            store.invoke("undo")
            try await wait("Failed Open must retain Undo") { store.state["layers"].array.count == 2 }
            store.invoke("redo")
            try await wait("Failed Open must retain Redo") { store.state["layers"].stableKey == unsavedLayers }
            openLocation = nil
            try await invoke("open_document")
            precondition(store.state["document_file"]["modified"].bool
                && store.state["layers"].stableKey == unsavedLayers)
            store.invoke("undo")
            try await wait("Restore the clean fixture before the queued-edit check") {
                store.state["layers"].array.count == 2 && !store.state["document_file"]["modified"].bool
            }
            print("PASS platform \(platform): failed Open preserves unsaved drawing, history and retry cancellation")
            store.projectFiles.error = nil; openLocation = target
            beforeOpen = { store.invoke("add_layer") }
            try await invoke("open_document")
            precondition(store.projectFiles.error != nil, "An edit queued before capture must prevent replacement")
            precondition(store.state["layers"].array.count == 3)
            beforeOpen = nil; store.projectFiles.error = nil
            try await invoke("open_document")
            precondition(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            precondition(store.state["layers"].array.contains { $0["label"].string == savedLayerName })
            precondition(!store.state["document_file"]["modified"].bool)
            store.invoke("add_layer")
            var closed: Bool?
            store.projectFiles.confirmCurrentClose { closed = $0 }
            try await wait("Close prompt missing") { store.projectFiles.confirming }
            store.projectFiles.choose("cancel")
            try await wait("Close cancellation did not settle") { closed != nil }
            precondition(closed == false)
            closed = nil
            store.projectFiles.confirmCurrentClose { closed = $0 }
            try await wait("Save on close prompt missing") { store.projectFiles.confirming }
            store.projectFiles.choose("save")
            try await wait("Save on close did not finish") { closed != nil }
            precondition(closed == true && !store.state["document_file"]["modified"].bool)
            withExtendedLifetime(layer) {}
            print("Project files pass for Apple platform \(platform)")
        }
    }
}
