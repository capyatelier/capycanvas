import Foundation
import AppKit
import QuartzCore

@main struct RecoveryChecks {
    @MainActor static func wait(_ description: String, _ condition: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(45)
        while !condition() {
            precondition(Date() < deadline, description)
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    static func io<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            NativeProjectTask.io.async {
                do { continuation.resume(returning: try work()) }
                catch { continuation.resume(throwing: error) }
            }
        }
    }
    @MainActor static func flush(_ store: EditorStore) async -> Bool {
        await withCheckedContinuation { continuation in store.flushPersistence { continuation.resume(returning: $0) } }
    }
    @MainActor static func attach(_ store: EditorStore) async throws -> CAMetalLayer {
        let layer = CAMetalLayer(); layer.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
        store.native!.attach(layer, width: 128, height: 128, scale: 1)
        // Complete offscreen startup. Recovery below still
        // has no display link or frame after pen-up, preserving that regression.
        let deadline = Date().addingTimeInterval(45)
        while !store.snapshot["shaders_ready"].bool || store.workspaceLibrary?.ready != true {
            precondition(Date() < deadline && store.failure == nil, store.failure ?? "Native startup timed out")
            let ready = await withCheckedContinuation { continuation in
                store.native!.flushPersistence { continuation.resume(returning: $0) }
            }
            precondition(ready, "Offscreen renderer preparation failed")
            let now = FrameTrace.now()
            await withCheckedContinuation { continuation in
                store.native!.frame(now: now, target: now + 16_666_667) { _, _, _ in continuation.resume() }
            }
            try await Task.sleep(for: .milliseconds(10))
        }
        precondition(store.failure == nil, store.failure ?? "")
        return layer
    }
    @MainActor static func releaseDuringFlush(platform: UInt32, root: URL) async throws {
        let persistence = EditorPersistence(root: root), files = RecoveryFiles(root: root)
        var store: EditorStore? = EditorStore(platform: platform, persistence: persistence)
        let layer = try await attach(store!)
        store!.invoke("add_layer")
        try await wait("Released-scene layer missing") { store!.state["layers"].array.count == 3 }
        let id = store!.state["layer_tools"]["editing_layer"]["id"].uint
        store!.layer(["op": "rename", "id": id, "name": "Released scene survivor"])
        try await wait("Released-scene edit missing") {
            store!.state["layers"].array.contains { $0["label"].string == "Released scene survivor" }
        }
        weak let released = store
        var completed: Bool?
        store!.flushPersistence { completed = $0 }
        // Scene teardown can release its last owner before the native barrier's
        // reply reaches MainActor. The full recovery write must still finish.
        store = nil
        try await wait("Released-scene flush did not reply") { completed != nil }
        precondition(completed == true, "Scene release must not abandon an accepted recovery flush")
        try await wait("Completed flush retained its editor") { released == nil }
        let records = try await io { try files.list().records }
        precondition(records.count == 1, "The released scene's committed drawing must remain recoverable")

        let reopened = EditorStore(platform: platform, persistence: persistence)
        let reopenedLayer = try await attach(reopened)
        reopened.recovery.restore(records[0])
        try await wait("Released-scene archive did not reopen") {
            reopened.state["document_file"]["epoch"].uint == 1 && !reopened.projectFiles.busy
        }
        precondition(reopened.projectFiles.error == nil && reopened.failure == nil)
        precondition(reopened.state["layers"].array.count == 3
            && reopened.state["layers"].array.contains { $0["label"].string == "Released scene survivor" })
        precondition(reopened.state["document_file"]["modified"].bool,
            "Recovery must preserve the unsaved drawing instead of acknowledging Save")
        let closed = await withCheckedContinuation { continuation in
            reopened.recovery.close { continuation.resume(returning: $0) }
        }
        precondition(closed)
        withExtendedLifetime((layer, reopenedLayer)) {}
        print("Recovery survives released scene on Apple platform \(platform)"); fflush(stdout)
    }
    @MainActor static func main() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("capy-recovery-\(UUID())")
        defer { try? FileManager.default.removeItem(at: directory) }
        for platform: UInt32 in [0, 1] {
            try await releaseDuringFlush(platform: platform, root: directory.appendingPathComponent("released-\(platform)"))
            let root = directory.appendingPathComponent("platform-\(platform)")
            let persistence = EditorPersistence(root: root), files = RecoveryFiles(root: root)
            let scene = UUID().uuidString
            var store: EditorStore? = EditorStore(platform: platform, scene: scene, persistence: persistence)
            let layer = try await attach(store!)
            store!.invoke("add_layer")
            try await wait("First edit not applied") { store!.state["layers"].array.count == 3 }
            let id = store!.state["layer_tools"]["editing_layer"]["id"].uint
            store!.layer(["op":"rename", "id":id, "name":"Recovery survivor"])
            try await wait("Rename not applied") { store!.state["layers"].array.contains { $0["label"].string == "Recovery survivor" } }
            let brushDeadline = Date().addingTimeInterval(20)
            repeat {
                let prepared = await withCheckedContinuation { continuation in
                    store!.native!.flushPersistence { continuation.resume(returning: $0) }
                }
                precondition(prepared && Date() < brushDeadline, "Drawing preparation did not finish")
                if store!.snapshot["brush_ready"].bool { break }
                try await Task.sleep(for: .milliseconds(10))
            } while true
            let beforeInk = store!.state["document_file"]["revision"].uint
            let now = FrameTrace.now()
            store!.native!.pointer(id: 7, tool: 0, button: 0, records: [
                50,60,1,0,0,0,0,Double(now),1,
                70,80,1,0,0,0,0,Double(now + 10_000_000),2,
                90,95,1,0,0,0,0,Double(now + 20_000_000),3,
            ], predicted: false, revision: store!.cameraRevision)
            // No display link/frame follows this pen-up. The full lifecycle
            // barrier must submit it and await the published recovery archive.
            let firstFlush = await flush(store!); precondition(firstFlush)
            precondition(store!.state["document_file"]["revision"].uint > beforeInk, "Queued pen-up must reach committed recovery")
            print("Recovery first flush \(platform)"); fflush(stdout)
            let first = try await io { try files.list().records.first! }
            let firstBytes = try await io { try Data(contentsOf: files.archive(first)!) }
            precondition(store!.state["document_file"]["modified"].bool, "Recovery must not acknowledge Save")
            precondition(store!.state["requests"].array.isEmpty)
            for url in [files.archive(first)!, files.archive(first)!.deletingLastPathComponent().appendingPathComponent("current.json")] {
                let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as! NSNumber
                precondition(mode.intValue & 0o777 == 0o600)
            }

            // A cancelled task cannot replace the previous durable generation.
            let expected = (store!.state["document_file"]["epoch"].uint, store!.state["document_file"]["revision"].uint)
            let task: NativeProjectTask = await withCheckedContinuation { continuation in
                store!.native!.recoveryTask(expected: expected) { task, error in
                    precondition(task != nil, error ?? "Capture failed"); continuation.resume(returning: task!)
                }
            }
            task.cancel()
            do { _ = try await io { try files.write(task, scene: first.scene, title: "Cancelled") }; preconditionFailure("Cancelled write succeeded") }
            catch {}
            let unchanged = try await io { try files.current(first.scene) }
            precondition(unchanged == first)
            let afterCancel = try await io { try Data(contentsOf: files.archive(first)!) }
            precondition(afterCancel == firstBytes)
            print("Recovery cancellation \(platform)"); fflush(stdout)

            // Hold the file worker, then queue edits while one snapshot is in
            // flight. The flush must cover the newest revision without a queue
            // of full project copies or an older completion marking it current.
            let gate = DispatchSemaphore(value: 0)
            NativeProjectTask.io.async { precondition(gate.wait(timeout: .now() + 20) == .success) }
            store!.invoke("add_layer")
            try await wait("Second edit missing") { store!.state["layers"].array.count == 4 }
            var completed: Bool?
            store!.flushPersistence { completed = $0 }
            try await wait("Recovery did not start") { store!.recovery.saving }
            for _ in 0..<8 { store!.invoke("add_layer") }
            try await wait("Queued edits missing") { store!.state["layers"].array.count == 12 }
            gate.signal()
            try await wait("Flush did not include queued edits") { completed != nil }
            precondition(completed == true)
            let latest = try await io { try files.current(first.scene)! }
            precondition(latest.generation != first.generation)
            let generationCount = try await io {
                try FileManager.default.contentsOfDirectory(at: files.archive(latest)!.deletingLastPathComponent(), includingPropertiesForKeys: nil)
                    .filter { $0.pathExtension == "capy" }.count
            }
            precondition(generationCount == 1, "Obsolete successful generations must be reclaimed")
            try await io { try files.remove(first) }
            let afterStaleRemoval = try await io { try files.current(first.scene) }
            precondition(afterStaleRemoval == latest, "A stale discard must not delete a newer recovery generation")
            let corruptFolder = root.appendingPathComponent("recovery/\(UUID())")
            try FileManager.default.createDirectory(at: corruptFolder, withIntermediateDirectories: true)
            try Data("{broken".utf8).write(to: corruptFolder.appendingPathComponent("current.json"))
            let mixed = try await io { try files.list() }
            precondition(mixed.records.count == 1 && mixed.errors.count == 1, "One corrupt record must not hide valid recoveries")
            try FileManager.default.removeItem(at: corruptFolder)
            print("Recovery latest generation \(platform)"); fflush(stdout)

            // Force a manifest read/write failure; never replace invalid state
            // with defaults, and keep the accepted in-memory drawing retryable.
            let manifest = files.archive(latest)!.deletingLastPathComponent().appendingPathComponent("current.json")
            let manifestBytes = try Data(contentsOf: manifest)
            try Data("{broken".utf8).write(to: manifest)
            store!.invoke("add_layer")
            try await wait("Failure fixture edit missing") { store!.state["layers"].array.count == 13 }
            let failed = await flush(store!); precondition(!failed && store!.recovery.error != nil)
            precondition(FileManager.default.fileExists(atPath: files.archive(latest)!.path))
            precondition((try? Data(contentsOf: manifest)) == Data("{broken".utf8))
            try AtomicJSONFile.write(manifestBytes, to: manifest)
            let retried = await flush(store!); precondition(retried && store!.recovery.error == nil)
            print("Recovery retry \(platform)"); fflush(stdout)

            // Simulate loss of the process owner without a clean close. A new
            // owner (even with the restored scene ID) must offer the archive and
            // leave it intact while its own initial blank document is clean.
            store = nil
            try await Task.sleep(for: .milliseconds(100))
            let reopened = EditorStore(platform: platform, scene: scene, persistence: persistence)
            let reopenedLayer = try await attach(reopened)
            try await wait("Recovery not offered after owner loss") { reopened.recovery.presented && !reopened.recovery.records.isEmpty }
            let recovered = reopened.recovery.records[0]
            let startupFlush = await flush(reopened); precondition(startupFlush)
            let preserved = try await io { try files.current(recovered.scene) }
            precondition(preserved == recovered)
            // Recovery must protect a different drawing already in this window.
            // Save-and-continue retains the selected recovery destination across
            // the intermediate manual-save request, without another Open picker.
            let manual = root.appendingPathComponent("before-recovery.capy")
            reopened.projectFiles = ProjectFiles(store: reopened, dialogs: .init(
                open: { _ in preconditionFailure("Recovery must retain its selected archive through Save") },
                save: { _, _, complete in complete(manual) },
                export: platform == 0 ? { source, complete in
                    do { try FileManager.default.copyItem(at: source, to: manual); complete(manual) }
                    catch { preconditionFailure("Fixture export failed: \(error)") }
                } : nil))
            reopened.invoke("add_layer")
            try await wait("Current replacement fixture missing") { reopened.state["layers"].array.count == 3 }
            for recoveryFirst in [false, true] {
                if recoveryFirst {
                    reopened.recovery.restore(recovered)
                    reopened.projectFiles.openURL(manual)
                } else {
                    reopened.projectFiles.openURL(manual)
                    reopened.recovery.restore(recovered)
                }
                precondition(reopened.projectFiles.error == (recoveryFirst
                    ? "Finish the current document operation first"
                    : "Finish the current canvas operation before recovering a drawing"),
                    "External Open and recovery must reserve their destination before shared busy state arrives")
                reopened.projectFiles.error = nil
                try await wait("Replacement must ask about the current unsaved drawing") { reopened.projectFiles.confirming }
                reopened.projectFiles.choose("cancel")
                try await wait("Cancelled replacement did not settle") { !reopened.projectFiles.busy }
                precondition(reopened.state["document_file"]["epoch"].uint == 0 && reopened.state["layers"].array.count == 3)
                let cancelledReplacement = try await io { try files.current(recovered.scene) }
                precondition(cancelledReplacement == recovered)
            }
            reopened.recovery.restore(recovered)
            try await wait("Save before recovery prompt missing") { reopened.projectFiles.confirming }
            reopened.projectFiles.choose("save")
            try await wait("Recovery adoption did not finish") { reopened.state["document_file"]["epoch"].uint == 1 && !reopened.projectFiles.busy }
            precondition(FileManager.default.fileExists(atPath: manual.path))
            precondition(reopened.projectFiles.error == nil, reopened.projectFiles.error ?? "")
            precondition(reopened.state["layers"].array.count == 13)
            precondition(reopened.state["layers"].array.contains { $0["label"].string == "Recovery survivor" })
            precondition(reopened.state["document_file"]["modified"].bool && reopened.state["document_file"]["location"].isNull)
            let migrated = await flush(reopened); precondition(migrated)
            let remaining = try await io { try files.list().records }
            precondition(remaining.count == 1 && remaining[0].scene != recovered.scene)

            var closed: Bool?
            reopened.projectFiles.confirmClose { closed = $0 }
            try await wait("Recovered content did not require close confirmation") { reopened.projectFiles.confirming }
            reopened.projectFiles.choose("cancel")
            try await wait("Close cancellation did not complete") { closed != nil }
            precondition(closed == false)
            let cancelledClose = try await io { try files.list().records.count }
            precondition(cancelledClose == 1)
            closed = nil
            reopened.projectFiles.confirmClose { closed = $0 }
            try await wait("Discard confirmation missing") { reopened.projectFiles.confirming }
            reopened.projectFiles.choose("discard")
            try await wait("Discard did not authorize close") { closed == true }
            let removed = await withCheckedContinuation { continuation in reopened.recovery.close { continuation.resume(returning: $0) } }
            precondition(removed)
            let afterClose = try await io { try files.list().records }
            precondition(afterClose.isEmpty)
            withExtendedLifetime((layer, reopenedLayer)) {}
            print("Artwork recovery passes for Apple platform \(platform)")
        }
    }
}
