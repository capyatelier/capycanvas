// Exercise editor request/acknowledgement behavior without driving system menus
// or asking AppKit to animate a desktop Space. Only invisible test windows exist.
import AppKit

@MainActor private final class TestWindow: NSWindow {
    var observedFullscreen = false
    var transitions = 0
    override var styleMask: NSWindow.StyleMask {
        get { observedFullscreen ? super.styleMask.union(.fullScreen) : super.styleMask.subtracting(.fullScreen) }
        set { super.styleMask = newValue }
    }
    override func toggleFullScreen(_ sender: Any?) {
        transitions += 1
        begin(!observedFullscreen)
    }
    func begin(_ entering: Bool) {
        let notice = Notification(name: entering ? NSWindow.willEnterFullScreenNotification : NSWindow.willExitFullScreenNotification, object: self)
        if entering { delegate?.windowWillEnterFullScreen?(notice) }
        else { delegate?.windowWillExitFullScreen?(notice) }
    }
    func finish(_ entered: Bool) {
        observedFullscreen = entered
        let notice = Notification(name: entered ? NSWindow.didEnterFullScreenNotification : NSWindow.didExitFullScreenNotification, object: self)
        if entered { delegate?.windowDidEnterFullScreen?(notice) }
        else { delegate?.windowDidExitFullScreen?(notice) }
    }
}

@MainActor private final class Downstream: NSObject, NSWindowDelegate {
    var entered = 0, exited = 0, failed = 0
    func windowDidEnterFullScreen(_ notification: Notification) { entered += 1 }
    func windowDidExitFullScreen(_ notification: Notification) { exited += 1 }
    func windowDidFailToEnterFullScreen(_ window: NSWindow) { failed += 1 }
    func windowDidFailToExitFullScreen(_ window: NSWindow) { failed += 1 }
}

@main struct WindowPresentationChecks {
    @MainActor static func wait(_ description: String, _ condition: () -> Bool) async throws {
        let end = Date().addingTimeInterval(20)
        while !condition() {
            precondition(Date() < end, description)
            try await Task.sleep(for: .milliseconds(5))
        }
    }
    @MainActor static func main() async throws {
        setenv("CAPY_DISABLE_PERSISTENCE", "1", 1)
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let mac = EditorStore(platform: 1), otherMac = EditorStore(platform: 1), ipad = EditorStore(platform: 0)
        for store in [mac, otherMac, ipad] {
            try await wait("Initial editor state") { !store.state.isNull }
        }
        precondition(mac.command("fullscreen")["enabled"].bool)
        precondition(!ipad.command("fullscreen")["enabled"].bool, "UIKit has no programmatic full-screen toggle")
        let window = TestWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 500),
            styleMask: [.titled, .resizable], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        let downstream = Downstream(), adapter = DocumentWindowDelegate(store: mac)
        window.delegate = downstream
        adapter.attach(window)
        defer { adapter.attach(nil); window.close() }
        func settled(_ mode: Bool) async throws {
            try await wait("Fullscreen request acknowledgement") {
                mac.state["requests"].array.isEmpty && mac.state["fullscreen"].bool == mode
                    && mac.command("fullscreen")["selected"].bool == mode
            }
        }

        mac.invoke("fullscreen")
        try await wait("First native request") { window.transitions == 1 }
        precondition(!mac.command("fullscreen")["selected"].bool, "Requested mode is not observed mode")
        mac.invoke("fullscreen")
        try await wait("Second queued request") { mac.state["requests"].array.count == 2 }
        window.finish(true)
        try await settled(true)
        precondition(window.transitions == 1, "Repeated enter requests must not toggle back out")
        precondition(mac.command("fullscreen")["icon"].string == "fullscreen-exit")

        mac.invoke("fullscreen")
        try await wait("Exit request") { window.transitions == 2 }
        precondition(mac.command("fullscreen")["selected"].bool)
        window.finish(false)
        try await settled(false)

        // Native controls can change mode without a shared request.
        window.begin(true); window.finish(true)
        try await settled(true)
        window.begin(false)
        mac.invoke("fullscreen")
        try await wait("Request during native transition") { !mac.state["requests"].array.isEmpty }
        precondition(window.transitions == 2)
        window.finish(false)
        try await settled(false)
        precondition(window.transitions == 2)

        // Two observations may arrive before the serial owner's first snapshot.
        window.begin(true); window.finish(true)
        window.begin(false); window.finish(false)
        // Querying the owner provides an ordering fence before inspecting state.
        let menu = await withCheckedContinuation { continuation in
            mac.query(["type": "application_menu", "menu": "view"]) { continuation.resume(returning: $0) }
        }
        let fullscreen = menu["sections"].array.flatMap(\.array).first { $0["action"]["command"].string == "fullscreen" }
        precondition(fullscreen != nil && fullscreen?["selected"].bool == false)
        precondition(mac.failure == nil)

        mac.invoke("fullscreen")
        try await wait("Failed enter request") { window.transitions == 3 }
        adapter.windowDidFailToEnterFullScreen(window)
        try await settled(false)
        precondition(mac.failure != nil && downstream.failed == 1)
        mac.failure = nil

        window.begin(true); window.finish(true)
        try await settled(true)
        mac.invoke("fullscreen")
        try await wait("Failed exit request") { window.transitions == 4 }
        adapter.windowDidFailToExitFullScreen(window)
        try await settled(true)
        precondition(mac.failure != nil && downstream.failed == 2)
        mac.failure = nil

        mac.invoke("fullscreen")
        try await wait("Detached request") { window.transitions == 5 }
        adapter.attach(nil)
        try await settled(true)
        precondition(mac.failure != nil && window.delegate === downstream)
        precondition(downstream.entered == 4 && downstream.exited == 3)
        for other in [otherMac, ipad] {
            precondition(!other.state["fullscreen"].bool && other.state["requests"].array.isEmpty,
                "Window mode must not leak across editor sessions")
        }
        print("PASS: shared fullscreen requests, native observations, rapid transitions, failure/detach, delegate forwarding, and Mac/iPad session isolation")
    }
}
