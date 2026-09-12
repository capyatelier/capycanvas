// Prepare isolated long-list data through the actual Apple coordinator and
// shared Rust creation operations. No UI automation or user storage is involved.
import Foundation

@main struct WorkspaceSwitcherSeed {
    @MainActor static func main() async throws {
        guard let path = ProcessInfo.processInfo.environment["CAPY_SWITCHER_SEED_DIRECTORY"] else {
            throw HostFailure(message: "Set CAPY_SWITCHER_SEED_DIRECTORY to a new fixture directory")
        }
        let root = URL(fileURLWithPath: path, isDirectory: true)
        if FileManager.default.fileExists(atPath: root.path),
            !(try FileManager.default.contentsOfDirectory(atPath: root.path)).isEmpty {
            throw HostFailure(message: "The fixture directory must be empty")
        }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let editor = EditorStore(platform: 0, persistence: EditorPersistence(root: root))
        let library = editor.workspaceLibrary!
        let deadline = Date().addingTimeInterval(20)
        while !library.ready {
            if let error = library.error { throw HostFailure(message: error) }
            guard Date() < deadline else { throw HostFailure(message: "Fixture startup timed out") }
            try await Task.sleep(for: .milliseconds(5))
        }
        for index in 0..<24 {
            _ = try await library.operation(["type": "new", "name": String(format: "Drawing Task %02d", index)])
        }
        _ = try await library.operation(["type": "switch", "id": "builtin:workspace:illustrator"])
        let rows = try await library.read(["type": "view", "page": "workspaces", "query": "", "idle": true])["rows"]
        precondition(rows.array.count == 27)
        try rows.encoded().write(to: root.appendingPathComponent("fixture-rows.json"), atomically: true, encoding: .utf8)
        try await library.close()
        print("PASS: prepared 27 workspaces through real creation operations; all owners released")
    }
}
