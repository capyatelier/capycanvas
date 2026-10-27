// Exercise editor request/acknowledgement behavior without driving system menus
// or asking AppKit to animate a desktop Space. Only invisible test windows exist.
import AppKit
import SwiftUI

@MainActor private final class TestWindow: NSWindow {
    var observedFullscreen = false
    var transitions = 0
    var focusRequests = 0, closeRequests = 0
    override func makeKeyAndOrderFront(_ sender: Any?) { focusRequests += 1 }
    override func performClose(_ sender: Any?) { closeRequests += 1 }
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

@MainActor private final class CanvasLifetime: ObservableObject {
    @Published var identity: Int?
}

private struct LifetimeEditor: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var canvas: CanvasLifetime
    var body: some View {
        EditorView(store: store) {
            if let identity = canvas.identity {
                MacMetalCanvas(store: store).id(identity).frame(width: 0, height: 0)
            } else { Color.clear }
        }
    }
}

@main struct WindowPresentationChecks {
    @MainActor static func main() {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        Task { @MainActor in
            do { try await run(); exit(0) }
            catch {
                FileHandle.standardError.write(Data("FAIL: \(error)\n".utf8))
                exit(1)
            }
        }
        NSApp.run()
    }
    @MainActor static func run() async throws {
        let persistence = EditorPersistence(root: nil)
        let mac = EditorStore(platform: 1, persistence: persistence)
        let otherMac = EditorStore(platform: 1, persistence: persistence)
        let ipad = EditorStore(platform: 0, persistence: persistence)
        for store in [mac, otherMac, ipad] {
            try await wait("Initial editor state") { !store.state.isNull }
        }
        if CommandLine.arguments.contains("--ownership-only") {
            try await delegateOwnership(mac, other: otherMac); return
        }
        if CommandLine.arguments.contains("--bridge-only") {
            try await bridgeLifetime(); return
        }
        precondition(mac.command("fullscreen")["enabled"].bool)
        precondition(!ipad.command("fullscreen")["enabled"].bool, "UIKit has no programmatic full-screen toggle")
        let window = TestWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 500),
            styleMask: [.titled, .resizable], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        let downstream = Downstream(), adapter = DocumentWindowDelegate(store: mac)
        window.delegate = downstream
        window.tabbingMode = .preferred
        adapter.attach(window)
        precondition(window.tabbingMode == .disallowed, "Native tabs must not cover the shared editor header")
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
        mac.failure = nil
        try await delegateOwnership(mac, other: otherMac)
        try await bridgeLifetime()
    }

    @MainActor private static func testWindow() -> TestWindow {
        let window = TestWindow(contentRect: CGRect(x: 0, y: 0, width: 700, height: 500),
            styleMask: [.titled, .resizable], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        return window
    }
    @MainActor private static func bindings(_ store: EditorStore, window: TestWindow,
        owner: DocumentWindowDelegate, _ label: String) throws {
        try require(store.projectFiles.presentationWindow === window && window.delegate === owner
            && store.focusWindow != nil && store.projectFiles.closeWindow != nil
            && store.windowPresentation.changeFullscreen != nil, label)
        let focus = window.focusRequests, close = window.closeRequests
        store.focusWindow?(); store.projectFiles.closeWindow?()
        try require(window.focusRequests == focus + 1 && window.closeRequests == close + 1,
            label + ": callbacks must still target the active native window")
        let original = window.observedFullscreen
        for target in [!original, original] {
            var replies = 0, observed: Bool?, failure: String?
            let transitions = window.transitions
            store.windowPresentation.changeFullscreen?(target) { actual, error in
                replies += 1; observed = actual; failure = error
            }
            try require(window.transitions == transitions + 1, label + ": fullscreen must reach the active delegate")
            window.finish(target)
            try require(replies == 1 && observed == target && failure == nil,
                label + ": the active delegate must finish each fullscreen callback exactly once")
        }
    }
    @MainActor private static func delegateOwnership(_ store: EditorStore, other: EditorStore) async throws {
        let isolated = testWindow(), first = testWindow(), second = testWindow()
        let isolatedDownstream = Downstream(), firstDownstream = Downstream(), secondDownstream = Downstream()
        isolated.delegate = isolatedDownstream; first.delegate = firstDownstream; second.delegate = secondDownstream
        let isolatedOwner = DocumentWindowDelegate(store: other)
        isolatedOwner.attach(isolated)
        defer { isolatedOwner.attach(nil); isolated.close(); first.close(); second.close() }
        let initial = DocumentWindowDelegate(store: store), unattached = DocumentWindowDelegate(store: store)
        initial.attach(first)
        unattached.attach(nil)
        try bindings(store, window: first, owner: initial,
            "An unattached delegate's detach must preserve the active presenter and bindings")
        initial.attach(nil)
        try require(first.delegate === firstDownstream && store.projectFiles.presentationWindow == nil
            && store.projectFiles.closeWindow == nil && store.focusWindow == nil
            && store.windowPresentation.changeFullscreen == nil, "Active detach must restore the original delegate and clear its own bindings")
        let replacement = DocumentWindowDelegate(store: store)
        replacement.attach(first)
        initial.attach(nil); initial.attach(nil)
        try bindings(store, window: first, owner: replacement,
            "Repeated retired delegate detach must preserve its replacement presenter")
        replacement.attach(nil)
        try require(first.delegate === firstDownstream, "Replacement teardown must restore the original downstream delegate")
        for sameWindow in [true, false] {
            let old = DocumentWindowDelegate(store: store), current = DocumentWindowDelegate(store: store)
            let destination = sameWindow ? first : second
            old.attach(first); current.attach(destination)
            old.attach(nil); old.attach(nil)
            try bindings(store, window: destination, owner: current,
                "First late old detach must preserve replacement ownership on \(sameWindow ? "the same" : "a different") window")
            if !sameWindow { try require(first.delegate === firstDownstream, "The retired window must regain its original downstream") }
            let downstream = sameWindow ? firstDownstream : secondDownstream
            try require(downstream.entered > 0 && downstream.exited > 0, "Native fullscreen observations must reach the original downstream")
            current.attach(nil)
            try require(destination.delegate === downstream, "Teardown must bypass obsolete adapters and restore the original downstream")
        }
        try require(other.projectFiles.presentationWindow === isolated && isolated.delegate === isolatedOwner
            && isolated.focusRequests == 0 && isolated.closeRequests == 0 && isolated.transitions == 0,
            "Retired delegates must not alter another store's native window")
        try bindings(other, window: isolated, owner: isolatedOwner, "The other store's native callbacks must remain usable")
        print("PASS: unattached/repeated/late same-window and different-window delegate teardown preserves presenter, callbacks, fullscreen, downstream and store isolation")
    }
    @MainActor private static func first<T: NSView>(_ type: T.Type, in view: NSView) -> T? {
        (view as? T) ?? view.subviews.lazy.compactMap { first(type, in: $0) }.first
    }
    @MainActor private static func bridgeLifetime() async throws {
        let store = EditorStore(platform: 1, persistence: EditorPersistence(root: nil), managedWorkspaces: false, deferLaunch: true)
        try require(store.native == nil, "The lifetime fixture must begin before native startup")
        let window = testWindow(), downstream = Downstream(), canvas = CanvasLifetime()
        window.delegate = downstream
        let host = NSHostingController(rootView: LifetimeEditor(store: store, canvas: canvas))
        window.contentViewController = host
        defer { window.contentViewController = nil; window.close() }
        try await wait("Stable EditorView bridge must start and bind its native window without a canvas", seconds: 5,
            step: { host.view.layoutSubtreeIfNeeded() }) {
                store.native != nil && store.projectFiles.presentationWindow === window
                    && self.first(EditorAppearanceView.self, in: host.view) != nil
            }
        try await wait("Stable bridge initial shared state", failure: { store.failure }) { !store.state.isNull }
        guard let bridge = first(EditorAppearanceView.self, in: host.view),
            let owner = window.delegate as? DocumentWindowDelegate else { throw HostFailure(message: "The native appearance bridge must own the document delegate") }
        try require(first(MacCanvasView.self, in: host.view) == nil, "Color.clear must not create a Metal canvas")
        try bindings(store, window: window, owner: owner, "The stable bridge must own presentation before canvas creation")
        var previous: MacCanvasView?
        for identity in [1, 2] {
            let retired = previous
            canvas.identity = identity
            try await wait("Native canvas replacement \(identity)", seconds: 5, step: { host.view.layoutSubtreeIfNeeded() }) {
                self.first(MacCanvasView.self, in: host.view).map { $0 !== retired } == true
            }
            guard let current = first(MacCanvasView.self, in: host.view) else { throw HostFailure(message: "The production Mac canvas must mount") }
            try require(current.window === window && current.bounds.isEmpty, "Zero-extent production canvases must mount without a Metal surface")
            retired?.stop(); retired?.stop()
            try require(first(EditorAppearanceView.self, in: host.view) === bridge,
                "Canvas replacement must retain the stable appearance bridge")
            try bindings(store, window: window, owner: owner, "Canvas replacement and late dismantle must preserve the window presenter")
            previous = current
        }
        canvas.identity = nil
        try await wait("Native canvas dismantle", seconds: 5, step: { host.view.layoutSubtreeIfNeeded() }) {
            self.first(MacCanvasView.self, in: host.view) == nil
        }
        previous?.stop(); previous?.stop()
        try require(first(EditorAppearanceView.self, in: host.view) === bridge, "Removing the canvas must retain the window lifetime bridge")
        try bindings(store, window: window, owner: owner, "A window without a canvas must retain presentation callbacks")
        window.contentViewController = nil
        try await wait("Native appearance bridge teardown", seconds: 5) {
            window.delegate === downstream && store.projectFiles.presentationWindow == nil
                && store.projectFiles.closeWindow == nil && store.focusWindow == nil
                && store.windowPresentation.changeFullscreen == nil
        }
        print("PASS: deferred EditorView Color.clear startup, stable native appearance ownership, canvas replacement/dismantle and final window teardown")
    }
}
