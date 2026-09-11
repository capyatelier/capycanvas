import Foundation
import AppKit
import QuartzCore

@main struct ProjectFileChecks {
    @MainActor static func wait(_ description: String, _ condition: () -> Bool) async throws {
        let end = Date().addingTimeInterval(45)
        while !condition() {
            precondition(Date() < end, description)
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func main() async throws {
        setenv("CAPY_DISABLE_PERSISTENCE", "1", 1)
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
            let store = EditorStore(platform: platform)
            let target = root.appendingPathComponent("drawing-\(platform).capy")
            let invalid = root.appendingPathComponent("invalid-\(platform).capy")
            try Data("incomplete".utf8).write(to: invalid)
            var saveLocation: URL? = target
            var openLocation: URL? = target
            var choices = 0
            var beforeOpen: (() -> Void)?
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(
                open: { choices += 1; beforeOpen?(); $0(openLocation) },
                save: { _, callback in choices += 1; callback(saveLocation) }))
            let layer = CAMetalLayer(); layer.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
            store.native!.attach(layer, width: 128, height: 128, scale: 1)
            let attached = await withCheckedContinuation { continuation in
                store.native!.submit(2, JSON(["type":"catalog"])) { continuation.resume(returning: $0 != nil) }
            }
            precondition(attached && store.failure == nil, store.failure ?? "Attach failed")
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
            precondition(store.state["project_file"]["title"].string == target.lastPathComponent)
            let mode = try FileManager.default.attributesOfItem(atPath: target.path)[.posixPermissions] as! NSNumber
            precondition(mode.intValue & 0o777 == 0o600)
            store.invoke("add_layer")
            try await wait("Layer edit not applied") { store.state["layers"].array.count == 3 }
            let id = store.state["layer_tools"]["editing_layer"]["id"].uint
            store.layer(["op":"rename", "id":id, "name":"Saved layer"])
            try await wait("Rename not applied") { store.state["layers"].array.contains { $0["label"].string == "Saved layer" } }
            try await invoke("save_document")
            precondition(!store.state["project_file"]["modified"].bool)
            let savedData = try Data(contentsOf: target)
            saveLocation = nil
            try await invoke("save_document_as")
            precondition((try? Data(contentsOf: target)) == savedData)
            store.invoke("add_layer")
            try await wait("Unsaved edit not applied") { store.state["layers"].array.count == 4 }
            let epoch = store.state["project_file"]["epoch"].uint
            store.invoke("new_document")
            try await wait("Unsaved prompt missing") { store.projectFiles.confirming }
            store.projectFiles.choose("cancel")
            try await wait("Cancel not acknowledged") { !store.projectFiles.busy }
            precondition(store.state["project_file"]["epoch"].uint == epoch && store.state["layers"].array.count == 4)
            store.invoke("new_document")
            try await wait("Discard prompt missing") { store.projectFiles.confirming }
            store.projectFiles.choose("discard")
            try await wait("New drawing failed") { !store.projectFiles.busy }
            precondition(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            precondition(store.state["layers"].array.count == 2 && store.state["project_file"]["epoch"].uint == epoch + 1)
            openLocation = invalid
            try await invoke("open_document")
            precondition(store.projectFiles.error != nil)
            precondition(store.state["layers"].array.count == 2)
            store.projectFiles.error = nil; openLocation = target
            beforeOpen = { store.invoke("add_layer") }
            try await invoke("open_document")
            precondition(store.projectFiles.error != nil, "An edit queued before capture must prevent replacement")
            precondition(store.state["layers"].array.count == 3)
            beforeOpen = nil; store.projectFiles.error = nil
            store.invoke("open_document")
            try await wait("Intervening changes must prompt") { store.projectFiles.confirming }
            store.projectFiles.choose("discard")
            try await wait("Confirmed open did not settle") { !store.projectFiles.busy }
            precondition(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            precondition(store.state["layers"].array.contains { $0["label"].string == "Saved layer" })
            precondition(!store.state["project_file"]["modified"].bool)
            store.invoke("add_layer")
            var closed: Bool?
            store.projectFiles.confirmClose { closed = $0 }
            try await wait("Close prompt missing") { store.projectFiles.confirming }
            store.projectFiles.choose("cancel")
            precondition(closed == false)
            closed = nil
            store.projectFiles.confirmClose { closed = $0 }
            try await wait("Save on close prompt missing") { store.projectFiles.confirming }
            store.projectFiles.choose("save")
            try await wait("Save on close did not finish") { closed != nil }
            precondition(closed == true && !store.state["project_file"]["modified"].bool)
            precondition(choices == 5, "Each user request must present at most one location choice")
            withExtendedLifetime(layer) {}
            print("Project files pass for Apple platform \(platform)")
        }
    }
}
