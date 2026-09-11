import Foundation

private final class Reply<Value>: @unchecked Sendable {
    private let ready = DispatchSemaphore(value: 0)
    private var value: Value?
    func set(_ value: Value) { self.value = value; ready.signal() }
    func get() -> Value {
        precondition(ready.wait(timeout: .now() + 15) == .success, "Owner callback timed out")
        return value!
    }
}
private final class State: @unchecked Sendable {
    private let lock = NSLock()
    private var snapshot = JSON()
    private var storage = JSON()
    private var failure: String?
    func receive(_ next: JSON?, _ error: String?) {
        lock.lock(); defer { lock.unlock() }
        if let error { failure = error }
        if let next {
            if !next["state"].isNull { snapshot = next }
            if !next["persistence"].isNull { storage = next["persistence"] }
        }
    }
    func read() -> (JSON, JSON, String?) { lock.lock(); defer { lock.unlock() }; return (snapshot, storage, failure) }
}
@main struct OwnerPersistenceChecks {
    static func main() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("capy-owner-\(UUID())")
        defer { try? FileManager.default.removeItem(at: directory) }
        func send(_ owner: NativeOwner, _ action: [String: Any]) {
            let result = Reply<Bool>()
            owner.submit(0, JSON(action)) { result.set($0 != nil) }
            precondition(result.get(), "Shared action must succeed")
        }
        func flush(_ owner: NativeOwner) -> Bool {
            let reply = Reply<Bool>(); owner.flushPersistence { reply.set($0) }; return reply.get()
        }
        for platform: UInt32 in [0,1] {
            let root = directory.appendingPathComponent("platform-\(platform)")
            let persistence = EditorPersistence(root: root)
            let sceneA = UUID().uuidString, sceneB = UUID().uuidString
            let stateA = State(), stateB = State()
            let a = try NativeOwner(platform: platform, scene: sceneA, persistence: persistence, receive: { stateA.receive($0, $1) })
            // Queue edits immediately: restoration must remain ahead of them.
            send(a, ["type":"set_theme", "theme":"dark"])
            send(a, ["type":"customize", "action":["type":"set_panel_visible", "panel":"color", "visible":true]])
            precondition(flush(a))
            let savedWorkspace = stateA.read().0["state"]["workspace"]
            let b = try NativeOwner(platform: platform, scene: sceneB, persistence: persistence, receive: { stateB.receive($0, $1) })
            precondition(flush(b))
            precondition(stateB.read().0["state"]["theme"].string == "dark")
            precondition(NSDictionary(dictionary: stateB.read().0["state"]["workspace"].object).isEqual(savedWorkspace.raw))
            // A new scene's own file must exist even before its first edit.
            let fileB = root.appendingPathComponent("workspaces/\(sceneB).json")
            precondition(FileManager.default.fileExists(atPath: fileB.path))

            send(a, ["type":"customize", "action":["type":"set_panel_visible", "panel":"color", "visible":false]])
            precondition(flush(a) && flush(b))
            let restoredState = State()
            let restored = try NativeOwner(platform: platform, scene: sceneB, persistence: persistence, receive: { restoredState.receive($0, $1) })
            precondition(flush(restored))
            precondition(NSDictionary(dictionary: restoredState.read().0["state"]["workspace"].object).isEqual(savedWorkspace.raw),
                "Restoring B must retain its workspace after A changes the default")

            // Rapid cross-window edits must converge to the final committed
            // settings, including after delayed notifications and write acks.
            for index in 0..<20 {
                (index.isMultiple(of: 2) ? a : b).submit(0, JSON(["type":"preferences", "action":["type":"edit", "id":"pressure", "value":1.0 + Double(index) / 20]]))
            }
            precondition(flush(a) && flush(b) && flush(a) && flush(restored))
            let disk = try JSON.decode(String(decoding: Data(contentsOf: root.appendingPathComponent("settings.json")), as: UTF8.self))
            for state in [stateA, stateB, restoredState] {
                precondition(state.read().2 == nil, state.read().2 ?? "")
                precondition(NSDictionary(dictionary: state.read().0["state"]["settings"].object).isEqual(disk.raw),
                    "All windows must converge to the persisted settings")
                precondition(state.read().0["state"]["requests"].array.isEmpty)
            }

            // Force an actual replacement failure, then retry the accepted
            // in-memory value after repairing the path. No GUI is involved.
            let settingsFile = root.appendingPathComponent("settings.json")
            try FileManager.default.removeItem(at: settingsFile)
            try FileManager.default.createDirectory(at: settingsFile, withIntermediateDirectories: false)
            send(a, ["type":"set_theme", "theme":"light"])
            precondition(!flush(a))
            precondition(stateA.read().0["state"]["theme"].string == "light", "Save failure must retain the accepted edit")
            precondition(stateA.read().1["can_retry"].bool && !stateA.read().1["error"].isNull)
            try FileManager.default.removeItem(at: settingsFile)
            a.retryPersistence(); precondition(flush(a) && flush(b))
            precondition(stateA.read().1["error"].isNull)
            precondition(stateB.read().0["state"]["theme"].string == "light")

            let unsupported = Data(#"{"version":999}"#.utf8)
            try AtomicJSONFile.write(unsupported, to: settingsFile)
            let invalidState = State()
            let invalid = try NativeOwner(platform: platform, scene: UUID().uuidString, persistence: persistence,
                receive: { invalidState.receive($0, $1) })
            precondition(!flush(invalid))
            precondition(invalidState.read().2 == nil && !invalidState.read().1["error"].isNull,
                "Invalid saved models must report a storage error without disabling the canvas")
            let preserved = try Data(contentsOf: settingsFile)
            precondition(preserved == unsupported, "Defaults must not overwrite an unsupported saved version")
        }
        print("Native owner persistence passed on both platform configurations: restore ordering, scene isolation, concurrent settings, durable acknowledgments and failed-save retry")
    }
}
