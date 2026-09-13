import AppKit
import SwiftUI

@MainActor private final class MenuFixture: ObservableObject {
    @Published var presented = true
    var actions: [String] = []
    var menu: AppleContextMenu {
        func leaf(_ name: String, enabled: Bool = true) -> [String: Any] {
            ["label": name, "enabled": enabled, "action": ["id": name],
             "bindings": name == "Direct" ? [["key": "z", "command": true, "shift": true]] : []]
        }
        return AppleContextMenu(JSON(["sections": [[
            leaf("Disabled", enabled: false),
            ["label": "Submenu", "enabled": true, "sections": [[leaf("Disabled child", enabled: false), leaf("Nested")]]],
            leaf("Direct")
        ]]])) { self.actions.append($0["id"].string) }
    }
}

private struct MenuFixtureView: View {
    @ObservedObject var fixture: MenuFixture
    var body: some View {
        VStack(alignment: .leading) {
            Text("Menu source").frame(width: 120, height: 36)
                .editorPopover(isPresented: $fixture.presented) {
                    EditorActionMenu(model: fixture.menu) { fixture.presented = false }
                }
            Spacer()
        }.frame(width: 500, height: 400, alignment: .topLeading)
            .modifier(EditorPopoverHost())
    }
}

/// Deliver native keys only to this fixture's window. No system menu automation.
@main final class EditorMenuKeyboardChecks: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        let fixture = MenuFixture()
        let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: 500, height: 400),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        window.contentView = NSHostingView(rootView: MenuFixtureView(fixture: fixture))
        window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await drain(0.3)
        func send(_ value: String, _ code: UInt16) async throws {
            try key(value, code: code, window: window); try await drain()
        }
        try await send("\u{F703}", 124)
        try await send("\r", 36)
        try require(!fixture.presented && fixture.actions == ["Nested"],
            "Right/Return must enter a submenu and activate its first enabled item")
        fixture.presented = true; try await drain()
        try await send("\u{F703}", 124)
        try await send("\u{F702}", 123)
        try await send("\u{F701}", 125)
        try await send("\r", 36)
        try require(!fixture.presented && fixture.actions == ["Nested", "Direct"],
            "Left must return, then Down/Return must execute the next enabled root action")
        fixture.presented = true; try await drain()
        try await send("\u{1b}", 53)
        try require(!fixture.presented && fixture.actions == ["Nested", "Direct"],
            "Escape must dismiss without executing an action")
        fixture.presented = true; try await drain()
        for type: NSEvent.EventType in [.keyDown, .keyUp] {
            let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [.command, .shift],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, characters: "Z", charactersIgnoringModifiers: "Z", isARepeat: false, keyCode: 6)!
            NSApp.postEvent(event, atStart: false)
        }
        try await drain()
        try require(!fixture.presented && fixture.actions == ["Nested", "Direct", "Direct"],
            "Shifted letters must match the shared shortcut's normalized key")
        print("PASS: shared popup keyboard focus, submenus, disabled rows, action dismissal and Escape")
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
