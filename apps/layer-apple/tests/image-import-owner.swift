import Foundation
import QuartzCore
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

/// Real image decoding and serial document ownership, without a file picker.
@main struct ImageImportOwnerChecks {
    @MainActor static func wait(_ label: String, native: NativeOwner, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !ready() {
            guard Date() < deadline else { throw HostFailure(message: "Timed out: \(label)") }
            let prepared = await withCheckedContinuation { done in
                native.flushPersistence { done.resume(returning: $0) }
            }
            try require(prepared, "Prepare pending image work")
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    static func require(_ condition: Bool, _ message: String) throws {
        guard condition else { throw HostFailure(message: message) }
    }
    @MainActor static func main() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-image-owner-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let url = root.appendingPathComponent("Imported image.png")
        let pixels = Data([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 0, 0, 0, 0])
        let image = CGImage(width: 2, height: 2, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: 8, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: CGDataProvider(data: pixels as CFData)!, decode: nil,
            shouldInterpolate: false, intent: .relativeColorimetric)!
        let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, image, nil)
        try require(CGImageDestinationFinalize(destination), "Write the synthetic image")
        let sourceBytes = try Data(contentsOf: url)
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform,
                persistence: EditorPersistence(root: root.appendingPathComponent("state-\(platform)")), managedWorkspaces: false)
            let native = store.native!
            let surface = CAMetalLayer(); surface.bounds = CGRect(x: 0, y: 0, width: 128, height: 128)
            native.attach(surface, width: 128, height: 128, scale: 1)
            defer { native.detach(); withExtendedLifetime(surface) {} }
            let deadline = Date().addingTimeInterval(30)
            while !store.snapshot["shaders_ready"].bool {
                try require(Date() < deadline && store.failure == nil, store.failure ?? "Canvas startup")
                let now = FrameTrace.now()
                await withCheckedContinuation { done in
                    native.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() }
                }
                try await Task.sleep(for: .milliseconds(10))
            }
            // A coordinated writer may temporarily expose incomplete bytes.
            // Image reads must wait for its complete replacement, just as
            // project reads do. This exercises Foundation coordination locally,
            // not a cloud provider or native picker.
            let writing = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0)
            let writer = Task.detached {
                try ProjectFileIO.coordinate(url, writing: true) { location in
                    try Data("incomplete image".utf8).write(to: location)
                    writing.signal()
                    guard release.wait(timeout: .now() + 20) == .success else {
                        throw HostFailure(message: "Coordinated writer was not released")
                    }
                    try sourceBytes.write(to: location)
                }
            }
            defer { release.signal() }
            let started = await withCheckedContinuation { done in
                DispatchQueue.global().async { done.resume(returning: writing.wait(timeout: .now() + 10) == .success) }
            }
            try require(started, "The coordinated image writer must hold the file")
            store.importLayer(url, epoch: store.state["document_file"]["epoch"].uint)
            let heldUntil = Date().addingTimeInterval(1)
            while store.failure == nil && Date() < heldUntil {
                try await Task.sleep(for: .milliseconds(10))
            }
            let prematureError = store.failure
            let prematureImport = store.state["layers"].array.count != 2
            release.signal()
            try await writer.value
            try require(prematureError == nil && !prematureImport,
                "Image import must wait for the coordinated writer instead of reading incomplete bytes: \(prematureError ?? "")")
            try await wait("Coordinated image import", native: native) {
                store.failure != nil || store.state["layers"].array.count == 3
            }
            try require(store.failure == nil, store.failure ?? "")
            store.invoke("undo")
            try await wait("Coordinated import Undo", native: native) { store.state["layers"].array.count == 2 }
            print("PASS platform \(platform): image read waits for coordinated replacement and imports once")
            let task = try await withCheckedThrowingContinuation { (done: CheckedContinuation<NativeProjectTask, Error>) in
                native.projectTask(opening: true) { task, error in
                    if let task { done.resume(returning: task) }
                    else { done.resume(throwing: HostFailure(message: error ?? "Prepare replacement drawing")) }
                }
            }
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                NativeProjectTask.io.async {
                    do { try task.read(from: nil, extent: [64, 64]); done.resume() }
                    catch { done.resume(throwing: error) }
                }
            }
            let originalEpoch = store.state["document_file"]["epoch"].uint
            // Queue replacement before decode completion, while the main-thread
            // editor still owns the old document. No sleeps or decoder hooks.
            let replacementError = await withCheckedContinuation { done in
                native.finishProject(task, opening: true, title: "Replacement", url: nil) { done.resume(returning: $0) }
                store.importLayer(url, epoch: originalEpoch)
            }
            try require(replacementError == nil, replacementError ?? "")
            try await wait("Import completion after document replacement", native: native) {
                store.failure != nil || store.state["layers"].array.contains { $0["label"].string == url.lastPathComponent }
            }
            try require(store.state["document_file"]["epoch"].uint == originalEpoch + 1, "The drawing must actually be replaced")
            try require(store.state["layers"].array.count == 2 && !store.state["document_file"]["modified"].bool,
                "An image chosen for the old drawing must not alter its replacement")
            try require(store.failure == "The drawing changed before the image finished importing. Import it again.",
                "Report the stale import so the user can retry: \(store.failure ?? "no error")")
            let invalid = root.appendingPathComponent("invalid.png")
            try Data("not an image".utf8).write(to: invalid)
            for failedURL in [root.appendingPathComponent("missing.png"), invalid] {
                store.failure = nil
                store.importLayer(failedURL, epoch: store.state["document_file"]["epoch"].uint)
                try await wait("Image read/decode error", native: native) { store.failure != nil }
                try require(store.state["layers"].array.count == 2 && !store.state["document_file"]["modified"].bool,
                    "Failed coordination or decoding must preserve the replacement drawing")
            }
            store.failure = nil
            // Ordinary edits keep the same document identity and must not
            // invalidate a pending image import.
            store.invoke("add_layer")
            store.importLayer(url, epoch: store.state["document_file"]["epoch"].uint)
            try await wait("Current drawing import", native: native) {
                store.failure != nil || store.state["layers"].array.count == 4
            }
            try require(store.failure == nil, store.failure ?? "")
            try require(store.state["layer_tools"]["editing_layer"]["label"].string == url.lastPathComponent,
                "A fresh import after an ordinary edit must become the editing layer")
            let imported = store.state["layers"].stableKey
            store.invoke("undo")
            try await wait("Image import Undo", native: native) { store.state["layers"].array.count == 3 }
            try require(!store.state["layers"].array.contains { $0["label"].string == url.lastPathComponent },
                "Undo removes only the imported layer")
            store.invoke("redo")
            try await wait("Image import Redo", native: native) { store.state["layers"].stableKey == imported }
            try require(try Data(contentsOf: url) == sourceBytes, "Import must preserve the source image")
            print("PASS platform \(platform): stale image rejection, read/decode failures, fresh import after editing, one-step Undo/Redo and unchanged source")
        }
    }
}
