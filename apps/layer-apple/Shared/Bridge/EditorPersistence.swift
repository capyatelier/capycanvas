import Foundation

/// One I/O queue orders settings writes and notifications across windows.
final class EditorPersistence: @unchecked Sendable {
    struct Loaded: Sendable {
        var settings: Data?
    }
    struct SettingsChange: Sendable { let revision: UInt64; let data: Data }
    static let shared = EditorPersistence(locations: StorageLocations.installation)
    let locations: StorageLocations?
    private let queue = DispatchQueue(label: "art.capycanvas.storage", qos: .utility)
    private var revision: UInt64 = 0
    private var acceptedSettings: Data?
    private var observers: [UUID: @Sendable (SettingsChange) -> Void] = [:]

    init(locations: StorageLocations?) {
        self.locations = locations
        if let state = locations?.state { queue.async { Self.excludeFromBackup(state) } }
    }
    /// Every store inside one private folder, or memory only without a folder.
    convenience init(root: URL?) { self.init(locations: root.flatMap(StorageLocations.within)) }
    private static func excludeFromBackup(_ directory: URL) {
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o700])
            var excluded = URLResourceValues()
            excluded.isExcludedFromBackup = true
            var url = directory
            try url.setResourceValues(excluded)
        } catch { NSLog("Capy Canvas could not exclude drawing sessions from backups: %@", error.localizedDescription) }
    }
    func load(observer: UUID, changed: @escaping @Sendable (SettingsChange) -> Void,
        completion: @escaping @Sendable (Loaded) -> Void) {
        queue.async { [self] in
            observers[observer] = changed
            var result = Loaded()
            if let acceptedSettings { result.settings = acceptedSettings }
            else if let locations { result.settings = try? AtomicJSONFile.read(locations.settings) }
            completion(result)
        }
    }
    func unsubscribe(_ observer: UUID) { queue.async { [self] in observers.removeValue(forKey: observer) } }
    func saveSettings(_ data: Data, completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            acceptedSettings = data
            revision &+= 1
            let change = SettingsChange(revision: revision, data: data)
            for observer in observers.values { observer(change) }
            do {
                if let locations { try AtomicJSONFile.write(data, to: locations.settings) }
                completion(nil)
            } catch { completion("Could not save settings: \(error.localizedDescription)") }
        }
    }
    func flush(_ completion: @escaping @Sendable () -> Void) { queue.async(execute: completion) }
}
