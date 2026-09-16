import AppKit
import SwiftUI
import Metal
import QuartzCore

/// Retain one actual canvas drawable. The test waits for a later snapshot
/// readback on the same shared wgpu queue before inspecting its rendered pixels.
/// This proves GPU output, not physical presentation timing. Readback usage is
/// test-only; the production layer remains framebuffer-only.
private final class DisplayCaptureLayer: CAMetalLayer, @unchecked Sendable {
    private let lock = NSLock()
    private var armed = false
    private var captured: (any CAMetalDrawable)?
    override var framebufferOnly: Bool {
        get { super.framebufferOnly }
        set { super.framebufferOnly = false }
    }
    func arm() { lock.lock(); captured = nil; armed = true; lock.unlock() }
    func ready() -> Bool { lock.lock(); defer { lock.unlock() }; return captured != nil }
    override func nextDrawable() -> (any CAMetalDrawable)? {
        let drawable = super.nextDrawable()
        lock.lock()
        if armed, drawable != nil { captured = drawable; armed = false }
        lock.unlock()
        return drawable
    }
    func pixels() throws -> [UInt8] {
        lock.lock(); let drawable = captured; lock.unlock()
        guard let drawable, let device, let queue = device.makeCommandQueue(),
            let command = queue.makeCommandBuffer(), let blit = command.makeBlitCommandEncoder() else {
            throw HostFailure(message: "Canvas readback unavailable")
        }
        let texture = drawable.texture, stride = texture.width * 4
        let buffer = device.makeBuffer(length: stride * texture.height, options: .storageModeShared)!
        blit.copy(from: texture, sourceSlice: 0, sourceLevel: 0, sourceOrigin: MTLOrigin(),
            sourceSize: MTLSize(width: texture.width, height: texture.height, depth: 1),
            to: buffer, destinationOffset: 0, destinationBytesPerRow: stride, destinationBytesPerImage: buffer.length)
        blit.endEncoding(); command.commit(); command.waitUntilCompleted()
        guard command.status == .completed else { throw HostFailure(message: "Canvas readback failed") }
        return Array(UnsafeBufferPointer(start: buffer.contents().assumingMemoryBound(to: UInt8.self), count: buffer.length))
    }
}

