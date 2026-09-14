import AppKit
import SwiftUI

@main struct EditorAppearanceChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        let store = EditorStore(platform: 1, persistence: EditorPersistence(root: nil))
        let window = NSWindow(contentRect: CGRect(x: 120, y: 120, width: 1200, height: 850), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; defer { window.contentView = nil; window.close() }
        let oldAppearance = NSApp.appearance; defer { NSApp.appearance = oldAppearance }
        NSApp.appearance = NSAppearance(named: .aqua)
        let host = NSHostingController(rootView: EditorView(store: store) { Color.clear })
        window.contentViewController = host; window.makeKeyAndOrderFront(nil)
        func awaitTheme(_ expected: String) async throws {
            let deadline = Date().addingTimeInterval(10)
            while true {
                let actual = host.view.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? "dark" : "light"
                if actual == expected && store.state["theme"].string == expected { return }
                guard Date() < deadline else { throw HostFailure(message: "Theme mismatch: expected \(expected), native \(actual), shared \(store.state["theme"].string)") }
                try await Task.sleep(for: .milliseconds(20))
            }
        }
        func set(_ theme: String?) async throws {
            try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                store.edit(["type": "set_theme", "theme": theme as Any? ?? NSNull()]) { error in
                    if let error { c.resume(throwing: HostFailure(message: error)) } else { c.resume() }
                }
            }
        }
        try await awaitTheme("light")
        try await set("dark"); try await awaitTheme("dark")
        try await set(nil); try await awaitTheme("light")
        NSApp.appearance = NSAppearance(named: .darkAqua); try await awaitTheme("dark")
        try await set("light"); try await awaitTheme("light")
        try await set(nil); try await awaitTheme("dark")
        print("PASS: shared editor and native host follow explicit Light/Dark, return to System and application appearance changes")
    }
}
