import Foundation

/// The SQLite manager never runs on MainActor or the drawing owner. Each window
/// has a serial manager owner; Rust shares one storage worker for its directory.
final class NativeWorkspaceLibrary: @unchecked Sendable {
    private let queue = DispatchQueue(label: "art.capycanvas.workspace-library", qos: .utility)
    private let handle: OpaquePointer
    init(platform: UInt32, root: URL, scene: String) throws {
        let key = "apple:scene:" + (UUID(uuidString: scene)?.uuidString ?? "default")
        let result = queue.sync {
            root.path.withCString { directory in
                key.withCString { capy_workspace_library_create(platform, directory, $0) }
            }
        }
        guard let result else { throw HostFailure(message: "Could not start workspace storage") }
        handle = result
    }
    deinit {
        let handle = handle
        queue.async {
            // A discarded system scene can bypass explicit close. Release its
            // claim on the storage queue without overwriting the saved copy.
            if let reply = "{\"type\":\"detach\"}".withCString({ capy_workspace_library_request(handle, $0) }) {
                capy_apple_string_free(reply)
            }
            capy_workspace_library_destroy(handle)
        }
    }
    func request(_ value: JSON, completion: @escaping @Sendable (JSON) -> Void) {
        queue.async { [self] in completion(call(value)) }
    }
    private func call(_ value: JSON) -> JSON {
        do {
            let text = try value.encoded()
            guard let reply = text.withCString({ capy_workspace_library_request(handle, $0) }) else {
                throw HostFailure(message: "Workspace storage did not reply")
            }
            defer { capy_apple_string_free(reply) }
            return try JSON.decode(String(cString: reply))
        } catch {
            return JSON(["error": ["kind": "unavailable", "message": error.localizedDescription]])
        }
    }
}
