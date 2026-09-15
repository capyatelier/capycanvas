import Foundation

enum WorkspacePackageKind: String, CaseIterable {
    case workspaceBackup = "workspace_backup", toolbar
    var fileExtension: String {
        switch self { case .workspaceBackup: "capyworkspace"; case .toolbar: "capytoolbar" }
    }
    static func forURL(_ url: URL) -> Self? { allCases.first { $0.fileExtension == url.pathExtension.lowercased() } }
}

/// Read externally opened packages off the editor queue. Rust owns validation
/// and import; the workspace manager has no separate package picker or exporter.
enum WorkspacePackageFiles {
    private static let io = DispatchQueue(label: "art.capycanvas.workspace-packages", qos: .userInitiated)
    static func read(from url: URL) async throws -> String {
        try await withCheckedThrowingContinuation { continuation in
            io.async {
                do {
                    let text = try ProjectFileIO.coordinate(url, writing: false) { source in
                        let file = try FileHandle(forReadingFrom: source)
                        defer { try? file.close() }
                        let maximum = 128 * 1024 * 1024
                        let data = try file.read(upToCount: maximum + 1) ?? Data()
                        guard data.count <= maximum else { throw HostFailure(message: "The workspace package is too large") }
                        guard let decoded = String(data: data, encoding: .utf8) else {
                            throw HostFailure(message: "The workspace package is not valid UTF-8")
                        }
                        return decoded
                    }
                    continuation.resume(returning: text)
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
}
