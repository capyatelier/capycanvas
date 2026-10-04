import Foundation
import Darwin
import CoreGraphics
import UniformTypeIdentifiers

extension UTType {
    static let capyProject = UTType(exportedAs: "art.capycanvas.project", conformingTo: .data)
    static let capyPhotoTypes: [UTType] = {
        guard let text = capy_photo_formats() else { return [] }
        defer { capy_apple_string_free(text) }
        guard let formats = try? JSON.decode(String(cString: text)) else { return [] }
        var types: [UTType] = []
        for format in formats.array {
            for ext in format["extensions"].array {
                if let type = UTType(filenameExtension: ext.string), !types.contains(type) { types.append(type) }
            }
        }
        return types
    }()
}

/// A job owns immutable Rust data and GPU preparation, never a NativeOwner.
/// Its final release can destroy a large retired document, so use the I/O queue.
final class NativeProjectTask: @unchecked Sendable {
    enum Kind: UInt32 { case save = 0, open = 1, place = 3, color, properties, source, histogram, clip }
    static let io = DispatchQueue(label: "art.capycanvas.project-files", qos: .userInitiated)
    let handle: OpaquePointer
    init(_ handle: OpaquePointer) { self.handle = handle }
    deinit { let handle = handle; Self.io.async { capy_project_free(handle) } }
    func cancel() { capy_project_cancel(handle) }
    private func check(_ result: Int32) throws {
        guard result < 0 else { return }
        let error = capy_project_error(handle)
        defer { if let error { capy_apple_string_free(error) } }
        throw HostFailure(message: error.map { String(cString: $0) } ?? "Document operation failed")
    }
    func write(to url: URL, previewOnly: Bool = false, original: URL? = nil, destinationError: String = "") throws {
        try ProjectFileIO.coordinate(url, writing: true) { destination in
            if previewOnly, let original {
                let access = original.startAccessingSecurityScopedResource()
                defer { if access { original.stopAccessingSecurityScopedResource() } }
                if ProjectFileIO.sameFile(original, destination) { throw HostFailure(message: destinationError) }
            }
            try ProjectFileIO.atomicWrite(to: destination, beforeCommit: { [self] in
                let existing = try? FileHandle(forReadingFrom: destination)
                defer { try? existing?.close() }
                try url.absoluteString.withCString { try self.check(capy_project_destination_matches(self.handle, existing?.fileDescriptor ?? -1, $0)) }
                guard capy_project_begin_commit(self.handle) == 0 else { throw HostFailure(message: "Document operation cancelled") }
            }) { [self] descriptor in try self.check(previewOnly ? capy_project_export_preview(self.handle, descriptor) : capy_project_write(self.handle, descriptor)) }
        }
    }
    func read(from url: URL?, options: JSON? = nil) throws {
        guard let url else {
            if let options { try check(try options.encoded().withCString { capy_project_new(handle, $0) }) }
            else { try check(capy_project_read(handle, -1, "Untitled")) }
            return
        }
        try ProjectFileIO.coordinate(url, writing: false) { source in
            let file = try FileHandle(forReadingFrom: source)
            defer { try? file.close() }
            try url.lastPathComponent.withCString { try self.check(capy_project_read(self.handle, file.fileDescriptor, $0)) }
        }
    }
    func read(image: Data, name: String = "Pasted image") throws {
        try name.withCString { title in
            try image.withUnsafeBytes { try check(capy_project_read_bytes(handle,
                $0.bindMemory(to: UInt8.self).baseAddress, $0.count, title)) }
        }
    }
    func prepareEdit(_ choice: JSON?, copy: Bool) throws {
        try check(try (choice ?? JSON()).encoded().withCString { capy_project_edit_work(handle, $0, copy) })
    }
    func compare() throws { try check(capy_project_compare(handle)) }
    var clipProgress: String? {
        guard let text = capy_project_clip_progress(handle) else { return nil }
        defer { capy_apple_string_free(text) }
        return String(cString: text)
    }
    func copyPixels(nonce: String) throws -> Data {
        try nonce.withCString { try check(capy_project_clip_run(handle, $0)) }
        var bytes: UnsafePointer<UInt8>?, count = 0
        try check(capy_project_clip_png(handle, &bytes, &count))
        guard let bytes else { throw HostFailure(message: "The copy has no pixels") }
        return Data(bytes: bytes, count: count)
    }
    func configureExport(_ recipe: JSON) throws {
        try check(try recipe.encoded().withCString { capy_project_export_options(handle, $0) })
    }
    func buildProof() throws { try check(capy_project_proof_build(handle)) }
    func proofPreservation() throws -> Data? {
        var bytes: UnsafePointer<UInt8>?, count = 0
        try check(capy_project_proof_preservation(handle, &bytes, &count))
        return bytes.map { Data(bytes: $0, count: count) }
    }
    func details() throws -> JSON {
        guard let text = capy_project_details(handle) else { try check(-1); return JSON() }
        defer { capy_apple_string_free(text) }
        return try JSON.decode(String(cString: text))
    }
    func comparison(after: Bool) throws -> CGImage {
        try comparison(index: after ? 1 : 0)
    }
    func comparison(index: UInt32) throws -> CGImage {
        var preview = CapyProjectPreview()
        guard capy_project_preview_at(handle, index, &preview) == 0, let pixels = preview.pixels,
            preview.width > 0, preview.height > 0, preview.width <= 512, preview.height <= 384,
            preview.count == Int(preview.width * preview.height * 4),
            let space = CGColorSpace(name: CGColorSpace.displayP3),
            let provider = CGDataProvider(data: Data(bytes: pixels, count: preview.count) as CFData),
            let image = CGImage(width: Int(preview.width), height: Int(preview.height), bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: Int(preview.width) * 4, space: space,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                provider: provider, decode: nil, shouldInterpolate: true, intent: .relativeColorimetric) else {
            throw HostFailure(message: "The color comparison is unavailable")
        }
        return image
    }

