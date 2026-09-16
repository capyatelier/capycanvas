import AppKit
import SwiftUI

@main final class LayerThumbnailChecks: NativeWorkspaceInputFixture {
    // Reflection only arranges replacement after a real GPU readback starts;
    // acceptance checks the new document's actual preview images.
    @MainActor static func pending(_ cache: LayerThumbnails) -> Int {
        let value = Mirror(reflecting: cache).children.first { $0.label == "pending" }!.value
        return Mirror(reflecting: value).children.count
    }
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(30)
        while !ready() {
            try require(Date() < deadline, "Timed out: \(label)")
            try await drain(0.005)
        }
    }
    @MainActor static func run() async throws {
        for platform: UInt32 in [1, 0] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
            let native = store.native!
            let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 1200, height: 870),
                styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            defer { window.contentView = nil; window.close() }
            window.contentView = NSHostingView(rootView: EditorView(store: store) { MacMetalCanvas(store: store) })
            window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
            try await wait("initial canvas and previews") {
                store.snapshot["shaders_ready"].bool && store.layerThumbnails.images.count == 2
            }
            let task = try await withCheckedThrowingContinuation { (done: CheckedContinuation<NativeProjectTask, Error>) in
                native.projectTask(opening: true) { task, error in
                    if let task { done.resume(returning: task) }
                    else { done.resume(throwing: HostFailure(message: error ?? "Prepare replacement drawing")) }
                }
            }
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                NativeProjectTask.io.async {
                    do { try task.read(from: nil, options: JSON(["extent": [64, 64], "color": ["space": "Srgb", "depth": "U8"], "background": "White"])); done.resume() }
                    catch { done.resume(throwing: error) }
                }
            }
            let epoch = store.state["document_file"]["epoch"].uint
            store.layerThumbnails.reset()
            try await wait("a real GPU preview readback in flight") { pending(store.layerThumbnails) > 0 }
            note("BEFORE platform \(platform): epoch=\(epoch), pending=\(pending(store.layerThumbnails)), images=\(store.layerThumbnails.images.keys.sorted())")
            let error = await withCheckedContinuation { done in
                native.finishProject(task, opening: true, title: "Replacement", url: nil) { done.resume(returning: $0) }
            }
            try require(error == nil, error ?? "")
            try await wait("replacement document publication") { store.state["document_file"]["epoch"].uint == epoch + 1 }
            try await drain(3)
            note("AFTER platform \(platform): epoch=\(store.state["document_file"]["epoch"].uint), pending=\(pending(store.layerThumbnails)), images=\(store.layerThumbnails.images.keys.sorted())")
            let expected = Set(store.state["layers"].array.map { LayerThumbnails.key($0["id"].uint, false) })
            try require(store.failure == nil, store.failure ?? "")
            try require(expected.count == 2 && expected.isSubset(of: Set(store.layerThumbnails.images.keys)),
                "Replacing a document during preview readback must populate both new layer thumbnails")
            note("PASS platform \(platform): document replacement retires old readbacks and populates current thumbnails")
        }
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await run(); exit(0) }
            catch { note("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
