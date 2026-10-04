import Foundation

/// Where each store lives. Rust names the stores within the folders it is given.
struct StorageLocations: Sendable {
    let settings: URL
    let exportPresets: URL
    let colorProfiles: URL
    let workspaces: URL
    let sessions: URL
    let shaders: URL
    let state: URL

    /// Application Support and Caches for this bundle, unless `CAPY_STORAGE_DIR`
    /// names a private folder. Resolving also sets the process temporary folder.
    static let installation: StorageLocations? = {
        var request: [String: Any] = [:]
        if let identifier = Bundle.main.bundleIdentifier {
            let manager = FileManager.default
            let support = manager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent(identifier, isDirectory: true)
            let caches = manager.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                .appendingPathComponent(identifier, isDirectory: true)
            request["platform"] = ["config": support.path, "data": support.path,
                "state": support.appendingPathComponent("State", isDirectory: true).path,
                "cache": caches.path, "temp": manager.temporaryDirectory.path]
        }
        do { return try StorageLocations.resolve(request) }
        catch {
            NSLog("Capy Canvas storage is unavailable: %@", error.localizedDescription)
            return nil
        }
    }()

    /// Every store inside one private folder.
    static func within(_ directory: URL) -> StorageLocations? {
        try? resolve(["directory": directory.path])
    }

    private static func resolve(_ request: [String: Any]) throws -> StorageLocations {
        let text = try JSON(request).encoded()
        guard let pointer = text.withCString({ capy_apple_storage($0) }) else {
            throw HostFailure(message: "App storage is unavailable")
        }
        defer { capy_apple_string_free(pointer) }
        let value = try JSON.decode(String(cString: pointer))
        if !value["error"].isNull { throw HostFailure(message: value["error"].string) }
        func location(_ key: String) -> URL { URL(fileURLWithPath: value[key].string) }
        return StorageLocations(settings: location("settings"), exportPresets: location("export_presets"),
            colorProfiles: location("color_profiles"), workspaces: location("workspaces"),
            sessions: location("sessions"), shaders: location("shaders"), state: location("state"))
    }
}
