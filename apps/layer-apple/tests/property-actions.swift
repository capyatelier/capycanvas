import Foundation

/// Replay the shared inventory through the real Apple editor/owner and decode
/// its published controls. No native windows, GPU, or user storage are accessed.
@main struct PropertyActionChecks {
    static func require(_ value: Bool, _ message: String) throws {
        if !value { throw HostFailure(message: message) }
    }
    @MainActor static func wait(_ label: String, until ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !ready() {
            try require(Date() < deadline, "Timed out: " + label)
            try await Task.sleep(for: .milliseconds(5))
        }
    }
    @MainActor static func edit(_ store: EditorStore, _ action: JSON) async -> String? {
        await withCheckedContinuation { continuation in
            store.edit(action.object) { continuation.resume(returning: $0) }
        }
    }
    @MainActor static func send(_ store: EditorStore, _ action: JSON) async throws {
        if let error = await edit(store, action) { throw HostFailure(message: error) }
    }
    @MainActor static func values(_ store: EditorStore) -> JSON {
        JSON(Dictionary(uniqueKeysWithValues: store.state["layer_properties"]["controls"].array.map {
            ($0["key"].string, $0["value"].raw)
        }))
    }
    @MainActor static func scenario(_ scenario: JSON, platform: UInt32, root: URL) async throws -> Int {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
        try await wait("workspace startup") { store.workspaceLibrary?.ready == true || store.workspaceLibrary?.error != nil }
        guard let library = store.workspaceLibrary, library.ready else {
            throw HostFailure(message: store.workspaceLibrary?.error ?? "Workspace not ready")
        }
        do {
            for action in scenario["setup"].array { try await send(store, action) }
            let expected = scenario["properties"]
            try require(store.state["layer_properties"]["controls"].stableKey == expected["controls"].stableKey,
                "Apple property schema/value mismatch: " + scenario["name"].string)
            try require(store.state["layer_properties"]["enabled"].bool, "Initial properties disabled")
            var count = 0
            for item in scenario["edits"].array {
                try require(item["error"].isNull && !item["result"].isNull, "Unresolved inventory edit")
                let key = item["key"].string, result = item["result"]
                let before = values(store)
                try await send(store, result["action"])
                let after = values(store)
                try require(after[key].stableKey == result["edited"].stableKey && before.stableKey != after.stableKey,
                    "Apple edit mismatch: " + key)
                try await send(store, JSON(["type": "invoke", "command": "undo"]))
                try require(values(store).stableKey == before.stableKey, "Undo failed to restore all properties: " + key)
                try await send(store, JSON(["type": "invoke", "command": "redo"]))
                try require(values(store).stableKey == after.stableKey, "Redo failed to restore all properties: " + key)
                try await send(store, JSON(["type": "effect", "action": ["op": "reset", "layer": expected["layer"].raw, "key": key]]))
                try require(values(store)[key].stableKey == result["reset"].stableKey, "Reset mismatch: " + key)
                count += 1
            }
            if !scenario["lock_action"].isNull {
                try await send(store, scenario["lock_action"])
                try require(!store.state["layer_properties"]["enabled"].bool, "Locked properties remain enabled")
                try require(store.state["layer_properties"]["controls"].stableKey == scenario["locked_properties"]["controls"].stableKey,
                    "Locked Apple controls changed their schema or values")
                if !scenario["locked_edit"].isNull {
                    let before = values(store)
                    let error = await edit(store, scenario["locked_edit"]["action"])
                    try require(error != nil && values(store).stableKey == before.stableKey,
                        "Locked Apple property edit was not rejected intact")
                }
            }
            try require(store.failure == nil && store.storageFailure == nil, store.failure ?? store.storageFailure ?? "")
            try await library.close()
            return count
        } catch {
            await library.detach()
            throw error
        }
    }
    @MainActor static func run() async throws {
        guard let path = ProcessInfo.processInfo.environment["CAPY_PROPERTY_INVENTORY"] else {
            throw HostFailure(message: "Set CAPY_PROPERTY_INVENTORY to the generated schema 4 inventory")
        }
        let inventory = try JSON.decode(String(contentsOfFile: path, encoding: .utf8))
        try require(inventory["schema"].uint == 4, "Expected inventory schema 4")
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-properties-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        for (platform, name): (UInt32, String) in [(0, "ios"), (1, "mac")] {
            let scenarios = inventory["platforms"][name]["property_scenarios"].array
            try require(scenarios.count == inventory["platforms"][name]["initial"]["state"]["adjustments"].array.count + 3,
                "Incomplete filter/layer property scenarios")
            var edits = 0
            for (index, item) in scenarios.enumerated() {
                edits += try await scenario(item, platform: platform, root: root.appendingPathComponent("\(name)-\(index)"))
            }
            print("PASS: \(name) Apple owner, \(scenarios.count) property schemas, \(edits) edit/undo/redo/reset routes and lock rejection")
        }
    }
    @MainActor static func main() async {
        do { try await run() }
        catch {
            FileHandle.standardError.write(Data("FAIL: \(error.localizedDescription)\n".utf8))
            exit(1)
        }
    }
}
