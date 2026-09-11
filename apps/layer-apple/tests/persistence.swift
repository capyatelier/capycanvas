import Foundation

private func check(_ condition: Bool, _ message: String = "Persistence assertion failed") { precondition(condition, message) }

private final class ResultBox<Value>: @unchecked Sendable {
    private let lock = NSLock()
    private var result: Value?
    private let ready = DispatchSemaphore(value: 0)
    func set(_ value: Value) { lock.lock(); result = value; lock.unlock(); ready.signal() }
    func get() -> Value {
        precondition(ready.wait(timeout: .now() + 10) == .success, "Persistence callback timed out")
        lock.lock(); defer { lock.unlock() }; return result!
    }
}

@main struct PersistenceChecks {
    static func main() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("capy-persistence-\(UUID())")
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("settings.json")
        check(try AtomicJSONFile.read(file) == nil)
        let first = Data(#"{"version":1,"theme":"dark"}"#.utf8)
        let second = Data(#"{"version":1,"theme":"light"}"#.utf8)
        try AtomicJSONFile.write(first, to: file)
        check(try AtomicJSONFile.read(file) == first)
        let attributes = try FileManager.default.attributesOfItem(atPath: file.path)
        precondition((attributes[.posixPermissions] as? NSNumber)?.intValue == 0o600)
        do {
            try AtomicJSONFile.write(Data(repeating: 0, count: AtomicJSONFile.maximumBytes + 1), to: file)
            preconditionFailure("Oversized writes must be rejected")
        } catch { check(try AtomicJSONFile.read(file) == first, "Failed save must retain the prior file") }
        let oversized = directory.appendingPathComponent("oversized.json")
        try Data(repeating: 32, count: AtomicJSONFile.maximumBytes + 1).write(to: oversized)
        do { _ = try AtomicJSONFile.read(oversized); preconditionFailure("Oversized reads must be rejected") }
        catch { }

        // Readers must observe a complete old or new generation, never a
        // partial write. This uses the real filesystem and atomic replacement.
        let done = DispatchGroup(); done.enter()
        let reader = ResultBox<Bool>()
        DispatchQueue.global().async {
            defer { done.leave() }
            var valid = true
            for _ in 0..<500 {
                do { let data = try AtomicJSONFile.read(file); valid = valid && (data == first || data == second) }
                catch { valid = false }
            }
            reader.set(valid)
        }
        for index in 0..<30 { try AtomicJSONFile.write(index.isMultiple(of: 2) ? second : first, to: file) }
        done.wait(); precondition(reader.get(), "Concurrent readers saw an incomplete generation")
        check(try FileManager.default.contentsOfDirectory(atPath: directory.path).allSatisfy { !$0.hasSuffix(".tmp") })

        let persistence = EditorPersistence(root: directory)
        let sceneA = UUID().uuidString, sceneB = UUID().uuidString
        let workspaceA = Data(#"{"version":1,"marker":"A"}"#.utf8)
        let workspaceB = Data(#"{"version":1,"marker":"B"}"#.utf8)
        func saveWorkspace(_ value: Data, _ scene: String) {
            let result = ResultBox<String?>()
            persistence.saveWorkspace(value, scene: scene) { result.set($0) }
            precondition(result.get() == nil)
        }
        func load(_ scene: String) -> EditorPersistence.Loaded {
            let result = ResultBox<EditorPersistence.Loaded>()
            persistence.load(scene: scene, observer: UUID(), changed: { _ in }) { result.set($0) }
            return result.get()
        }
        saveWorkspace(workspaceA, sceneA); saveWorkspace(workspaceB, sceneB)
        precondition(load(sceneA).workspace == workspaceA && load(sceneB).workspace == workspaceB,
            "Existing scenes must retain independent workspaces")
        precondition(load(UUID().uuidString).workspace == workspaceB, "New scenes inherit the last committed workspace")

        let notification = ResultBox<EditorPersistence.SettingsChange>()
        let loaded = ResultBox<EditorPersistence.Loaded>()
        persistence.load(scene: sceneA, observer: UUID(), changed: { notification.set($0) }) { loaded.set($0) }
        _ = loaded.get()
        let saved = ResultBox<String?>()
        persistence.saveSettings(second) { saved.set($0) }
        precondition(saved.get() == nil && notification.get().data == second)
        let flushed = ResultBox<Bool>(); persistence.flush { flushed.set(true) }; precondition(flushed.get())
        precondition(load(sceneA).settings == second, "Acknowledgment must follow durable replacement")

        let corrupt = directory.appendingPathComponent("workspaces/\(sceneA).json")
        try Data("broken".utf8).write(to: corrupt)
        let failed = load(sceneA)
        precondition(failed.workspace == nil && failed.errors["workspace"] != nil,
            "A corrupt scene must report failure rather than silently load another workspace")
        check(try Data(contentsOf: corrupt) == Data("broken".utf8), "Loading must not overwrite corrupt data")
        precondition(load(sceneB).workspace == workspaceB)

        let blocked = directory.appendingPathComponent("blocked")
        try first.write(to: blocked)
        let unavailable = EditorPersistence(root: blocked)
        let failure = ResultBox<String?>(); unavailable.saveSettings(second) { failure.set($0) }
        precondition(failure.get() != nil, "Write failure must reach its completion")
        check(try Data(contentsOf: blocked) == first)
        print("Persistence checks passed: atomic generations, private files, failure preservation, scene isolation, settings notifications and flush ordering")
    }
}
