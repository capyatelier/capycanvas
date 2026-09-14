// Exercise the app's native source presenter, asynchronous Rust menu request,
// shared preference action and teardown. No system-menu or AX automation.
import AppKit
import SwiftUI

@main final class NativeContextSourceChecks: NativeWorkspaceInputFixture {
    @MainActor static func source(_ view: NSView) -> NativeContextMenuInputView? {
        if let source = view as? NativeContextMenuInputView, source.bounds.size == CGSize(width: 36, height: 36) { return source }
        return view.subviews.lazy.compactMap(source).first
    }
    @MainActor static func activate(_ view: NativeContextMenuInputView) throws {
        guard let window = view.window else { throw HostFailure(message: "Detached fixture source") }
        for type: NSEvent.EventType in [.rightMouseDown, .rightMouseUp] {
            guard let event = NSEvent.mouseEvent(with: type,
                location: view.convert(CGPoint(x: view.bounds.midX, y: view.bounds.midY), to: nil),
                modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 0),
                let cg = event.cgEvent else { throw HostFailure(message: "Missing fixture source event") }
            cg.setIntegerValueField(.mouseEventButtonNumber, value: 1)
            guard let delivery = NSEvent(cgEvent: cg) else { throw HostFailure(message: "Invalid fixture event") }
            NSApp.postEvent(delivery, atStart: false)
        }
    }
    @MainActor static func wait(_ label: String, until ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !ready() {
            try require(Date() < deadline, "Timed out: \(label)"); try await drain(0.01)
        }
    }
    @MainActor static func run(_ platform: UInt32) async throws {
        let editor = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("editor startup") { !editor.state["layers"].array.isEmpty }
        let window = NSWindow(contentRect: CGRect(x: 160, y: 160, width: 1200, height: 870),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Native context source check"; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        defer { window.contentView = nil; window.close() }
        editor.native?.resize(width: 1200, height: 870, scale: 1)
        let host = NSHostingView(rootView: AnyView(ZStack(alignment: .topLeading) {
            Color.clear; EditorHeader(store: editor)
        }.coordinateSpace(name: "editor-workspace").modifier(EditorPopoverHost())))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await drain(0.3); host.layoutSubtreeIfNeeded()
        guard let view = source(host) else { throw HostFailure(message: "Native menu source was not mounted") }
        try require(window.isKeyWindow && view.bounds.size == CGSize(width: 36, height: 36),
            "The presenter must retain a focused, measured 36pt Zen source")
        let load = view.load
        view.load = { completion in
            note("Source requested menu")
            load { model in note("Source received: \(model?.title ?? "nil")"); completion(model) }
        }
        var completed = 0
        var ended = 0
        var menuFailure: String?
        let observer = NotificationCenter.default.addObserver(forName: NSMenu.didBeginTrackingNotification,
            object: nil, queue: .main) { notification in
            MainActor.assumeIsolated {
                guard let menu = notification.object as? NSMenu else { note("Native tracking without menu object"); return }
                note("Native tracking: \(menu.title)")
                guard menu.title == "Capy (Zen Mode)" else { return }
                // Run the app action after native tracking starts. The timer
                // shares the tracking run loop; no menu coordinates are used.
                let timer = Timer(timeInterval: 0.02, repeats: false) { _ in
                    MainActor.assumeIsolated {
                        note("Native action timer fired")
                        if let index = menu.items.firstIndex(where: { $0.title == "Change icon…" && $0.isEnabled }) {
                            menu.performActionForItem(at: index)
                        } else { menuFailure = "Native shared Change icon item missing" }
                        completed += 1; menu.cancelTracking()
                    }
                }
                RunLoop.main.add(timer, forMode: .eventTracking)
                RunLoop.main.add(timer, forMode: .default)
            }
        }
        let endObserver = NotificationCenter.default.addObserver(forName: NSMenu.didEndTrackingNotification,
            object: nil, queue: .main) { _ in MainActor.assumeIsolated { ended += 1; note("Native tracking ended") } }
        defer { NotificationCenter.default.removeObserver(endObserver) }
        defer { NotificationCenter.default.removeObserver(observer) }
        for index in 1...2 {
            guard let current = source(host) else { throw HostFailure(message: "Source disappeared between actions") }
            note("Activate \(index): same=\(current === view), key=\(window.isKeyWindow)")
            window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true); try await drain()
            try activate(current)
            try await wait("native source action and dismissal") { completed == index && ended == index }
            try require(menuFailure == nil, menuFailure ?? "")
            try await wait("shared icon preferences") { !editor.snapshot["preferences"].isNull }
            editor.dispatch(["type":"close_settings"])
            try await wait("closed preferences") { editor.snapshot["preferences"].isNull }
            try await drain()
        }
        guard let view = source(host) else { throw HostFailure(message: "Source disappeared before teardown") }
        var pending: (@MainActor (AppleContextMenu?) -> Void)?
        view.load = { pending = $0 }
        try activate(view)
        try await wait("pending source query") { pending != nil }
        try require(pending != nil, "Activation must request its model asynchronously")
        host.rootView = AnyView(Color.clear); host.layoutSubtreeIfNeeded(); try await drain()
        pending?(AppleContextMenu(JSON(["title": "Capy (Zen Mode)", "sections": [[
            ["label": "Change icon…", "enabled": true, "action": ["id": "unexpected"]]
        ]]])) { _ in menuFailure = "Removed source dispatched a late action" })
        try await drain(0.1)
        try require(completed == 2 && menuFailure == nil && view.window == nil,
            "Removing the source must invalidate the pending answer without reopening or dispatching")
        note("PASS platform \(platform): native source geometry, Rust menu loading, preference actions and pending-query teardown")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { for platform: UInt32 in [0, 1] { try await run(platform) }; exit(0) }
            catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