@main struct ManagedDisplayChecks {
    static func require(_ value: Bool, _ message: String) throws {
        if !value { throw HostFailure(message: message) }
    }
    @MainActor static func edit(_ store: EditorStore, _ action: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            store.edit(action) { error in
                if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
            }
        }
    }
    @MainActor static func wait(_ label: String, store: EditorStore, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(45)
        while !ready() {
            try require(Date() < deadline && store.failure == nil, store.failure ?? "Timed out: \(label)")
            let now = FrameTrace.now()
            await withCheckedContinuation { done in store.native!.frame(now: now, target: now + 16_666_667) { _, _, _ in done.resume() } }
            let prepared = await withCheckedContinuation { done in store.native!.flushPersistence { done.resume(returning: $0) } }
            try require(prepared, "Prepare canvas: \(label)")
            try await Task.sleep(for: .milliseconds(10))
        }
    }
    @MainActor static func swatches() throws {
        func decode(_ v: Double) -> Double { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
        func encode(_ v: Double) -> Int { Int(((v <= 0.0031308 ? v * 12.92 : 1.055 * pow(v, 1 / 2.4) - 0.055) * 255).rounded()) }
        for alpha in [1.0, 0.5] {
            let renderer = ImageRenderer(content: ColorSwatch(rgba: JSON([1.0, 0.5, 0.0, alpha])).frame(width: 20, height: 10))
            renderer.colorMode = .extendedLinear
            guard let image = renderer.cgImage, let space = CGColorSpace(name: CGColorSpace.displayP3),
                let context = CGContext(data: nil, width: 20, height: 10, bitsPerComponent: 8, bytesPerRow: 80,
                    space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue), let data = context.data else {
                throw HostFailure(message: "Native swatch capture unavailable")
            }
            context.draw(image, in: CGRect(x: 0, y: 0, width: 20, height: 10))
            let bytes = data.assumingMemoryBound(to: UInt8.self)
            // Sample each checker level; vertical origin does not affect the pair.
            for level in [0.8, 0.55] {
                let expected = [1.0, 0.5, 0.0].map { encode(decode($0) * alpha + decode(level) * (1 - alpha)) }
                let matches = stride(from: 0, to: 800, by: 4).filter { offset in
                    (0..<3).allSatisfy { abs(Int(bytes[offset + $0]) - expected[$0]) <= 2 }
                }.count
                try require(matches > 25, "SwiftUI P3 swatch alpha \(alpha), checker \(level) must match linear compositing; expected \(expected), matches \(matches)")
            }
        }
        print("PASS: actual SwiftUI swatches preserve P3 chroma and linear alpha")
    }
    @MainActor static func monitors(_ store: EditorStore) async throws {
        let screens = NSScreen.screens
        try require(!screens.isEmpty, "No current display to inspect")
        let view = MacCanvasView(store: store)
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 640, height: 480), styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        window.makeKeyAndOrderFront(nil)
        defer { view.stop(); window.contentView = nil; window.close() }
        let state = store.state["document_file"].stableKey
        for screen in screens + screens.prefix(1) {
            window.setFrameOrigin(CGPoint(x: screen.visibleFrame.midX - 320, y: screen.visibleFrame.midY - 240))
            window.contentView?.layoutSubtreeIfNeeded()
            let deadline = Date().addingTimeInterval(5)
            while window.screen !== screen || store.displayDetails.screen != screen.localizedName {
                try require(Date() < deadline, "Native window must observe its current screen")
                try await Task.sleep(for: .milliseconds(10))
            }
            try require(store.displayDetails.destination == screen.colorSpace?.localizedName, "Display Details must report the screen profile")
        }
        try require(store.state["document_file"].stableKey == state, "Screen moves must preserve document state")
        print("PASS: production AppKit screen observation across \(screens.count) connected display(s), same document state")
    }
    @MainActor static func run() async throws {
        try swatches()
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-managed-display-\(UUID())")
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
            defer { try? FileManager.default.removeItem(at: root) }
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root), managedWorkspaces: false)
            let native = store.native!
            let surface = DisplayCaptureLayer()
            surface.bounds = CGRect(x: 0, y: 0, width: 1024, height: 768)
            native.attach(surface, width: 1024, height: 768, scale: 1)
            defer { native.detach(); withExtendedLifetime(surface) {} }
            try await wait("Metal startup", store: store) { store.snapshot["shaders_ready"].bool }
            func tag() throws {
                try require(surface.colorspace?.name == CGColorSpace.displayP3, "Metal must retain its explicit P3 tag")
                try require(!surface.wantsExtendedDynamicRangeContent, "SDR viewing must not opt into HDR")
            }
            try tag()
            let master = root.appendingPathComponent("Master.capy")
            var space = "Srgb"
            store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { $0(nil) }, save: { _, _, done in done(master) }, create: { _, done in
                done(JSON(["extent": [128, 128], "color": ["space": space, "depth": "U16"], "background": "White"]))
            }))
            func invoke(_ command: String) async throws { try await edit(store, ["type": "invoke", "command": command]) }
            func idle() async throws {
                try await wait("Document completion", store: store) { !store.projectFiles.busy && store.state["requests"].array.isEmpty }
                try require(store.projectFiles.error == nil, store.projectFiles.error ?? "")
            }
            for next in ["Srgb", "DisplayP3", "ProPhoto", "AdobeRgb"] {
                space = next
                try await invoke("new_document"); try await idle()
                let color = JSON(["space": space, "rgba": [0.65, 0.5, 0.25, 1.0]])
                try await edit(store, ["type": "color", "action": ["op": "definition", "color": color.raw]])
                try await invoke("select_all")
                try await edit(store, ["type": "layer", "action": ["op": "fill_selection"]])
                try await invoke("deselect")
                try await invoke("fit_canvas")
                try await invoke("save_document"); try await idle()
                let original = try Data(contentsOf: master)
                let state = store.state["document_file"].stableKey
                let expected = ColorUI.preview(color)["rgba"].array.prefix(3).map { Int(($0.number * 255).rounded()) }
                // Capture a fresh frame from the same surface after each adoption.
                surface.arm(); native.redraw()
                try await wait("Rendered \(space)", store: store) { surface.ready() }
                // This snapshot submits and completes after the captured canvas
                // frame on the same GPU queue, establishing readback ordering.
                try await invoke("export_document")
                try await wait("Export loaded", store: store) { store.projectFiles.exportEditor?.loaded == true }
                let export = store.projectFiles.exportEditor!
                export.preview(export.recipe)
                try await wait("Snapshot readback", store: store) { !export.busy }
                try require(export.error == nil && export.previews.count == 2, export.error ?? "Snapshot previews")
                try require(export.previews.allSatisfy { $0.colorSpace?.name == CGColorSpace.displayP3 }, "Comparison images retain P3 tags")
                let bytes = try surface.pixels()
                export.cancel(); try await idle()
                var matches = 0
                for offset in stride(from: 0, to: bytes.count, by: 4) {
                    let rgb = [Int(bytes[offset + 2]), Int(bytes[offset + 1]), Int(bytes[offset])]
                    if zip(rgb, expected).allSatisfy({ abs($0 - $1) <= 2 }) { matches += 1 }
                }
                if matches <= 100 {
                    var counts: [UInt32: Int] = [:]
                    for offset in stride(from: 0, to: bytes.count, by: 4) {
                        let key = UInt32(bytes[offset + 2]) << 16 | UInt32(bytes[offset + 1]) << 8 | UInt32(bytes[offset])
                        counts[key, default: 0] += 1
                    }
                    print("Dominant RGB:", counts.sorted { $0.value > $1.value }.prefix(8), "camera:", store.state["camera"].stableKey)
                }
                try require(matches > 100, "Actual \(space) presentation must match the P3 swatch, expected \(expected), matched \(matches) pixels")
                try tag()
                store.layerThumbnails.show(token: "display", id: 1)
                try await wait("P3 thumbnail", store: store) { store.layerThumbnails.images["1:false"] != nil }
                try require(store.layerThumbnails.images["1:false"]?.colorSpace?.name == CGColorSpace.displayP3, "Thumbnail tag")
                native.resize(width: 1152, height: 768, scale: 1)
                surface.arm(); native.redraw()
                try await wait("Surface resize", store: store) { surface.ready() }
                try tag()
                native.resize(width: 1024, height: 768, scale: 1)
                try require(store.state["document_file"].stableKey == state && (try Data(contentsOf: master)) == original, "Viewing must preserve document state and saved bytes")
                print("PASS platform \(platform), \(space): rendered Metal pixels match P3 controls; adoption, resize, tags and saved bytes preserved")
            }
            // Replacing the presentation layer must preserve the live drawing.
            let replacement = CAMetalLayer()
            native.attach(replacement, width: 1024, height: 768, scale: 1)
            let flushed = await withCheckedContinuation { done in native.flushPersistence { done.resume(returning: $0) } }
            try require(flushed && replacement.colorspace?.name == CGColorSpace.displayP3, "Layer replacement retains managed viewing")
            if platform == 1 { try await monitors(store) }
        }
    }
    @MainActor static func main() {
        setbuf(stdout, nil)
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await run(); exit(0) }
            catch { print("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
