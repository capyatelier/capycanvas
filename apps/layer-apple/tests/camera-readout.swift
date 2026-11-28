import AppKit
import Combine
import SwiftUI

@main final class CameraReadoutChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        let store = EditorStore(platform: 0, persistence: EditorPersistence(root: nil))
        try await wait("editor catalog") { !store.state.isNull && !store.catalog.isNull }
        let camera = store.camera
        let status = CameraStatus(store: store, camera: camera)
        try require(!Mirror(reflecting: status).children.contains { $0.value is ObservedObject<CameraReadout> },
            "The button that owns the zoom menu must not observe the camera")
        let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 320, height: 120),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        let host = NSHostingView(rootView: status.padding(12).frame(width: 320, height: 120)
            .environment(\.editorPopupStore, store).modifier(EditorPopoverHost()))
        window.contentView = host
        window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await drain(0.3)
        func shown() -> String {
            guard let element = nativeAccessibilityElement(host, identifier: "camera-status") else { return "" }
            return element.perform(NSSelectorFromString("accessibilityLabel"))?.takeUnretainedValue() as? String ?? ""
        }
        var published = 0
        let subscription = camera.objectWillChange.sink { published += 1 }
        defer { subscription.cancel() }
        func pose(_ zoom: Double, _ degrees: Double, _ x: Double) -> JSON {
            JSON(["zoom": zoom, "rotation": degrees * .pi / 180, "translation": [x, 0]])
        }
        camera.receive(pose(0.5, 0, 0))
        try await wait("readout 50% · 0°", seconds: 5) { shown() == "50% · 0°" }
        published = 0
        for step in 1...40 { camera.receive(pose(0.5, 0, Double(step) * 7)) }
        try require(published == 0, "A pan must not publish the camera to SwiftUI, published \(published) times")
        for step in 1...40 { camera.receive(pose(0.5 + Double(step) * 0.01, 0, 280)) }
        try require(published == 0 && camera.text == "90% · 0°",
            "A zoom that keeps the readout's length must update only the native label until it settles, published \(published) times")
        try await wait("settled readout 90% · 0°", seconds: 5) { shown() == "90% · 0°" }
        try require(published == 1, "The settled readout publishes once, published \(published) times")
        camera.receive(pose(1.25, 90, 280))
        try require(published == 2, "A readout whose length changes publishes at once so its pill can resize")
        try await wait("readout 125% · 90°", seconds: 5) { shown() == "125% · 90°" }
        print("PASS: camera motion updates the native readout label without publishing to SwiftUI until it settles")
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
