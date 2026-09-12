// Cross-queue Swift/JSON/Rust handoff for both Apple presets. No visible window,
// native menus, GPU or artist preferences are involved.
import Foundation

@main struct WorkspaceLibraryChecks {
    @MainActor static func main() async throws {
        for platform: UInt32 in [0, 1] {
            let directory = FileManager.default.temporaryDirectory.appendingPathComponent("capy-library-\(UUID().uuidString)")
            defer { try? FileManager.default.removeItem(at: directory) }
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            let library = try NativeWorkspaceLibrary(platform: platform, root: directory, scene: UUID().uuidString)
            let deadline = Date().addingTimeInterval(10)
            while store.state.isNull {
                guard Date() < deadline else { throw HostFailure(message: "Initial editor did not publish") }
                try await Task.sleep(for: .milliseconds(5))
            }
            func session(_ value: [String: Any]) async throws -> JSON {
                try await withCheckedThrowingContinuation { continuation in
                    store.native!.workspaceSession(JSON(value)) { value, error in
                        DispatchQueue.main.async {
                            if let error { continuation.resume(throwing: HostFailure(message: error)) }
                            else { continuation.resume(returning: value ?? JSON()) }
                        }
                    }
                }
            }
            func request(_ value: [String: Any]) async throws -> JSON {
                let reply: JSON = await withCheckedContinuation { continuation in
                    library.request(JSON(value)) { continuation.resume(returning: $0) }
                }
                if !reply["error"].isNull { throw HostFailure(message: reply["error"]["message"].string) }
                return reply
            }
            func edit(_ value: [String: Any]) async throws {
                try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                    store.edit(value) { error in
                        if let error { continuation.resume(throwing: HostFailure(message: error)) }
                        else { continuation.resume() }
                    }
                }
            }
            func adopt(_ reply: JSON) async throws -> String {
                let adoption = reply["value"]["adoption"]
                precondition(!adoption.isNull)
                _ = try await session(["type": "adopt", "capture": adoption["capture"].raw])
                let active = try await request(["type": "activate", "token": adoption["token"].raw])
                _ = try await session(["type": "configure", "binding": active["value"]["binding"].raw])
                _ = try await session(["type": "end"])
                return active["status"]["active_id"].string
            }
            let now = UInt64(Date().timeIntervalSince1970 * 1000)
            _ = try await session(["type": "begin"])
            let initial = try await request(["type": "initialize", "now": now])
            let original = try await adopt(initial)
            try await edit(["type": "set_brush_size", "value": 53])
            try await edit(["type": "customize", "action": ["type": "set_panel_visible", "panel": "navigator", "visible": false]])
            let snapshot = try await session(["type": "capture"])
            _ = try await request(["type": "observe", "capture": snapshot["capture"].raw, "working": snapshot["working"].raw, "now": now + 1])
            _ = try await request(["type": "flush"])
            _ = try await session(["type": "begin"])
            let incoming = try await request(["type": "operation", "operation": ["type": "new", "name": "Fresh", "template": "builtin:default"], "now": now + 2])
            let next = try await adopt(incoming)
            precondition(next != original && store.state["brush"]["diameter"].number != 53)
            _ = try await session(["type": "begin"])
            let returning = try await request(["type": "operation", "operation": ["type": "switch", "id": original], "now": now + 3])
            let restoredID = try await adopt(returning)
            precondition(restoredID == original)
            precondition(store.state["brush"]["diameter"].number == 53)
            let restored = try await session(["type": "capture"])
            precondition(restored["capture"]["history"]["current"].string == snapshot["capture"]["history"]["current"].string)
            let view = try await request(["type": "view", "page": "workspaces", "query": "", "selected": original, "idle": true, "now": now + 4])
            precondition(view["value"]["rows"].array.count == 2 && !view["value"]["details"]["actions"].array.isEmpty)
            _ = try await request(["type": "close"])
            print("PASS: platform \(platform), Swift workspace/storage queues preserve names, scene identity, sparse brush overrides and layout history across switching")
        }
    }
}