    func packageSummary() throws -> JSON {
        guard let text = capy_project_package_summary(handle) else { throw HostFailure(message: "Document operation is unavailable") }
        defer { capy_apple_string_free(text) }
        return try JSON.decode(String(cString: text))
    }
    func packagePreview() throws -> CGImage? {
        var preview = CapyProjectPreview()
        try check(capy_project_package_preview(handle, &preview))
        guard let pixels = preview.pixels else { return nil }
        guard preview.width > 0, preview.height > 0, preview.width <= 1024, preview.height <= 1024,
            preview.count == Int(preview.width * preview.height * 4),
            let space = CGColorSpace(name: CGColorSpace.sRGB),
            let provider = CGDataProvider(data: Data(bytes: pixels, count: preview.count) as CFData),
            let image = CGImage(width: Int(preview.width), height: Int(preview.height), bitsPerComponent: 8, bitsPerPixel: 32,
                bytesPerRow: Int(preview.width) * 4, space: space,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                provider: provider, decode: nil, shouldInterpolate: true, intent: .relativeColorimetric) else {
            throw HostFailure(message: "The drawing preview is unavailable")
        }
        return image
    }

    func pendingProfile() throws -> JSON {
        guard let value = capy_project_profile(handle) else { throw HostFailure(message: "Document operation is unavailable") }
        defer { capy_apple_string_free(value) }
        return try JSON.decode(String(cString: value))
    }
    func assumeProfile(_ profile: JSON) throws {
        try check(try profile.encoded().withCString { capy_project_assume_profile(handle, $0) })
    }
}

enum ProjectFileIO {
    static func sameFile(_ original: URL, _ destination: URL) -> Bool {
        if original.standardizedFileURL.resolvingSymlinksInPath() == destination.standardizedFileURL.resolvingSymlinksInPath() { return true }
        guard let source = try? FileManager.default.attributesOfItem(atPath: original.path),
            let target = try? FileManager.default.attributesOfItem(atPath: destination.path),
            let sourceDevice = source[.systemNumber] as? NSNumber, let targetDevice = target[.systemNumber] as? NSNumber,
            let sourceFile = source[.systemFileNumber] as? NSNumber, let targetFile = target[.systemFileNumber] as? NSNumber else { return false }
        return sourceDevice == targetDevice && sourceFile == targetFile
    }
    /// File providers can substitute the coordinated URL. Scope and coordination
    /// enclose the entire read/write, including Rust validation/compression.
    static func coordinate<Value>(_ url: URL, writing: Bool, work: @escaping (URL) throws -> Value) throws -> Value {
        let access = url.startAccessingSecurityScopedResource()
        defer { if access { url.stopAccessingSecurityScopedResource() } }
        let coordinator = NSFileCoordinator()
        var coordinationError: NSError?
        var result: Result<Value, Error>?
        let operation: (URL) -> Void = { location in
            result = Result { try work(location) }
        }
        if writing {
            coordinator.coordinate(writingItemAt: url, options: .forReplacing,
                error: &coordinationError, byAccessor: operation)
        } else {
            coordinator.coordinate(readingItemAt: url, options: [],
                error: &coordinationError, byAccessor: operation)
        }
        guard let result else { throw coordinationError ?? HostFailure(message: "Could not access the selected file") }
        let value = try result.get()
        if let coordinationError { throw coordinationError }
        return value
    }
    /// Stream into a system-provided replacement directory on the same volume.
    /// A picked file grants access to that file, not arbitrary siblings or its
    /// parent directory. Foundation publishes the replacement within that grant.
    /// The destination is unchanged until validation/write/fsync all succeed.
    static func atomicWrite(to url: URL, beforeCommit: () throws -> Void = {}, write: (Int32) throws -> Void) throws {
        let manager = FileManager.default
        let directory = try manager.url(for: .itemReplacementDirectory, in: .userDomainMask,
            appropriateFor: url, create: true)
        defer { try? manager.removeItem(at: directory) }
        let temporary = directory.appendingPathComponent(".\(UUID().uuidString).capy-tmp")
        let descriptor = temporary.path.withCString { open($0, O_WRONLY | O_CREAT | O_EXCL, 0o600) }
        guard descriptor >= 0 else { throw posixFailure() }
        var opened = true
        defer { if opened { close(descriptor) }; try? FileManager.default.removeItem(at: temporary) }
        try write(descriptor)
        guard synchronize(descriptor) else { throw posixFailure() }
        let closed = close(descriptor); opened = false
        guard closed == 0 else { throw posixFailure() }
        try beforeCommit()
        if manager.fileExists(atPath: url.path) {
            _ = try manager.replaceItemAt(url, withItemAt: temporary)
        } else {
            try manager.moveItem(at: temporary, to: url)
        }
    }
    static func stagingURL(title: String, extension suffix: String = "capy") throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        let name = URL(fileURLWithPath: title).deletingPathExtension().lastPathComponent
        return folder.appendingPathComponent(name.isEmpty ? "Untitled" : name).appendingPathExtension(suffix)
    }
    /// Flushes through the drive's cache; a volume without that command gets an ordinary flush.
    static func synchronize(_ descriptor: Int32) -> Bool { fcntl(descriptor, F_FULLFSYNC) != -1 || fsync(descriptor) == 0 }
    private static func posixFailure() -> NSError { NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
}
