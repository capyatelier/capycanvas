import Foundation
import CryptoKit

/// Call on NativeProjectTask.io: every window shares ordered, atomic preferences.
final class ColorPreferencesStore: @unchecked Sendable {
    private let root: URL?
    private let profileLimit = 16 * 1024 * 1024
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
    private func digest(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
    private func validID(_ id: String) -> Bool { id.count == 64 && id.allSatisfy { "0123456789abcdef".contains($0) } }
    private var directory: URL? { root?.appendingPathComponent("color-profiles", isDirectory: true) }
    private func files() throws -> [URL] {
        guard let directory else { return [] }
        do {
            return try FileManager.default.contentsOfDirectory(at: directory,
                includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey]).filter {
                    let values = try $0.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
                    return values.isRegularFile == true && values.isSymbolicLink != true
                        && $0.pathExtension == "icc" && validID($0.deletingPathExtension().lastPathComponent)
                }.sorted { $0.lastPathComponent < $1.lastPathComponent }
        } catch let error as NSError where Self.missing(error) { return [] }
    }
    private func size(_ url: URL) throws -> Int { try url.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0 }
    private func read(_ url: URL, verify: Bool) throws -> Data {
        let file = try FileHandle(forReadingFrom: url)
        defer { try? file.close() }
        var data = Data()
        while data.count <= profileLimit {
            guard let part = try file.read(upToCount: min(64 * 1024, profileLimit + 1 - data.count)), !part.isEmpty else { break }
            data.append(part)
        }
        guard data.count <= profileLimit else { throw HostFailure(message: "ICC profile exceeds 16 MiB") }
        if verify && digest(data) != url.deletingPathExtension().lastPathComponent {
            throw HostFailure(message: "The saved profile changed. Remove or reimport it.")
        }
        return data
    }
    private func inspect(_ data: Data, summary: Bool) throws -> JSON {
        try data.withUnsafeBytes { bytes in
            try result(capy_color_profile_inspect(bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count, summary))
        }
    }
    func importProfile(_ url: URL) throws -> JSON {
        let data = try ProjectFileIO.coordinate(url, writing: false) { try self.read($0, verify: false) }
        let profile = try inspect(data, summary: false)
        if let directory {
            let id = digest(data)
            let others = try files().filter { $0.deletingPathExtension().lastPathComponent != id }
            let total = try others.reduce(data.count) { try $0 + size($1) }
            guard others.count < 128, total <= 64 * 1024 * 1024 else {
                throw HostFailure(message: "The profile library limit is 128 profiles and 64 MiB")
            }
            try prepare(directory)
            let target = directory.appendingPathComponent(id + ".icc")
            try ProjectFileIO.atomicWrite(to: target) { descriptor in
                try FileHandle(fileDescriptor: descriptor, closeOnDealloc: false).write(contentsOf: data)
            }
        }
        return profile
    }
    func profiles() throws -> [JSON] {
        var total = 0
        return try files().prefix(128).map { url in
            let count = try size(url), id = url.deletingPathExtension().lastPathComponent
            total += count
            let entry: JSON
            do {
                guard total <= 64 * 1024 * 1024 else { throw HostFailure(message: "Library exceeds 64 MiB. Remove unused profiles.") }
                entry = try inspect(read(url, verify: true), summary: true)
            } catch { entry = JSON(["name": "Unavailable profile", "issue": error.localizedDescription]) }
            return entry.replacing("id", with: JSON(id)).replacing("bytes", with: JSON(count))
        }.sorted { $0["name"].string.localizedStandardCompare($1["name"].string) == .orderedAscending }
    }
    func profile(_ id: String) throws -> JSON {
        guard validID(id), let directory else { throw HostFailure(message: "Select an imported profile") }
        return try inspect(read(directory.appendingPathComponent(id + ".icc"), verify: true), summary: false)
    }
    func removeProfile(_ id: String) throws {
        guard validID(id), let directory else { throw HostFailure(message: "Select an imported profile") }
        try FileManager.default.removeItem(at: directory.appendingPathComponent(id + ".icc"))
    }
}
