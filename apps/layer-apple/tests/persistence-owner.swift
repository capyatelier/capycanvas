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
    private var publications = 0
    func receive(_ next: JSON?, _ error: String?) {
        lock.lock(); defer { lock.unlock() }
        if let error { failure = error }
        if let next {
            if !next["state"].isNull { snapshot = next; publications += 1 }
            if !next["persistence"].isNull { storage = next["persistence"] }
        }
    }
    func read() -> (JSON, JSON, String?) { lock.lock(); defer { lock.unlock() }; return (snapshot, storage, failure) }
    func publicationCount() -> Int { lock.lock(); defer { lock.unlock() }; return publications }
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
        func checkBubblePublication(platform: UInt32) throws {
            let state = State()
            let owner = try NativeOwner(platform: platform, persistence: EditorPersistence(root: nil),
                receive: { state.receive($0, $1) })
            owner.resize(width: 1200, height: 870, scale: 1)
            send(owner, ["type": "set_theme", "theme": "light"])
            send(owner, ["type": "customize", "action": ["type": "header", "action": ["type": "add", "zone": "center",
                "before": NSNull(), "item": ["kind": "tool", "control": ["kind": "command", "command": "drawing_brush"]]]]])
            send(owner, ["type": "customize", "action": ["type": "header", "action": ["type": "edit", "editing": false]]])
            func query(_ value: [String: Any]) -> JSON {
                let reply = Reply<JSON?>(); owner.submit(2, JSON(value)) { reply.set($0) }
                return reply.get()!
            }
            func frame() {
                let reply = Reply<Bool>(), now = FrameTrace.now()
                owner.frame(now: now, target: now + 16_666_667) { _, _, _ in reply.set(true) }
                precondition(reply.get() && state.read().2 == nil)
            }
            guard let source = state.read().0["header"]["model"]["zones"].array.flatMap(\.array).first(where: {
                $0["item"]["control"]["command"].string == "drawing_brush"
            }) else { throw HostFailure(message: "Windowless owner must expose the drawing brush header source") }
            for phase in ["down", "up"] {
                send(owner, ["type": "bubble", "action": ["type": "drag", "grab": ["kind": "header", "id": source["id"].uint],
                    "phase": phase, "position": [420, 340], "viewport": [1200, 870]]])
            }
            let bubble = query(["type": "bubbles"])[0]["view"], id = bubble["id"].uint
            guard let ring = bubble["rings"].array.first(where: { $0["enabled"].bool }) else {
                throw HostFailure(message: "Windowless owner must expose an enabled brush bubble ring")
            }
            let arc = ring["arc"]
            func action(_ phase: String, _ fraction: Double) {
                let angle = (arc["start"].number + arc["sweep"].number * fraction) * .pi / 180
                send(owner, ["type": "bubble", "action": ["type": "ring", "id": id, "phase": phase,
                    "position": [cos(angle) * arc["radius"].number, sin(angle) * arc["radius"].number]]])
            }
            let before = state.read().0["state"]["tool_settings"].stableKey
            func history() -> [JSON] { state.read().0["state"]["commands"].array.filter { ["undo", "redo"].contains($0["id"].string) } }
            precondition(history().count == 2 && history().allSatisfy { !$0["enabled"].bool })
            owner.framePublication(active: true)
            let initial = state.publicationCount()
            action("down", 0.2)
            precondition(state.publicationCount() == initial + 1, "Ring Down must publish before acknowledgment")
            let down = state.publicationCount()
            for index in 0..<12 { action("move", 0.3 + Double(index) * 0.04) }
            let latest = query(["type": "bubbles"])[0]["view"]["rings"].stableKey
            _ = query(["type": "catalog"])
            guard state.publicationCount() == down else {
                throw HostFailure(message: "Moves and pure queries must wait for the display frame")
            }
            frame()
            precondition(state.publicationCount() == down + 1
                && state.read().0["bubbles"][0]["view"]["rings"].stableKey == latest,
                "One frame must publish the latest ordered ring value")
            frame()
            precondition(state.publicationCount() == down + 1, "An empty frame must not republish the ring")
            let edited = state.read().0["state"]["tool_settings"].stableKey
            action("up", 0.74)
            precondition(state.publicationCount() == down + 2
                && state.read().0["state"]["tool_settings"].stableKey == edited && edited != before
                && history().allSatisfy { !$0["enabled"].bool },
                "Ring Up must publish immediately and retain the last move value")
            action("down", 0.4); action("move", 0.6); action("cancel", 0.6)
            precondition(state.read().0["state"]["tool_settings"].stableKey == edited && state.read().2 == nil,
                "Cancel must immediately restore the exact value from before Down")
            action("down", 0.4); action("move", 0.6)
            let pending = query(["type": "bubbles"])[0]["view"]["rings"].stableKey, active = state.publicationCount()
            owner.framePublication(active: false)
            _ = query(["type": "catalog"])
            precondition(state.publicationCount() == active + 1
                && state.read().0["bubbles"][0]["view"]["rings"].stableKey == pending,
                "Stopping display frames must immediately publish the pending move")
            action("move", 0.7)
            precondition(state.publicationCount() == active + 2
                && state.read().0["bubbles"][0]["view"]["rings"].stableKey != pending,
                "Moves without display frames must publish before acknowledgment")
            action("cancel", 0.7)
            precondition(state.read().0["state"]["tool_settings"].stableKey == edited && state.read().2 == nil
                && history().allSatisfy { !$0["enabled"].bool },
                "Cancel must restore the exact value without adding document Undo")
            print("PASS bubble ring publication platform \(platform): ordered moves, query isolation, frame stop, Up, Cancel and no document Undo")
        }
        func checkCatalogTrace(platform: UInt32) throws {
            let output = directory.appendingPathComponent("trace-\(platform)")
            let previous = ProcessInfo.processInfo.environment["CAPY_TRACE_DIRECTORY"]
            setenv("CAPY_TRACE_DIRECTORY", output.path, 1)
            defer {
                if let previous { setenv("CAPY_TRACE_DIRECTORY", previous, 1) }
                else { unsetenv("CAPY_TRACE_DIRECTORY") }
            }
            let state = State()
            var owner: NativeOwner? = try NativeOwner(platform: platform, persistence: EditorPersistence(root: nil),
                traceDuration: 30, receive: { state.receive($0, $1) })
            let reply = Reply<Bool>(), now = FrameTrace.now()
            owner!.frame(now: now, target: now + 16_666_667) { _, _, _ in reply.set(true) }
            precondition(reply.get() && state.read().2 == nil, "The traced native owner must publish without a frame error")
            owner = nil
            let deadline = Date().addingTimeInterval(10)
            var files: [URL] = []
            while files.isEmpty {
                files = (try? FileManager.default.contentsOfDirectory(at: output, includingPropertiesForKeys: nil))?
                    .filter { $0.pathExtension == "jsonl" } ?? []
                precondition(Date() < deadline, "The native owner must finish its private trace")
                if files.isEmpty { Thread.sleep(forTimeInterval: 0.01) }
            }
            let lines = try String(contentsOf: files[0], encoding: .utf8).split(separator: "\n").dropFirst()
            let events = try lines.map { try JSONSerialization.jsonObject(with: Data($0.utf8)) as! [UInt64] }
            precondition(events.contains { $0[0] == UInt64(FrameTraceEvent.Kind.state.rawValue) && $0[3] & 2 != 0 },
                "The actual native trace must report its loaded catalog")
            print("PASS native catalog trace platform \(platform)")
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
            var kinds = Set<String>()
            let sampled = rows.filter { row in
                let type = row["kind"]["type"].string
                return ["switch", "choice", "number"].contains(type) && row["enabled"].bool && row["visible"].bool
                    && kinds.insert(type).inserted
            }
            precondition(sampled.count == 3, "Settings must publish an editable switch, choice and number")
            for row in sampled {
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
                let original = current(), kind = original["kind"], baseline = state.read().0["state"]["settings"].stableKey
                let value: Any
                switch kind["type"].string {
                case "switch": value = !kind["active"].bool
                case "choice": value = (Int(kind["selected"].uint) + 1) % kind["options"].array.count
                case "number": value = kind["control"]["max"].number
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
                let disk = try JSON.decode(String(decoding: Data(contentsOf: persistence.locations!.settings), as: UTF8.self))
                precondition(disk.stableKey == baseline && !current()["reset"]["enabled"].bool,
                    "Reset must be durable: \(id)")
                print("PASS preference platform \(platform): \(id) edit, fresh-owner restore and exact durable Reset")
            }
            fflush(stdout)
        }
        func checkPressure(platform: UInt32) throws {
            let persistence = EditorPersistence(root: directory.appendingPathComponent("pressure-\(platform)")), state = State()
            let owner = try NativeOwner(platform: platform, persistence: persistence, receive: { state.receive($0, $1) })
            send(owner, ["type": "invoke", "command": "pen_pressure"])
            let original = state.read().0["state"]["settings"].stableKey
            let points = state.read().0["state"]["pressure_calibration"]["editor"]["points"].stableKey
            func calibration(_ kind: String) { send(owner, ["type": "pressure_calibration", "action": ["kind": kind]]) }
            func alter() {
                send(owner, ["type": "pressure_calibration", "action": ["kind": "sensitivity", "lighter": false]])
                precondition(state.read().0["state"]["pressure_calibration"]["editor"]["points"].stableKey != points,
                    "Firmer must alter the actual pressure response")
            }
            alter(); calibration("cancel")
            precondition(state.read().0["state"]["settings"].stableKey == original && flush(owner),
                "Cancel must preserve every saved preference")
            send(owner, ["type": "invoke", "command": "pen_pressure"])
            precondition(state.read().0["state"]["pressure_calibration"]["editor"]["points"].stableKey == points,
                "Reopening after Cancel must restore the exact original response")
            alter()
            let chosen = state.read().0["state"]["pressure_calibration"]["editor"]["points"].stableKey
            calibration("apply")
            precondition(flush(owner))
            let edited = state.read().0["state"]["settings"], disk = try JSON.decode(String(decoding:
                Data(contentsOf: persistence.locations!.settings), as: UTF8.self))
            precondition(edited.stableKey != original && disk.stableKey == edited.stableKey,
                "Apply must durably save the edited response")
            let restoredState = State()
            let restored = try NativeOwner(platform: platform, persistence: persistence, receive: { restoredState.receive($0, $1) })
            send(restored, ["type": "invoke", "command": "pen_pressure"])
            precondition(restoredState.read().0["state"]["settings"].stableKey == edited.stableKey
                && restoredState.read().0["state"]["pressure_calibration"]["editor"]["points"].stableKey == chosen,
                "A fresh owner must restore the exact applied pressure response")
            precondition(state.read().2 == nil && restoredState.read().2 == nil && flush(restored))
            print("PASS pressure platform \(platform): exact Cancel restoration and Apply/flush/fresh-owner curve persistence")
        }
        func checkTilt(platform: UInt32, theme: String) throws {
            let persistence = EditorPersistence(root: directory.appendingPathComponent("tilt-\(platform)-\(theme)")), state = State()
            let owner = try NativeOwner(platform: platform, persistence: persistence, receive: { state.receive($0, $1) })
            send(owner, ["type": "set_theme", "theme": theme])
            let baseline = state.read().0["state"]["settings"].stableKey
            func command(_ owner: NativeOwner) { send(owner, ["type": "invoke", "command": "pen_tilt"]) }
            func calibration(_ kind: String) {
                send(owner, ["type": "tilt_calibration", "action": ["kind": "panel", "action": ["kind": kind]]])
            }
            func sensor(_ owner: NativeOwner, id: UInt64) {
                send(owner, ["type": "pen_input", "device": ["id": id, "key": "apple-fixture:pencil", "label": "Fixture Pencil",
                    "native_to_surface": ["reverse_horizontal": false, "reverse_vertical": false, "swap": false]],
                    "sample": [400.0, 300.0, 0.6, 0.4, 0.2, 0.0, 0.0, 1_000_000_000.0, 0.0]])
            }
            func alter() {
                send(owner, ["type": "tilt_calibration", "action": ["kind": "panel", "action": ["kind": "sensitivity", "lighter": true]]])
                send(owner, ["type": "tilt_calibration", "action": ["kind": "axes", "axes": [
                    "reverse_horizontal": true, "reverse_vertical": false, "swap": true]]])
            }
            command(owner)
            precondition(!state.read().0["state"]["tilt_calibration"]["ready"].bool)
            let original = state.read().0["state"]["tilt_calibration"]["editor"]["points"].stableKey
            let camera = state.read().0["state"]["camera"].stableKey, layers = state.read().0["state"]["layers"].stableKey
            alter(); sensor(owner, id: 0)
            precondition(state.read().0["state"]["tilt_calibration"]["ready"].bool
                && state.read().0["state"]["tilt_calibration"]["device"].string == "Fixture Pencil")
            precondition(state.read().0["state"]["camera"].stableKey == camera && state.read().0["state"]["layers"].stableKey == layers
                && !state.read().0["state"]["document_file"]["modified"].bool,
                "Owner-routed dialog sensor samples must preserve the drawing and camera")
            calibration("cancel")
            precondition(state.read().0["state"]["settings"].stableKey == baseline && flush(owner))
            command(owner)
            precondition(state.read().0["state"]["tilt_calibration"]["editor"]["points"].stableKey == original)
            alter()
            let response = state.read().0["state"]["tilt_calibration"]["editor"]["points"].stableKey
            let axes = state.read().0["state"]["tilt_calibration"]["axes"].stableKey
            calibration("apply"); precondition(flush(owner))
            let settings = state.read().0["state"]["settings"]
            let disk = try JSON.decode(String(decoding: Data(contentsOf: persistence.locations!.settings), as: UTF8.self))
            precondition(disk.stableKey == settings.stableKey && settings.stableKey != baseline)
            let restoredState = State()
            let restored = try NativeOwner(platform: platform, persistence: persistence, receive: { restoredState.receive($0, $1) })
            command(restored); sensor(restored, id: 900)
            precondition(restoredState.read().0["state"]["tilt_calibration"]["editor"]["points"].stableKey == response
                && restoredState.read().0["state"]["tilt_calibration"]["axes"].stableKey == axes,
                "A fresh render owner must restore response and axes for the stable Pencil source despite a new contact alias")
            send(restored, ["type": "curve_editor", "target": ["kind": "tilt"], "action": ["kind": "reset"]])
            precondition(restoredState.read().0["state"]["tilt_calibration"]["editor"]["points"].stableKey == original)
            send(restored, ["type": "tilt_calibration", "action": ["kind": "panel", "action": ["kind": "cancel"]]])
            precondition(restoredState.read().0["state"]["settings"].stableKey == settings.stableKey
                && state.read().2 == nil && restoredState.read().2 == nil && flush(restored))
            print("PASS tilt platform \(platform) \(theme): cold sensor binding, Cancel/Reset rollback, Apply/flush and fresh-owner stable-source persistence")
        }
        for platform: UInt32 in [0,1] {
            try checkBubblePublication(platform: platform)
            try checkCatalogTrace(platform: platform)
            try checkPreferenceRows(platform: platform)
            try checkPressure(platform: platform)
            for theme in ["light", "dark"] { try checkTilt(platform: platform, theme: theme) }
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

            // Rapid cross-window edits must converge to the final committed
            // settings, including after delayed notifications and write acks.
            for index in 0..<20 {
                (index.isMultiple(of: 2) ? a : b).submit(0, JSON(["type":"preferences", "action":["type":"edit", "id":"pan_speed", "value":1.0 + Double(index) / 20]]))
            }
            precondition(flush(a) && flush(b) && flush(a) && flush(restored))
            let disk = try JSON.decode(String(decoding: Data(contentsOf: persistence.locations!.settings), as: UTF8.self))
            for state in [stateA, stateB, restoredState] {
                precondition(state.read().2 == nil, state.read().2 ?? "")
                precondition(NSDictionary(dictionary: state.read().0["state"]["settings"].object).isEqual(disk.raw),
                    "All windows must converge to the persisted settings")
                precondition(state.read().0["state"]["requests"].array.isEmpty)
            }

            // Force an actual replacement failure, then retry the accepted
            // in-memory value after repairing the path. No GUI is involved.
            let settingsFile = persistence.locations!.settings
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
            let invalid = try NativeOwner(platform: platform, persistence: EditorPersistence(root: root),
                receive: { invalidState.receive($0, $1) })
            precondition(flush(invalid))
            precondition(invalidState.read().2 == nil && invalidState.read().1["error"].isNull,
                "Settings from another version must restore without a storage error")
            let preserved = try Data(contentsOf: settingsFile)
            precondition(preserved == unsupported, "Defaults must not overwrite an unsupported saved version")

            let corrupt = Data(repeating: 0xff, count: AtomicJSONFile.maximumBytes + 1)
            try corrupt.write(to: settingsFile)
            let corruptState = State()
            let corruptOwner = try NativeOwner(platform: platform, persistence: EditorPersistence(root: root),
                receive: { corruptState.receive($0, $1) })
            precondition(flush(corruptOwner), "An optional settings read warning must not block orderly persistence flush")
            precondition(corruptState.read().2 == nil && !corruptState.read().1["error"].string.isEmpty,
                "Settings read failure must remain visible without invalidating the native owner")
            precondition(corruptState.read().0["state"]["settings"].stableKey == invalidState.read().0["state"]["settings"].stableKey,
                "An unreadable settings file must launch with complete shared defaults")
            let retained = try Data(contentsOf: settingsFile)
            precondition(retained == corrupt, "Default launch and successful flush must preserve unreadable settings bytes")
            print("PASS optional settings warning platform \(platform): defaults, visible cause, orderly flush and unchanged corrupt bytes")
        }
        print("Native owner persistence passed on both platform configurations: restore ordering, settings-only storage, concurrent settings, durable acknowledgments and failed-save retry")
    }
}
