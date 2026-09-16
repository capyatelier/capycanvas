import AppKit
import SwiftUI

@MainActor private final class HeaderInputBattery: BatterySource {
    func start(_ changed: @escaping (DeviceBattery?) -> Void) { changed(nil) }
    func stop() {}
}

/// Native events exercise the real shared header view and its serial Rust owner.
/// Each window has isolated storage; no global input or artist data is used.
@main final class HeaderNativeInputChecks: NativeWorkspaceInputFixture {
    @MainActor static func wait(_ label: String, _ ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !ready() {
            try require(Date() < deadline, "Timed out: \(label)")
            try await drain(0.01)
        }
    }
    @MainActor static func run(_ platform: UInt32) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("Initial models") { !store.snapshot["header"].isNull }
        store.native?.resize(width: 1200, height: 870, scale: 1)
        let window = NSWindow(contentRect: CGRect(x: 60, y: 80, width: 1200, height: 870),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.animationBehavior = .none
        defer { window.contentView = nil; window.close() }
        let notifications = NotificationCenter()
        var clock = Calendar.current.date(from: DateComponents(year: 2026, month: 9, day: 13, hour: 16, minute: 40))!
        let status = SystemStatus(source: HeaderInputBattery(), notifications: notifications, now: { clock })
        let subscription = status.acquire()
        defer { status.release(subscription) }
        let host = NSHostingView(rootView: EditorHeader(store: store, status: status)
            .frame(maxWidth: .infinity, maxHeight: .infinity).coordinateSpace(name: "editor-workspace")
            .modifier(EditorPopoverHost()).environment(\.editorPopupStore, store))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await drain(0.3)
        guard let marker = find(host), marker.model === store.header else {
            throw HostFailure(message: "The actual header native input root must be mounted")
        }
        let header = store.header, hold = try holdDuration(marker)
        func action(_ value: [String: Any]) async throws {
            try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
                store.edit(value) { error in
                    if let error { done.resume(throwing: HostFailure(message: error)) } else { done.resume() }
                }
            }
            try await drain()
        }
        func edit(_ value: [String: Any]) async throws {
            try await action(["type":"customize", "action":["type":"header", "action":value]])
        }
        func source(_ kind: String, component: String? = nil) -> HeaderSource? {
            header.sources.values.filter {
                $0.value["kind"].string == kind && (component == nil || $0.value["value"]["kind"].string == component)
            }.min { $0.bounds.minX < $1.bounds.minX }
        }
        func center(_ source: HeaderSource) -> CGPoint { CGPoint(x: source.bounds.midX, y: source.bounds.midY) }
        func send(_ type: NSEvent.EventType, _ point: CGPoint, pen: Bool = false) async throws {
            try event(type, at: point, marker: marker, number: 0, tablet: pen)
            try await drain(type == .leftMouseDown ? 0.02 : 0.08)
        }
        func layout() -> String { store.state["workspace"]["layout"].stableKey }
        try await action(["type":"window_fullscreen", "fullscreen":true])
        try await action(["type":"invoke", "command":"customize_workspace_ui"])
        try await wait("Clock-change drag source") { source("component", component: "space") != nil }
        let clockSource = source("component", component: "space")!
        try await send(.leftMouseDown, center(clockSource))
        try await send(.leftMouseDragged, CGPoint(x: 650, y: 300))
        try await wait("Clock-change active drag") { header.contact.dragging && !header.preview.isNull }
        let oldTime = status.time, oldGeometry = header.geometry.stableKey
        clock = clock.addingTimeInterval(60)
        notifications.post(name: .NSSystemClockDidChange, object: nil)
        try await drain(0.2)
        try require(status.time != oldTime, "The real status model must deliver the minute change")
        try require(header.contact.dragging && !header.preview.isNull && header.geometry.stableKey == oldGeometry,
            "A minute tick must not resize the monospaced clock or cancel the held title-bar item")
        try await send(.leftMouseUp, CGPoint(x: 650, y: 300))
        try await edit(["type":"cancel"])
        try await action(["type":"window_fullscreen", "fullscreen":false])
        note("PASS platform \(platform): minute update preserves native title-bar drag and geometry")
        for size in ["small", "medium", "large"] {
            try await edit(["type":"set_size", "size":size])
            let initial = layout()
            try await action(["type":"invoke", "command":"customize_workspace_ui"])
            try await wait("Bank and existing items") { source("component", component: "space") != nil && source("item") != nil }
            try require(header.enabled, "Native header editor must accept input")
            for pen in [false, true] {
                let chip = source("component", component: "space")!, start = center(chip)
                try await send(.leftMouseDown, start, pen: pen); try await send(.leftMouseUp, start, pen: pen)
                try require(layout() == initial, "Bank clicks are inert")
                try await send(.leftMouseDown, start, pen: pen); try await drain(hold)
                try require(header.menu.isNull, "Bank holds never open menus")
                try await send(.leftMouseUp, start, pen: pen)
                try require(layout() == initial, "Bank holds are inert")
                // Padding belongs to the whole chip, and is immediate for pen too.
                let padding = CGPoint(x: chip.bounds.minX + 2, y: chip.bounds.midY)
                try await send(.leftMouseDown, padding, pen: pen)
                try require(header.contact.device == (pen ? .pen : .mouse) && !header.contact.requiresHold,
                    "Whole bank chips must classify the device and use native slop without a hold")
                let outside = CGPoint(x: 650, y: 300)
                try await send(.leftMouseDragged, outside, pen: pen)
                try await wait("Detached component") { header.preview["detached"].bool }
                try require(header.contact.dragging && layout() == initial, "Motion previews without publishing workspace changes")
                try await send(.leftMouseUp, outside, pen: pen)
                try await wait("Component released") { header.preview.isNull }
                try require(layout() == initial, "Outside release discards a bank component")

                let existing = source("item")!, itemStart = center(existing)
                try await send(.leftMouseDown, itemStart, pen: pen); try await drain(hold)
                try require(!header.contact.dragging, "Stationary items never start a transaction")
                if pen { try await wait("Pen context") { !header.menu.isNull } }
                else { try require(header.menu.isNull, "Mouse holds never open menus") }
                try await send(.leftMouseDragged, outside, pen: pen)
                try await wait("Detached existing item") { header.preview["detached"].bool }
                try require(header.menu.isNull && layout() == initial, "The same contact closes its menu and keeps motion transient")
                // Moving back into the bar retains the original grab offset.
                let zone = header.geometry["zones"][2].rect
                let end = CGPoint(x: zone.maxX - 1, y: existing.bounds.midY)
                try await send(.leftMouseDragged, end, pen: pen)
                try await wait("Reattached item") { !header.preview.isNull && !header.preview["detached"].bool }
                try await send(.leftMouseUp, end, pen: pen)
                try await wait("Item release applied") { layout() != initial && header.preview.isNull }
                try await edit(["type":"cancel"])
                try require(layout() == initial, "Cancel restores the full arrangement")
                try await action(["type":"invoke", "command":"customize_workspace_ui"])
                try await wait("Editor restored") { source("item") != nil }
            }
            let item = source("item")!, point = center(item)
            try await send(.rightMouseDown, point); try await send(.rightMouseUp, point)
            try await wait("Native secondary-click menu") { !header.menu.isNull }
            try require(header.selected == item.value["value"].uint && layout() == initial && !header.contact.dragging,
                "The native secondary-click handler must select the item and open its menu without changing the arrangement")
            header.closeMenu(); try await drain()
            let generation = header.contact.generation
            try await send(.leftMouseDown, point); try await send(.leftMouseUp, point)
            try require(header.selected == item.value["value"].uint,
                "Click selects the actual item: expected \(item.value), selected \(String(describing: header.selected)), contact generation \(generation)/\(header.contact.generation), suppressed \(header.contact.suppressClick), point \(point)")
            try key("\u{f703}", code: 124, window: window); try await drain()
            try require(layout() != initial, "Native Right moves the selected item")
            let arranged = layout()
            try await edit(["type":"edit", "editing":false])
            try await action(["type":"invoke", "command":"undo_workspace"])
            try require(layout() == initial, "Done commits one ordinary workspace undo step")
            try await action(["type":"invoke", "command":"redo_workspace"])
            try require(layout() == arranged, "Redo restores the whole edit")
            try await action(["type":"invoke", "command":"undo_workspace"])
            note("PASS platform \(platform), \(size): native mouse/pen bank, held context, secondary click, detach/reentry, Cancel, keyboard and Done history")
        }
        window.setContentSize(CGSize(width: 640, height: 480))
        store.native?.resize(width: 640, height: 480, scale: 1)
        let narrow = layout()
        for pen in [false, true] {
            try await action(["type":"invoke", "command":"customize_workspace_ui"])
            try await wait("Narrow overflow") { header.geometry["hidden"].array.contains { !$0.array.isEmpty } }
            let zone = (0..<3).first { !header.geometry["hidden"][$0].array.isEmpty }!
            let bounds = header.geometry["overflow"][zone].rect
            let point = CGPoint(x: bounds.midX, y: bounds.midY)
            try await send(.leftMouseDown, point); try await send(.leftMouseUp, point)
            try await wait("Measured native overflow rows") { header.overflowZone == zone && !header.overflowSources.isEmpty }
            let row = header.overflowSources.values.first!, start = center(row)
            try await send(.leftMouseDown, start, pen: pen)
            try require(!header.contact.requiresHold, "Hidden title-bar items retain the immediate editing rule")
            let outside = CGPoint(x: 380, y: 330)
            try await send(.leftMouseDragged, outside, pen: pen)
            try await wait("Overflow drag") { header.contact.dragging && !header.preview.isNull }
            try require(header.overflowZone == nil && layout() == narrow, "Overflow closes without dropping the original contact or publishing motion")
            try await send(.leftMouseUp, outside, pen: pen)
            try await wait("Hidden item removed") { layout() != narrow && header.preview.isNull }
            try await edit(["type":"cancel"])
            try require(layout() == narrow, "Cancel restores every hidden item")
        }
        note("PASS platform \(platform): actual narrow overflow popup, immediate mouse/pen row removal and parent cancellation")
        try require(store.failure == nil, store.failure ?? "")
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
