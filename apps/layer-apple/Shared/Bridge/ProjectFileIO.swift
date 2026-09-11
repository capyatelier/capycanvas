import Foundation
import Darwin
import UniformTypeIdentifiers

extension UTType {
    static let capyProject = UTType(exportedAs: "art.capycanvas.project", conformingTo: .data)
}

/// A job owns immutable Rust data and GPU preparation, never a NativeOwner.
/// Its final release can destroy a large retired document, so use the I/O queue.
final class NativeProjectTask: @unchecked Sendable {
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
    func write(to url: URL) throws {
        try ProjectFileIO.coordinate(url, writing: true) { destination in
            try ProjectFileIO.atomicWrite(to: destination, beforeCommit: { [self] in
                guard capy_project_begin_commit(self.handle) == 0 else { throw HostFailure(message: "Document operation cancelled") }
            }) { [self] descriptor in try self.check(capy_project_write(self.handle, descriptor)) }
        }
    }
    func read(from url: URL?) throws {
        guard let url else { try check(capy_project_read(handle, -1)); return }
        try ProjectFileIO.coordinate(url, writing: false) { source in
            let file = try FileHandle(forReadingFrom: source)
            defer { try? file.close() }
            try self.check(capy_project_read(self.handle, file.fileDescriptor))
        }
    }
}

enum ProjectFileIO {
    /// File providers can substitute the coordinated URL. Scope and coordination
    /// enclose the entire read/write, including Rust validation/compression.
    static func coordinate(_ url: URL, writing: Bool, work: @escaping (URL) throws -> Void) throws {
        let access = url.startAccessingSecurityScopedResource()
        defer { if access { url.stopAccessingSecurityScopedResource() } }
        let coordinator = NSFileCoordinator()
        var coordinationError: NSError?, operationError: Error?
        let operation: (URL) -> Void = { location in
            do { try work(location) } catch { operationError = error }
        }
        if writing {
            coordinator.coordinate(writingItemAt: url, options: .forReplacing,
                error: &coordinationError, byAccessor: operation)
        } else {
            coordinator.coordinate(readingItemAt: url, options: [],
                error: &coordinationError, byAccessor: operation)
        }
        if let operationError { throw operationError }
        if let coordinationError { throw coordinationError }
    }
    /// Stream into a private sibling file. The destination is unchanged until
    /// validation/compression/write and fsync all succeed. No Data-sized copy.
    static func atomicWrite(to url: URL, beforeCommit: () throws -> Void = {}, write: (Int32) throws -> Void) throws {
        let directory = url.deletingLastPathComponent()
        let temporary = directory.appendingPathComponent(".\(UUID().uuidString).capy-tmp")
        let descriptor = temporary.path.withCString { open($0, O_WRONLY | O_CREAT | O_EXCL, 0o600) }
        guard descriptor >= 0 else { throw posixFailure() }
        var opened = true
        defer { if opened { close(descriptor) }; try? FileManager.default.removeItem(at: temporary) }
        try write(descriptor)
        guard fsync(descriptor) == 0 else { throw posixFailure() }
        let closed = close(descriptor); opened = false
        guard closed == 0 else { throw posixFailure() }
        try beforeCommit()
        guard temporary.path.withCString({ source in url.path.withCString { rename(source, $0) } }) == 0
            else { throw posixFailure() }
        let directoryDescriptor = directory.path.withCString { open($0, O_RDONLY) }
        guard directoryDescriptor >= 0 else { throw posixFailure() }
        defer { close(directoryDescriptor) }
        guard fsync(directoryDescriptor) == 0 else { throw posixFailure() }
    }
    static func stagingURL(title: String) throws -> URL {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        let name = URL(fileURLWithPath: title).deletingPathExtension().lastPathComponent
        return folder.appendingPathComponent(name.isEmpty ? "Untitled" : name).appendingPathExtension("capy")
    }
    private static func posixFailure() -> NSError { NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
}
