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
        func preferenceRows(_ state: State) -> [JSON] {
            state.read().0["preferences"]["pages"].array.flatMap { page in
                page["groups"].array.flatMap { $0["rows"].array }
            }
        }
        func preferenceValue(_ row: JSON) -> JSON {
            let kind = row["kind"]
            switch kind["type"].string {
            case "switch": return kind["active"]
            case "choice": return kind["selected"]
            default: return kind["value"]
            }
        }
        func checkPreferenceRows(platform: UInt32) throws {
            let inventory = State()
            let catalogOwner = try NativeOwner(platform: platform, persistence: EditorPersistence(root: nil),
                receive: { inventory.receive($0, $1) })
            send(catalogOwner, ["type": "invoke", "command": "settings"])
            let rows = preferenceRows(inventory)
            precondition(!rows.isEmpty, "Settings inventory must be published")
            var checked = 0
            for row in rows where ["switch", "choice", "number", "text"].contains(row["kind"]["type"].string) {
                let id = row["id"].string
                let root = directory.appendingPathComponent("preferences-\(platform)/\(id)")
                let persistence = EditorPersistence(root: root), state = State()
                let owner = try NativeOwner(platform: platform, persistence: persistence,
                    receive: { state.receive($0, $1) })
                send(owner, ["type": "invoke", "command": "settings"])
                func action(_ type: String, value: Any? = nil) {
                    var action: [String: Any] = ["type": type, "id": id]
                    if let value { action["value"] = value }
                    send(owner, ["type": "preferences", "action": action])
                }
                func current(_ value: State = state) -> JSON { preferenceRows(value).first { $0["id"].string == id }! }
                // UIKit's native lookahead hides the manual amount. Test its
                // exposed manual state, without changing the other cases.
                if platform == 0 && id == "prediction_horizon" {
                    send(owner, ["type": "preferences", "action": ["type": "edit", "id": "platform_prediction", "value": false]])
                }
                let original = current(), kind = original["kind"], baseline = state.read().0["state"]["settings"].stableKey
                if !original["enabled"].bool {
                    precondition(platform == 1 && id == "platform_prediction" && !kind["active"].bool,
                        "An unaccounted disabled preference requires an explicit acceptance case")
                    print("PASS preference platform \(platform): \(id) is off/disabled because native prediction is unavailable")
                    continue
                }
                precondition(original["visible"].bool, "Editable preference must be reachable: \(id)")
                let value: Any
                switch kind["type"].string {
                case "switch": value = !kind["active"].bool
                case "choice": value = (Int(kind["selected"].uint) + 1) % kind["options"].array.count
                case "number": value = kind["control"]["max"].number
                case "text":
                    precondition(kind["constraint"].string == "hex_color", "Unaccounted text constraint: \(id)")
                    value = "#123456"
                default: preconditionFailure("Unaccounted editable preference kind")
                }
                action("edit", value: value)
                precondition(state.read().0["preferences"]["error"].isNull && state.read().2 == nil)
                let edited = preferenceValue(current()).stableKey
                precondition(edited == JSON(value).stableKey && edited != preferenceValue(original).stableKey,
                    "The exact row ID must update its displayed value: \(id)")
                precondition(current()["reset"]["enabled"].bool && flush(owner), "Edited preference must save and enable Reset: \(id)")
                let restoredState = State()
                let restored = try NativeOwner(platform: platform, persistence: persistence,
                    receive: { restoredState.receive($0, $1) })
                send(restored, ["type": "invoke", "command": "settings"])
                precondition(preferenceValue(current(restoredState)).stableKey == edited,
                    "A fresh owner must restore the edited preference: \(id)")
                action("reset")
                precondition(state.read().0["preferences"]["error"].isNull && flush(owner) && flush(restored))
                precondition(state.read().0["state"]["settings"].stableKey == baseline,
                    "Reset must restore the complete baseline without changing unrelated preferences: \(id)")
                let disk = try JSON.decode(String(decoding: Data(contentsOf: root.appendingPathComponent("settings.json")), as: UTF8.self))
                precondition(disk.stableKey == baseline && !current()["reset"]["enabled"].bool,
                    "Reset must be durable: \(id)")
                checked += 1
                print("PASS preference platform \(platform): \(id) edit, fresh-owner restore and exact durable Reset")
            }
            print("PASS platform \(platform): \(rows.count) Settings rows enumerated; \(checked) editable preference routes restored and reset")
            fflush(stdout)
        }
        for platform: UInt32 in [0,1] {
            try checkPreferenceRows(platform: platform)
            let root = directory.appendingPathComponent("platform-\(platform)")
            let persistence = EditorPersistence(root: root)
            let stateA = State(), stateB = State()
            let a = try NativeOwner(platform: platform, persistence: persistence, receive: { stateA.receive($0, $1) })
            // Queue edits immediately: restoration must remain ahead of them.
            send(a, ["type":"set_theme", "theme":"dark"])
            send(a, ["type":"customize", "action":["type":"set_panel_visible", "panel":"color", "visible":true]])
            precondition(flush(a))
            let b = try NativeOwner(platform: platform, persistence: persistence, receive: { stateB.receive($0, $1) })
            precondition(flush(b))
            precondition(stateB.read().0["state"]["theme"].string == "dark")
            send(a, ["type":"customize", "action":["type":"set_panel_visible", "panel":"color", "visible":false]])
            precondition(flush(a) && flush(b))
            let restoredState = State()
            let restored = try NativeOwner(platform: platform, persistence: persistence, receive: { restoredState.receive($0, $1) })
            precondition(flush(restored))
            precondition(restoredState.read().0["state"]["theme"].string == "dark")
            precondition(!FileManager.default.fileExists(atPath: root.appendingPathComponent("workspace.json").path)
                && !FileManager.default.fileExists(atPath: root.appendingPathComponent("workspaces").path),
                "The drawing owner must not write a second workspace store")

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
            let invalid = try NativeOwner(platform: platform, persistence: persistence,
                receive: { invalidState.receive($0, $1) })
            precondition(!flush(invalid))
            precondition(invalidState.read().2 == nil && !invalidState.read().1["error"].isNull,
                "Invalid saved models must report a storage error without disabling the canvas")
            let preserved = try Data(contentsOf: settingsFile)
            precondition(preserved == unsupported, "Defaults must not overwrite an unsupported saved version")
        }
        print("Native owner persistence passed on both platform configurations: restore ordering, settings-only storage, concurrent settings, durable acknowledgments and failed-save retry")
    }
}
