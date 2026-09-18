import Foundation
import Darwin

/// Run inside a signed App Sandbox with a grant for one disposable file only.
/// The runner creates its bookmark before signing, without touching artist data.
@main struct ProjectFileAccessChecks {
    static func main() throws {
        if CommandLine.arguments.count == 3 {
            let source = URL(fileURLWithPath: CommandLine.arguments[1])
            try Data("original".utf8).write(to: source)
            let bookmark = try source.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
            try bookmark.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
            return
        }
        let data = try Data(contentsOf: Bundle.main.url(forResource: "input", withExtension: "bookmark")!)
        var stale = false
        let destination = try URL(resolvingBookmarkData: data, options: [.withoutUI, .withoutImplicitStartAccessing],
            relativeTo: nil, bookmarkDataIsStale: &stale)
        precondition(!stale && (try? Data(contentsOf: destination)) == nil, "The sandbox must deny access before the grant")
        func read() throws -> Data { try ProjectFileIO.coordinate(destination, writing: false) { try Data(contentsOf: $0) } }
        func write(_ value: String, fail: Bool = false, cancel: Bool = false) throws {
            try ProjectFileIO.coordinate(destination, writing: true) { location in
                try ProjectFileIO.atomicWrite(to: location, beforeCommit: {
                    if cancel { throw HostFailure(message: "cancelled") }
                }) { descriptor in
                    let bytes = Array(value.utf8)
                    precondition(bytes.withUnsafeBytes { Darwin.write(descriptor, $0.baseAddress, $0.count) } == bytes.count)
                    if fail { throw HostFailure(message: "write failed") }
                }
            }
        }
        let original = try read()
        precondition(original == Data("original".utf8))
        for (fail, cancel) in [(true, false), (false, true)] {
            do { try write("incomplete", fail: fail, cancel: cancel); preconditionFailure("Expected cancellation/failure") }
            catch let error as HostFailure { precondition(error.message == (fail ? "write failed" : "cancelled")) }
            let unchanged = try read()
            precondition(unchanged == original, "Failed saves must leave the original intact")
        }
        for revision in 1...3 {
            let text = "saved revision \(revision)"
            try write(text)
            let saved = try read()
            precondition(saved == Data(text.utf8), "Repeated Save must replace the selected file")
        }
        try ProjectFileIO.coordinate(destination, writing: true) { location in
            let sibling = location.deletingLastPathComponent().appendingPathComponent("unselected.capy")
            do { try Data().write(to: sibling); preconditionFailure("The grant must not cover arbitrary siblings") }
            catch { precondition((error as NSError).domain == NSCocoaErrorDomain) }
        }
        precondition((try? Data(contentsOf: destination)) == nil, "Every scope acquisition must be balanced")
        print("PASS: file-only sandbox grant, failed/cancelled write preservation, three atomic replacements, exact reads and balanced access")
    }
}
