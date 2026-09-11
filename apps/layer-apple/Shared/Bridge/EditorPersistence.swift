import Foundation

/// One I/O queue orders settings writes across windows. Workspace files belong
/// to individual scenes; the last committed workspace seeds a newly opened one.
final class EditorPersistence: @unchecked Sendable {
    struct Loaded: Sendable {
        var settings: Data?
        var workspace: Data?
        var workspaceNeedsSnapshot = false
        var errors: [String: String] = [:]
    }
    struct SettingsChange: Sendable { let revision: UInt64; let data: Data }
    static let shared = EditorPersistence(root: configuredRoot())
    let root: URL?
    private let queue = DispatchQueue(label: "art.capycanvas.storage", qos: .utility)
    private var revision: UInt64 = 0
    private var observers: [UUID: @Sendable (SettingsChange) -> Void] = [:]

    init(root: URL?) { self.root = root }
    private static func configuredRoot() -> URL? {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent(Bundle.main.bundleIdentifier ?? "art.capycanvas.apple", isDirectory: true)
        #if DEBUG
        let environment = ProcessInfo.processInfo.environment
        if environment["CAPY_DISABLE_PERSISTENCE"] == "1" { return nil }
        if let namespace = environment["CAPY_PERSISTENCE_NAMESPACE"], let id = UUID(uuidString: namespace) {
            return base.appendingPathComponent("test-\(id.uuidString)", isDirectory: true)
        }
        // Existing deterministic fixtures must not read or alter user preferences.
        if environment["CAPY_INITIAL_ACTIONS"] != nil { return nil }
        #endif
        return base
    }
    func load(scene: String, observer: UUID, changed: @escaping @Sendable (SettingsChange) -> Void,
        completion: @escaping @Sendable (Loaded) -> Void) {
        queue.async { [self] in
            observers[observer] = changed
            var result = Loaded()
            if let root {
                do { result.settings = try AtomicJSONFile.read(root.appendingPathComponent("settings.json")) }
                catch { result.errors["settings"] = "Could not restore settings: \(error.localizedDescription)" }
                do {
                    let sceneURL = workspaceURL(scene)
                    result.workspace = try AtomicJSONFile.read(sceneURL)
                    if result.workspace == nil {
                        result.workspace = try AtomicJSONFile.read(root.appendingPathComponent("workspace.json"))
                        result.workspaceNeedsSnapshot = true
                    }
                } catch { result.errors["workspace"] = "Could not restore workspace: \(error.localizedDescription)" }
            }
            completion(result)
        }
    }
    func unsubscribe(_ observer: UUID) { queue.async { [self] in observers.removeValue(forKey: observer) } }
    func saveSettings(_ data: Data, completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do {
                if let root { try AtomicJSONFile.write(data, to: root.appendingPathComponent("settings.json")) }
                revision &+= 1
                let change = SettingsChange(revision: revision, data: data)
                for observer in observers.values { observer(change) }
                completion(nil)
            } catch { completion("Could not save settings: \(error.localizedDescription)") }
        }
    }
    func saveWorkspace(_ data: Data, scene: String, updateDefault: Bool = true, completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do {
                if let root {
                    try AtomicJSONFile.write(data, to: workspaceURL(scene))
                    if updateDefault { try AtomicJSONFile.write(data, to: root.appendingPathComponent("workspace.json")) }
                }
                completion(nil)
            } catch { completion("Could not save workspace: \(error.localizedDescription)") }
        }
    }
    private func workspaceURL(_ scene: String) -> URL {
        // Scene identifiers are generated locally; reject path syntax even in
        // debug fixtures. They will also identify document recovery sessions.
        let name = UUID(uuidString: scene)?.uuidString ?? "default"
        return root!.appendingPathComponent("workspaces", isDirectory: true).appendingPathComponent(name + ".json")
    }
    func flush(_ completion: @escaping @Sendable () -> Void) { queue.async(execute: completion) }
}
