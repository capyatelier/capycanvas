import AppKit
import Metal
import ObjectiveC
import SwiftUI

/// Missing display-timing callbacks must not stop real rendering or artwork history.
@main final class PresentationProgressChecks: NativeWorkspaceInputFixture {
    private final class Callbacks: @unchecked Sendable {
        private let lock = NSLock()
        private var count = 0
        func withheld() { lock.lock(); count += 1; lock.unlock() }
        var total: Int { lock.lock(); defer { lock.unlock() }; return count }
    }
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !ready() {
            try require(Date() < deadline, "Timed out: " + label)
            try await drain(0.01)
        }
    }
    @MainActor static func run() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("capy-presentation-\(UUID())")
        defer { try? FileManager.default.removeItem(at: directory) }
        setenv("CAPY_TRACE_SECONDS", "90", 1)
        setenv("CAPY_TRACE_DIRECTORY", directory.path, 1)
        // Replace only the real drawable's notification registration in this
        // disposable process. Rendering, drawable ownership and presentation
        // still go through the actual Metal implementation.
        let drawableClass: AnyClass = try autoreleasepool {
            let layer = CAMetalLayer()
            layer.device = MTLCreateSystemDefaultDevice()
            layer.drawableSize = CGSize(width: 32, height: 32)
            guard let drawable = layer.nextDrawable() else { throw HostFailure(message: "Metal drawable unavailable") }
            return type(of: drawable)
        }
        guard let method = class_getInstanceMethod(drawableClass, NSSelectorFromString("addPresentedHandler:")) else {
            throw HostFailure(message: "Drawable presentation callback method unavailable")
        }
        let calls = Callbacks()
        let block: @convention(block) (AnyObject, AnyObject) -> Void = { _, _ in calls.withheld() }
        let replacement = imp_implementationWithBlock(block)
        let original = method_setImplementation(method, replacement)
        defer { method_setImplementation(method, original); imp_removeBlock(replacement) }
        for platform: UInt32 in [1, 0] {
            let before = calls.total
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
            let window = NSWindow(contentRect: CGRect(x: 80, y: 80, width: 1200, height: 870),
                styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            defer { window.contentView = nil; window.close() }
            window.contentView = NSHostingView(rootView: EditorView(store: store) { MacMetalCanvas(store: store) })
            window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
            do {
                try await wait("startup without display-timing callbacks") {
                    store.snapshot["shaders_ready"].bool && store.layerThumbnails.images.count == 2
                }
            } catch {
                note("Withheld callbacks: \(calls.total - before); canvas submitted: \(store.canvasSubmitted)")
                throw error
            }
            let id = store.state["layer_tools"]["editing_layer"]["id"].uint
            func pixels() -> Data? {
                guard let image = store.layerThumbnails.images[LayerThumbnails.key(id, false)],
                    let data = image.dataProvider?.data else { return nil }
                return data as Data
            }
            guard let blank = pixels() else { throw HostFailure(message: "Initial artwork preview missing") }
            store.dispatch(["type": "set_color", "rgba": [0.1, 0.3, 0.8, 1]])
            store.invoke("select_all"); store.invoke("fill_selection")
            try await wait("painted preview without display-timing callbacks") { pixels() != nil && pixels() != blank }
            let painted = pixels()!
            store.invoke("undo")
            try await wait("Undo without display-timing callbacks") { pixels() == blank }
            store.invoke("redo")
            try await wait("Redo without display-timing callbacks") { pixels() == painted }
            try require(calls.total - before > 3, "The test must withhold more than one drawable pool's callbacks")
            try require(store.failure == nil && store.snapshot["error"].isNull, store.failure ?? "Renderer error")
            note("PASS platform \(platform): startup, rendered artwork and exact preview Undo/Redo with \(calls.total - before) missing presentation callbacks")
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
