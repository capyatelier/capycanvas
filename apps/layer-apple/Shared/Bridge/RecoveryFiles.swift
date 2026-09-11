import Foundation

/// Private generations are published by one small atomic manifest. A kill
/// before publication leaves the previous archive reachable. No provider URLs,
/// bookmarks or machine identifiers enter a recovery record or project.
struct RecoveryRecord: Codable, Identifiable, Equatable, Sendable {
    let version: Int
    let scene: UUID
    let generation: UUID
    let title: String
    let modified: Date
    var id: UUID { generation }
}

struct RecoveryFiles: Sendable {
    private struct Index: Codable { var version = 1; let record: RecoveryRecord? }
    let root: URL?
    private var directory: URL? { root?.appendingPathComponent("recovery", isDirectory: true) }
    private func folder(_ scene: UUID) -> URL? { directory?.appendingPathComponent(scene.uuidString, isDirectory: true) }
    func archive(_ record: RecoveryRecord) -> URL? {
        folder(record.scene)?.appendingPathComponent(record.generation.uuidString + ".capy")
    }
    func current(_ scene: UUID) throws -> RecoveryRecord? {
        guard let folder = folder(scene), let data = try AtomicJSONFile.read(folder.appendingPathComponent("current.json")) else { return nil }
        let index = try JSONDecoder().decode(Index.self, from: data)
        guard index.version == 1 else { throw HostFailure(message: "Unsupported recovery record") }
        guard let record = index.record else { return nil }
        guard record.version == 1, record.scene == scene, record.title.utf8.count <= 1024,
            record.modified.timeIntervalSince1970.isFinite else { throw HostFailure(message: "Invalid recovery record") }
        return record
    }
    func list() throws -> (records: [RecoveryRecord], errors: [String]) {
        guard let directory, FileManager.default.fileExists(atPath: directory.path) else { return ([], []) }
        var records: [RecoveryRecord] = [], errors: [String] = []
        for url in try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) {
            guard let scene = UUID(uuidString: url.lastPathComponent) else { continue }
            do { if let record = try current(scene) { records.append(record) } }
            catch { errors.append("A recovery record could not be read; its files have been preserved. \(error.localizedDescription)") }
        }
        return (records.sorted { $0.modified > $1.modified }, errors)
    }
    @discardableResult func write(_ task: NativeProjectTask, scene: UUID, title: String) throws -> RecoveryRecord? {
        guard let folder = folder(scene) else { return nil }
        let fm = FileManager.default
        // Validate the existing manifest before replacing it. Corrupt recovery
        // data remains available instead of being silently replaced by defaults.
        _ = try current(scene)
        try fm.createDirectory(at: folder, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let record = RecoveryRecord(version: 1, scene: scene, generation: UUID(),
            title: String(title.prefix(256)), modified: Date())
        let destination = archive(record)!
        try task.write(to: destination)
        try AtomicJSONFile.write(JSONEncoder().encode(Index(record: record)), to: folder.appendingPathComponent("current.json"))
        // Only obsolete private generations are removed, after publication.
        for url in try fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)
            where url.pathExtension == "capy" && url.lastPathComponent != destination.lastPathComponent
                && UUID(uuidString: url.deletingPathExtension().lastPathComponent) != nil {
            try? fm.removeItem(at: url)
        }
        return record
    }
    /// A late UI completion must not delete a newer generation from another
    /// owner. Remove the manifest first, then its now-unreferenced archive.
    func remove(_ record: RecoveryRecord) throws {
        guard let folder = folder(record.scene), try current(record.scene) == record else { return }
        try AtomicJSONFile.write(JSONEncoder().encode(Index(record: nil)), to: folder.appendingPathComponent("current.json"))
        if let archive = archive(record) { try? FileManager.default.removeItem(at: archive) }
    }
}
