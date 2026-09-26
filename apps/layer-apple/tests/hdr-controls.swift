import AppKit
import SwiftUI

/// Owned windows and temporary storage; native events exercise actual hit testing.
@main final class HDRControlChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-hdr-controls-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: root) }
        let store = EditorStore(platform: 0, persistence: EditorPersistence(root: root), managedWorkspaces: false)
        let native = store.native!, surface = attachSurface(store, CGSize(width: 256, height: 192))
        defer { native.detach(); withExtendedLifetime(surface) {} }
        func wait(_ label: String, _ ready: () -> Bool) async throws {
            try await CapyTest.wait(label, failure: { store.failure }, step: { await frame(native) }, ready)
        }
        func invoke(_ command: String) async throws { try await store.apply(["type": "invoke", "command": command]) }
        func query(_ action: [String: Any]) async -> JSON {
            await withCheckedContinuation { done in store.query(action) { done.resume(returning: $0) } }
        }
        try await wait("Metal startup") { store.snapshot["shaders_ready"].bool }
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { _, done in done([]) }, save: { _, _, done in done(nil) }, create: { _, done in
            done(JSON(["extent": [128, 96], "color": ["space": "DisplayP3", "depth": "F16"], "background": "White"]))
        }))
        try await invoke("new_document")
        try await wait("HDR document") { store.snapshot["color_panel"]["hdr"].bool && !store.projectFiles.busy }
        try await wait("Float Metal surface") { surface.pixelFormat == .rgba16Float }
        try require(surface.wantsExtendedDynamicRangeContent, "HDR surface opts into native EDR")
        try require(surface.colorspace?.name == CGColorSpace.extendedLinearSRGB, "HDR surface carries linear extended sRGB metadata")
        let proof = store.proof
        _ = await query(["type": "proof_panel", "action": ["type": "mode", "mode": "sdr"]])
        let window = NSWindow(contentRect: CGRect(x: 80, y: 100, width: 226, height: 226), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        let host = NSHostingView(rootView: ProofDial(store: store, controller: proof).frame(width: 226, height: 226))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await wait("Shared proof texture") { ProofGlass.shared.image != nil }
        try await drain(0.1)
        func contact(_ view: NSView) -> ParameterInput.ContactView? {
            if let contact = view as? ParameterInput.ContactView { return contact }
            return view.subviews.lazy.compactMap(contact).first
        }
        guard let input = contact(host) else { throw HostFailure(message: "Native proof capture missing") }
        let size = input.bounds.width
        let geometry = ColorUI.resolve(["type": "proof_dial", "size": size, "recipe": store.snapshot["proof_panel"]["recipe"].raw])
        // The first mount starts before the async illustration arrives. Verify
        // that this same Canvas replaces its placeholder without being remounted.
        host.layoutSubtreeIfNeeded(); host.displayIfNeeded()
        guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
            throw HostFailure(message: "First proof dial capture unavailable")
        }
        host.cacheDisplay(in: host.bounds, to: bitmap)
        var shades: [CGFloat] = []
        for y in 3...7 { for x in 3...7 where abs(x - 5) + abs(y - 5) >= 2 {
            if let c = bitmap.colorAt(x: bitmap.pixelsWide * x / 10, y: bitmap.pixelsHigh * y / 10)?.usingColorSpace(.sRGB) {
                shades.append(c.redComponent)
            }
        } }
        try require((shades.max() ?? 0) - (shades.min() ?? 0) > 0.25, "First-mounted proof dial redraws the loaded glass texture")
        let center = CGPoint(x: size / 2, y: size / 2)
        let baseline = store.snapshot["proof_panel"]["recipe"].stableKey
        try event(.leftMouseDown, at: center, marker: input, number: 1)
        try event(.leftMouseDragged, at: CGPoint(x: size * 0.66, y: size * 0.36), marker: input, number: 2)
        try event(.leftMouseUp, at: CGPoint(x: size * 0.66, y: size * 0.36), marker: input, number: 3)
        try await wait("Dial drag") { store.snapshot["proof_panel"]["recipe"].stableKey != baseline }
        try await invoke("undo")
        try require(store.snapshot["proof_panel"]["recipe"].stableKey == baseline, "One undo restores the complete dial drag")
        try event(.leftMouseDown, at: center, marker: input, number: 4)
        try event(.leftMouseDragged, at: CGPoint(x: size * 0.35, y: size * 0.64), marker: input, number: 5)
        try await drain()
        try require(window.firstResponder === input, "Proof capture owns native keyboard focus")
        try key("\u{1b}", code: 53, window: window)
        try await wait("Escape cancels") { store.snapshot["proof_panel"]["recipe"].stableKey == baseline }
        try event(.leftMouseUp, at: center, marker: input, number: 6)
        try await invoke("redo")
        try require(store.snapshot["proof_panel"]["recipe"].stableKey != baseline, "Cancelled gesture preserves redo")
        _ = await query(["type": "proof_panel", "action": ["type": "control", "part": 3, "edit": ["type": "reset"]]])
        let top = geometry["arcs"][0]["path"][32]
        try event(.leftMouseDown, at: CGPoint(x: top[0].number, y: top[1].number), marker: input, number: 7)
        try event(.leftMouseDragged, at: center, marker: input, number: 8)
        try event(.leftMouseUp, at: center, marker: input, number: 9)
        try await drain()
        try require(store.snapshot["proof_panel"]["recipe"]["balance"].number == 0 && store.snapshot["proof_panel"]["recipe"]["contrast"].number == 1, "Arc capture cannot jump to center control")
        _ = await query(["type": "proof_panel", "action": ["type": "control", "part": 3, "edit": ["type": "reset"]]])
        try key("\u{f703}", code: 124, window: window)
        try await wait("Keyboard brightness") { store.snapshot["proof_panel"]["recipe"]["exposure"].number > 0 }
        try await invoke("undo")
        try require(abs(store.snapshot["proof_panel"]["recipe"]["exposure"].number) < 0.001, "Key release makes one undo step")
        for theme in ["light", "dark"] {
            try await store.apply(["type": "set_theme", "theme": theme])
            for width: CGFloat in [128, 160, 226, 400] {
                let palette = EditorPalette(source: store.state["palette"])
                let content = ProofDial(store: store, controller: proof).frame(width: width, height: width)
                    .foregroundStyle(palette["text"]).background(palette["panel"])
                    .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                let capture = NSHostingView(rootView: content)
                window.contentView = capture; window.setContentSize(CGSize(width: width, height: width))
                try await drain(0.12); capture.layoutSubtreeIfNeeded(); capture.displayIfNeeded()
                if let directory = ProcessInfo.processInfo.environment["CAPY_HDR_CAPTURE"], let bitmap = capture.bitmapImageRepForCachingDisplay(in: capture.bounds) {
                    capture.cacheDisplay(in: capture.bounds, to: bitmap)
                    try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: directory).appendingPathComponent("proof-\(theme)-\(Int(width)).png"))
                }
            }
        }
        let field = ColorFieldImageCache(), viewing = store.colorViewing.replacing("headroom", with: JSON(4)).replacing("stops", with: JSON(2))
        guard let first = field.image(side: 226, hue: 237, shape: .circle, rgbSpace: "DisplayP3", hdr: viewing) else { throw HostFailure(message: "No HDR field") }
        try require(first.bitsPerComponent == 32 && first.colorSpace?.name == CGColorSpace.extendedLinearSRGB, "Picker retains Float32 and native EDR color space")
        try require(first === field.image(side: 226, hue: 237, shape: .circle, rgbSpace: "DisplayP3", hdr: viewing), "Unchanged picker reuses exact image")
        let asyncField = HDRColorFieldImageCache()
        let request = JSON(["side": 96, "hue": 237, "shape": "circle", "space": "DisplayP3", "viewing": viewing.raw])
        for i in 0..<60 { asyncField.request(request.replacing("viewing", with: viewing.replacing("stops", with: JSON(Double(i) / 30)))) }
        let last = viewing.replacing("stops", with: JSON(59.0 / 30))
        let expected = field.image(side: 96, hue: 237, shape: .circle, rgbSpace: "DisplayP3", hdr: last)!.dataProvider!.data!
        try await wait("Coalesced HDR field") {
            guard let image = asyncField.image, let data = image.dataProvider?.data else { return false }
            return CFEqual(data, expected)
        }
        asyncField.request(request); asyncField.cancel()
        try await drain(0.2)
        try require(asyncField.image == nil, "Retired HDR bitmap cannot republish after cancellation")
        var draft = ColorUI.resolve(["type": "form", "request": ["color": ["space": "DisplayP3", "rgba": [1.8, 1.2, 0.4, 0.5]], "document_space": "DisplayP3", "intensity": 2, "change_intensity_text": "bad"]])
        try require(!draft["error"].isNull, "Invalid HDR text is rejected")
        draft = ColorUI.resolve(["type": "form", "request": draft["draft"].raw])
        try require(!draft["error"].isNull, "Invalid EV remains invalid after another field refresh")
        note("PASS: EDR surface, Float32 picker caching/coalescing/cancellation, native proof capture/cancel/keyboard/history, HDR validation and eight proof captures")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in do { try await run(); exit(0) } catch { note("FAIL: \(error)"); exit(1) } }
        NSApp.run()
    }
}
