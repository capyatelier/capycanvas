import AppKit
import SwiftUI

/// Native field input and the real window-toolbar Done button, with no artist storage.
@main final class SettingsTextChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        for platform: UInt32 in [0, 1] {
            for (id, draft) in [("light_base", "#12ab34"), ("dark_base", "#234567")] {
                let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
                let deadline = Date().addingTimeInterval(15)
                while store.state.isNull || store.catalog.isNull {
                    try require(Date() < deadline, "Settings owner startup timed out"); try await drain(0.01)
                }
                func action(_ value: [String: Any]) async throws {
                    try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                        store.edit(value) { error in
                            if let error { done.resume(throwing: HostFailure(message: error)) }
                            else { done.resume() }
                        }
                    }
                    try await drain()
                }
                func open() async throws {
                    try await action(["type": "invoke", "command": "settings"])
                    try await action(["type": "preferences", "action": ["type": "page", "page": "appearance"]])
                }
                func value() -> String {
                    store.snapshot["preferences"]["pages"].array.flatMap { $0["groups"].array }
                        .flatMap { $0["rows"].array }.first { $0["id"].string == id }?["kind"]["value"].string ?? ""
                }
                try await open()
                let defaultValue = value()
                let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 900, height: 600),
                    styleMask: [.titled, .closable], backing: .buffered, defer: false)
                window.isReleasedWhenClosed = false
                defer { window.contentView = nil; window.close() }
                let host = NSHostingView(rootView: SettingsView(store: store))
                window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
                try await drain(0.4)
                func fields(_ view: NSView) -> [NSTextField] {
                    (view as? NSTextField).map { [$0] } ?? view.subviews.flatMap(fields)
                }
                guard let field = fields(host).first(where: { $0.isEditable && $0.stringValue == value() }) else {
                    throw HostFailure(message: "Missing native theme-color field")
                }
                field.selectText(nil); try await drain()
                for character in draft { try key(String(character), code: 0, window: window); try await drain(0.01) }
                try await drain()
                try require((window.firstResponder as? NSTextView)?.string == draft, "Native typing must reach the theme-color field")
                func nodes(_ node: Any) -> [any NSAccessibilityProtocol] {
                    guard let item = node as? any NSAccessibilityProtocol else { return [] }
                    return [item] + (item.accessibilityChildren() ?? []).flatMap(nodes)
                }
                guard let done = nodes(window).first(where: { $0.accessibilityIdentifier() == "settings-done" }) else {
                    throw HostFailure(message: "Missing Settings Done button")
                }
                func finish() async throws {
                    let rect = host.convert(window.convertFromScreen(done.accessibilityFrame()), from: nil)
                    for type: NSEvent.EventType in [.leftMouseDown, .leftMouseUp] {
                        try event(type, at: CGPoint(x: rect.midX, y: rect.midY), marker: host, number: 1); try await drain()
                    }
                    try require(store.snapshot["preferences"].isNull, "Done must close Settings")
                    try await open()
                }
                try await finish()
                try require(value() == draft, "Done must retain the typed theme color; reopened value: \(value())")
                guard let field = fields(host).first(where: { $0.isEditable && $0.stringValue == value() }) else {
                    throw HostFailure(message: "Missing reopened native theme-color field")
                }
                field.selectText(nil); try await drain()
                for character in "#abcdef" { try key(String(character), code: 0, window: window); try await drain(0.01) }
                try require((window.firstResponder as? NSTextView)?.string == "#abcdef", "The replacement draft must be focused")
                // Exercise the same shared action as the row's Reset menu item
                // while the native editor retains its unsubmitted draft.
                try await action(["type": "preferences", "action": ["type": "reset", "id": id]])
                try require(value() == defaultValue, "Reset must publish the shared default")
                try require((window.firstResponder as? NSTextView)?.string == defaultValue,
                    "Reset must replace the focused draft with the shared default")
                try await finish()
                try require(value() == defaultValue, "Done must preserve Reset instead of restoring the discarded draft")
                try require(store.failure == nil, store.failure ?? "")
                note("PASS: platform \(platform), native \(id) typing, Done/reopen and focused Reset/Done/reopen")
            }
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
