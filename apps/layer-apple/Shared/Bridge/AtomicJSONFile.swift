import Foundation
import Darwin

/// File transport only. Rust validates the version and semantics before restore.
enum AtomicJSONFile {
    static let maximumBytes = 1_048_576
    static func read(_ url: URL) throws -> Data? {
        let file: FileHandle
        do { file = try FileHandle(forReadingFrom: url) }
        catch {
            let error = error as NSError
            if (error.domain == NSCocoaErrorDomain && [NSFileNoSuchFileError, NSFileReadNoSuchFileError].contains(error.code))
                || (error.domain == NSPOSIXErrorDomain && error.code == ENOENT) { return nil }
            throw error
        }
        defer { try? file.close() }
        let data = try file.read(upToCount: maximumBytes + 1) ?? Data()
        guard data.count <= maximumBytes else { throw failure("Saved settings or workspace is too large") }
        _ = try JSONSerialization.jsonObject(with: data)
        return data
    }

    static func write(_ data: Data, to url: URL) throws {
        guard data.count <= maximumBytes else { throw failure("Settings or workspace is too large to save") }
        let directory = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
        let temporary = directory.appendingPathComponent(".\(UUID().uuidString).tmp")
        let descriptor = temporary.path.withCString { open($0, O_WRONLY | O_CREAT | O_EXCL, 0o600) }
        guard descriptor >= 0 else { throw posixFailure() }
        var openDescriptor = true
        defer {
            if openDescriptor { close(descriptor) }
            try? FileManager.default.removeItem(at: temporary)
        }
        try data.withUnsafeBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let count = Darwin.write(descriptor, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw posixFailure() }
                offset += count
            }
        }
        guard fsync(descriptor) == 0 else { throw posixFailure() }
        let closed = close(descriptor); openDescriptor = false
        guard closed == 0 else { throw posixFailure() }
        let replaced = temporary.path.withCString { source in url.path.withCString { rename(source, $0) } }
        guard replaced == 0 else { throw posixFailure() }
        let directoryDescriptor = directory.path.withCString { open($0, O_RDONLY) }
        guard directoryDescriptor >= 0 else { throw posixFailure() }
        defer { close(directoryDescriptor) }
        guard fsync(directoryDescriptor) == 0 else { throw posixFailure() }
    }
    private static func failure(_ message: String) -> NSError {
        NSError(domain: "art.capycanvas.storage", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
    }
    private static func posixFailure() -> NSError { NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
}
