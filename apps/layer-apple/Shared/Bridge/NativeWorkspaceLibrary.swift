import Foundation

/// The SQLite manager never runs on MainActor or the drawing owner. Each window
/// has a serial manager owner; Rust shares one storage worker for its directory.
final class NativeWorkspaceLibrary: @unchecked Sendable {
    private let queue = DispatchQueue(label: "art.capycanvas.workspace-library", qos: .utility)
    private let handle: OpaquePointer
    private let root: URL
    init(platform: UInt32, root: URL, scene: String) throws {
        let key = "apple:scene:" + (UUID(uuidString: scene)?.uuidString ?? "default")
        let result = queue.sync {
            root.path.withCString { directory in
                key.withCString { capy_workspace_library_create(platform, directory, $0) }
            }
        }
        guard let result else { throw HostFailure(message: "Could not start workspace storage") }
        handle = result; self.root = root
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
    /// Read only unacknowledged legacy sources on this storage queue. All inputs
    /// must parse before the single Rust migration transaction can publish them.
    /// No legacy file is removed, rewritten, or read again after acknowledgement.
    func migrateLegacy(completion: @escaping @Sendable (JSON) -> Void) {
        queue.async { [self] in
            do {
                let files: [URL]
                do {
                    files = try FileManager.default.contentsOfDirectory(
                        at: root.appendingPathComponent("workspaces", isDirectory: true),
                        includingPropertiesForKeys: nil).filter {
                            $0.pathExtension == "json" && (UUID(uuidString: $0.deletingPathExtension().lastPathComponent) != nil
                                || $0.lastPathComponent == "default.json")
                        }.sorted { $0.lastPathComponent < $1.lastPathComponent }
                } catch let error as NSError where error.domain == NSCocoaErrorDomain
                    && [NSFileNoSuchFileError, NSFileReadNoSuchFileError].contains(error.code) { files = [] }
                var scenes: [[Any]] = [], fallback: Any = NSNull(), mappings: [String: Any] = [:]
                var bytes = 0
                let sources = files.map { ("workspaces/" + $0.lastPathComponent, $0) }
                    + [("workspace.json", root.appendingPathComponent("workspace.json"))]
                for (source, url) in sources {
                    let existing = call(JSON(["type": "legacy_mapping", "source": source]))
                    if !existing["error"].isNull { completion(existing); return }
                    let id = existing["value"]["mapping"]["value"]
                    if !id.isNull { mappings[source] = id.raw; continue }
                    guard let data = try AtomicJSONFile.read(url) else { continue }
                    bytes += data.count
                    guard bytes <= 128 * 1024 * 1024 else {
                        throw HostFailure(message: "Saved workspaces exceed the supported import size")
                    }
                    let value = try JSONSerialization.jsonObject(with: data)
                    if source == "workspace.json" { fallback = [source, value] }
                    else { scenes.append([source, value]) }
                }
                let reply = call(JSON(["type": "migrate", "scenes": scenes, "fallback": fallback,
                    "now": UInt64(Date().timeIntervalSince1970 * 1000)]))
                if !reply["error"].isNull { completion(reply); return }
                mappings.merge(reply["value"]["mappings"].object) { _, new in new }
                completion(reply.replacing("value", with: JSON(["mappings": mappings])))
            } catch {
                completion(JSON(["error": ["kind": "invalid", "message": "Could not import saved workspaces: \(error.localizedDescription)"]]))
            }
        }
    }
}
