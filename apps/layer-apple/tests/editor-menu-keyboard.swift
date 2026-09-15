import AppKit
import SwiftUI

@MainActor private final class MenuFixture: ObservableObject {
    @Published var presented = true
    var actions: [String] = []
    var selected: String?
    var rowCount = 0
    var menuWidth: CGFloat = 340
    var menu: AppleContextMenu {
        func leaf(_ name: String, enabled: Bool = true) -> [String: Any] {
            ["label": name, "enabled": enabled, "selected": name == selected, "action": ["id": name],
             "bindings": name == "Direct" ? [["key": "z", "command": true, "shift": true]] : []]
        }
        if rowCount > 0 {
            return AppleContextMenu(JSON(["sections": [(0..<rowCount).map { leaf("Row \($0)") }]])) {
                self.actions.append($0["id"].string)
            }
        }
        return AppleContextMenu(JSON(["sections": [[
            leaf("Disabled", enabled: false),
            ["label": "Submenu", "enabled": true, "sections": [[leaf("Disabled child", enabled: false), leaf("Nested"), leaf("Selected child")]]],
            leaf("Direct"),
            ["label": "Later submenu", "enabled": true, "sections": [[
                leaf("First child"),
                ["label": "Deeper submenu", "enabled": true, "sections": [[leaf("Deep child")]]]
            ]]]
        ]]])) { self.actions.append($0["id"].string) }
    }
}

private struct MenuFixtureView: View {
    @ObservedObject var fixture: MenuFixture
    var size = CGSize(width: 500, height: 400)
    var body: some View {
        VStack(alignment: .leading) {
            Text("Menu source").frame(width: 120, height: 36)
                .editorPopover(isPresented: $fixture.presented) {
                    EditorActionMenu(model: fixture.menu, width: fixture.menuWidth) { fixture.presented = false }
                }
            Spacer()
        }.frame(width: size.width, height: size.height, alignment: .topLeading)
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
        fixture.selected = "Direct"
        fixture.presented = true; try await drain()
        try await send("\r", 36)
        try require(!fixture.presented && fixture.actions.last == "Direct" && fixture.actions.count == 4,
            "Return must keep the selected choice when a menu opens")
        fixture.selected = "Selected child"
        fixture.presented = true; try await drain()
        try await send("\u{F703}", 124)
        try await send("\r", 36)
        try require(!fixture.presented && fixture.actions.last == "Selected child" && fixture.actions.count == 5,
            "Opening a submenu must focus its selected enabled choice")
        fixture.selected = "Disabled"
        fixture.presented = true; try await drain()
        try await send("\r", 36)
        try await send("\r", 36)
        try require(!fixture.presented && fixture.actions.last == "Nested" && fixture.actions.count == 6,
            "A selected disabled row must not take focus or prevent navigation")
        fixture.selected = "Direct"
        fixture.presented = true; try await drain()
        try await send("\u{F701}", 125)
        try await send("\u{F703}", 124)
        try await send("\u{F701}", 125)
        try await send("\u{F703}", 124)
        try await send("\u{F702}", 123)
        try await send("\u{F702}", 123)
        try await send("\u{F703}", 124)
        try await send("\u{F701}", 125)
        try await send("\u{F703}", 124)
        try await send("\r", 36)
        try require(!fixture.presented && fixture.actions.last == "Deep child" && fixture.actions.count == 7,
            "Returning through nested menus must restore each parent row, including a later submenu")
        print("PASS: shared popup keyboard focus, submenu return, disabled rows, action dismissal and Escape")
        try await checkWindowFit()
    }
    @MainActor static func checkWindowFit() async throws {
        for size in [CGSize(width: 700, height: 500), CGSize(width: 360, height: 500), CGSize(width: 700, height: 760)] {
            let fixture = MenuFixture(); fixture.rowCount = 30; fixture.menuWidth = 360
            let window = NSWindow(contentRect: CGRect(origin: CGPoint(x: 100, y: 100), size: size),
                styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            defer { window.contentView = nil; window.close() }
            let host = NSHostingView(rootView: MenuFixtureView(fixture: fixture, size: size))
            window.contentView = host; window.makeKeyAndOrderFront(nil)
            try await drain(0.3)
            func scrollViews(_ view: NSView) -> [NSScrollView] {
                (view as? NSScrollView).map { [$0] } ?? view.subviews.flatMap(scrollViews)
            }
            guard let scroll = scrollViews(host).first else { throw HostFailure(message: "Missing native menu scroller") }
            let rect = host.convert(scroll.bounds, from: scroll)
            try require(host.bounds.insetBy(dx: 7, dy: 7).contains(rect),
                "The menu scroller must fit inside the editor with its edge clearance: window=\(host.bounds), menu=\(rect)")
            for _ in 1..<fixture.rowCount { try key("\u{F701}", code: 125, window: window); try await drain(0.01) }
            try await drain()
            guard let document = scroll.documentView else { throw HostFailure(message: "Missing menu scroll content") }
            let visible = scroll.documentVisibleRect
            let remaining = document.isFlipped ? document.bounds.maxY - visible.maxY : visible.minY - document.bounds.minY
            try require(remaining <= 7, "Keyboard navigation must reveal the end of the menu: content=\(document.bounds), visible=\(visible), remaining=\(remaining)")
            if let directory = ProcessInfo.processInfo.environment["CAPY_MENU_CAPTURES"] {
                try FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)
                let capture = Process(); capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
                capture.arguments = ["-x", "-o", "-l", String(window.windowNumber),
                    "\(directory)/menu-\(Int(size.width))-\(Int(size.height)).png"]
                try capture.run(); capture.waitUntilExit()
                try require(capture.terminationStatus == 0, "The owned menu window capture must succeed")
            }
            try key("\r", code: 36, window: window); try await drain()
            try require(!fixture.presented && fixture.actions == ["Row 29"], "The last item must remain reachable in a constrained menu")
            print("PASS: menu fit and last-row navigation at \(Int(size.width))×\(Int(size.height))")
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
