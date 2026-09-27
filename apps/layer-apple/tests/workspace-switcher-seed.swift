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
        let workspaces = editor.workspaces!
        try await workspaces.started("Fixture startup")
        for index in 0..<24 { try await workspaces.create(String(format: "Drawing Task %02d", index)) }
        try await workspaces.perform(["type": "switch", "id": "builtin:workspace:illustrator"])
        let rows = try await workspaces.workspaceRows()
        precondition(rows.count == 27)
        try JSON(rows.map(\.raw)).encoded().write(to: root.appendingPathComponent("fixture-rows.json"), atomically: true, encoding: .utf8)
        try await workspaces.closed()
        print("PASS: prepared 27 workspaces through real creation operations; all owners released")
    }
}
