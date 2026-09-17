import Foundation

/// Call on NativeProjectTask.io: every window shares ordered, atomic preferences.
final class ColorPreferencesStore: @unchecked Sendable {
    private let root: URL?
    init(root: URL?) { self.root = root }
    var canSave: Bool { root != nil }

    private func result(_ pointer: UnsafeMutablePointer<CChar>?) throws -> JSON {
        guard let pointer else { throw HostFailure(message: "Color preferences are unavailable") }
        defer { capy_apple_string_free(pointer) }
        let value = try JSON.decode(String(cString: pointer))
        if !value["error"].isNull { throw HostFailure(message: value["error"].string) }
        return value
    }
    func presets(color: JSON, request: JSON) throws -> JSON {
        let url = root?.appendingPathComponent("color-export-presets.json")
        let file: FileHandle?
        do { file = try url.map { try FileHandle(forReadingFrom: $0) } }
        catch let error as NSError where Self.missing(error) { file = nil }
        defer { try? file?.close() }
        func operate(_ output: Int32) throws -> JSON {
            try request.encoded().withCString { action in
                try color.encoded().withCString { color in
                    try result(capy_export_presets(file?.fileDescriptor ?? -1, output, action, color))
                }
            }
        }
        if ["list", "get"].contains(request["type"].string) { return try operate(-1) }
        guard let url else { throw HostFailure(message: "Saving color preferences is disabled") }
        try prepare(url.deletingLastPathComponent())
        var view = JSON()
        try ProjectFileIO.atomicWrite(to: url) { view = try operate($0) }
        return view
    }
    private func prepare(_ directory: URL) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
    }
    private static func missing(_ error: NSError) -> Bool {
        error.domain == NSCocoaErrorDomain && [NSFileNoSuchFileError, NSFileReadNoSuchFileError].contains(error.code)
    }
    func exportDraft(recipe: JSON, action: JSON = JSON(["type": "refresh"])) throws -> JSON {
        try recipe.encoded().withCString { recipe in
            try action.encoded().withCString { try result(capy_export_draft(recipe, $0)) }
        }
    }
    private func library(_ action: JSON, bytes: Data = Data()) throws -> JSON {
        try action.encoded().withCString { action in
            try bytes.withUnsafeBytes { buffer in
                try result(capy_profile_library(action, buffer.bindMemory(to: UInt8.self).baseAddress, bytes.count))
            }
        }
    }
    private var directory: URL? { root?.appendingPathComponent("color-profiles", isDirectory: true) }
    private func inventory() throws -> [JSON] {
        var entries: [[String: Any]] = []
        if let directory {
            do {
                for url in try FileManager.default.contentsOfDirectory(at: directory,
                    includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey]) {
                    let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
                    if values.isRegularFile == true && values.isSymbolicLink != true && url.pathExtension == "icc" {
                        entries.append(["id": url.deletingPathExtension().lastPathComponent, "bytes": values.fileSize ?? 0])
                    }
                }
            } catch let error as NSError where Self.missing(error) { }
        }
        return try library(JSON(["type": "inventory", "entries": entries])).array
    }
    private func location(_ id: String) throws -> URL {
        // Validate the app-owned key before constructing a path or reading bytes.
        let key = try library(JSON(["type": "remove", "id": id]))["id"].string
        guard let directory else { throw HostFailure(message: "Select an imported profile") }
        return directory.appendingPathComponent(key + ".icc")
    }
    private func read(_ url: URL) throws -> Data {
        let limit = Int(try library(JSON(["type": "limits"]))["read_bytes"].uint)
        let file = try FileHandle(forReadingFrom: url)
        defer { try? file.close() }
        var data = Data()
        while data.count <= limit {
            guard let part = try file.read(upToCount: min(64 * 1024, limit + 1 - data.count)), !part.isEmpty else { break }
            data.append(part)
        }
        return data
    }
    func importProfile(_ url: URL) throws -> JSON {
        let data = try ProjectFileIO.coordinate(url, writing: false) { try self.read($0) }
        let entry = try library(JSON(["type": "import", "entries": inventory().map(\.raw)]), bytes: data)
        if let directory {
            try prepare(directory)
            try ProjectFileIO.atomicWrite(to: location(entry["id"].string)) { descriptor in
                try FileHandle(fileDescriptor: descriptor, closeOnDealloc: false).write(contentsOf: data)
            }
        }
        return exportProfile(entry)
    }
    func profiles() throws -> [JSON] {
        try inventory().map { entry in
            var data = Data(), failure: String?
            if entry["issue"].isNull {
                do { data = try read(location(entry["id"].string)) }
                catch { failure = error.localizedDescription }
            }
            return try library(JSON(["type": "inspect", "entry": entry.raw,
                "error": failure.map { $0 as Any } ?? NSNull()]), bytes: data)
        }.sorted { $0["name"].string.localizedStandardCompare($1["name"].string) == .orderedAscending }
    }
    func profile(_ id: String) throws -> JSON {
        try exportProfile(library(JSON(["type": "get", "id": id]), bytes: read(location(id))))
    }
    private func exportProfile(_ entry: JSON) -> JSON {
        JSON(["name": entry["name"].raw, "channels": entry["channels"].raw, "profile": entry["profile"].raw])
    }
    func removeProfile(_ id: String) throws {
        try FileManager.default.removeItem(at: location(id))
    }
}
