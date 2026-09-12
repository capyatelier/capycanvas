// Deliver AppKit events inside an owned fixture window. No AX traversal,
// desktop coordinates, artist storage or system menu actions are used.
import AppKit
import SwiftUI

@main struct WorkspaceSwitcherInputChecks {
    @MainActor static var previousPoint: CGPoint?
    static func note(_ text: String) { FileHandle.standardError.write(Data((text + "\n").utf8)) }
    @MainActor static func require(_ value: Bool, _ message: String) throws {
        if !value { throw HostFailure(message: message) }
    }
    @MainActor static func drain(_ seconds: Double = 0.08) async throws {
        try await Task.sleep(for: .seconds(seconds))
    }
    @MainActor static func find(_ view: NSView) -> ReorderInputView? {
        if let marker = view as? ReorderInputView { return marker }
        return view.subviews.lazy.compactMap(find).first
    }
    @MainActor static func key(_ characters: String, code: UInt16, window: NSWindow) throws {
        for type: NSEvent.EventType in [.keyDown, .keyUp] {
            guard let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, characters: characters, charactersIgnoringModifiers: characters,
                isARepeat: false, keyCode: code) else { throw HostFailure(message: "No native fixture key") }
            NSApp.postEvent(event, atStart: false)
        }
    }
    @MainActor static func event(_ type: NSEvent.EventType, at point: CGPoint, marker: ReorderInputView, number: Int, tablet: Bool = false) throws {
        guard let window = marker.window, let event = NSEvent.mouseEvent(with: type,
            location: marker.convert(point, to: nil), modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
            windowNumber: window.windowNumber, context: nil, eventNumber: number, clickCount: 1, pressure: 0.5) else {
            throw HostFailure(message: "No native fixture event")
        }
        // mouseEvent(with:) leaves buttonNumber at zero even for right-down/up.
        // Rewrap the CGEvent to deliver an actual secondary-button contact.
        guard let cg = event.cgEvent else { throw HostFailure(message: "Missing fixture CGEvent") }
        let button: Int64 = type == .rightMouseDown || type == .rightMouseUp ? 1 : 0
        cg.setIntegerValueField(.mouseEventButtonNumber, value: button)
        cg.setIntegerValueField(.mouseEventNumber, value: Int64(number))
        if tablet { cg.setIntegerValueField(.mouseEventSubtype, value: Int64(CGEventMouseSubtype.tabletPoint.rawValue)) }
        if type == .leftMouseDragged, let previousPoint {
            cg.setDoubleValueField(.mouseEventDeltaX, value: point.x - previousPoint.x)
            cg.setDoubleValueField(.mouseEventDeltaY, value: point.y - previousPoint.y)
        }
        previousPoint = point
        guard let delivered = NSEvent(cgEvent: cg) else { throw HostFailure(message: "Invalid fixture CGEvent") }
        try require(delivered.type == type && delivered.buttonNumber == button
            && delivered.eventNumber == number && delivered.windowNumber == window.windowNumber
            && delivered.locationInWindow == event.locationInWindow,
            "Fixture event must retain the native button, window and measured location: type=\(delivered.type.rawValue)/\(type.rawValue), button=\(delivered.buttonNumber)/\(button), number=\(delivered.eventNumber)/\(number), window=\(delivered.windowNumber)/\(window.windowNumber), location=\(delivered.locationInWindow)/\(event.locationInWindow)")
        try require((delivered.subtype == .tabletPoint) == tablet, "The fixture must retain the actual tablet subtype")
        NSApp.postEvent(delivered, atStart: false)
    }
    @MainActor static func penChecks(marker: ReorderInputView, library: WorkspaceLibrary, manager: WorkspaceManager) async throws {
        let model = marker.model!, window = marker.window!
        let order = library.status["order"].array.map { $0.string }, first = order[0], last = order.last!
        let selected = manager.selection
        let hold = (window.contentView!.gestureRecognizers.first { $0 is NSPressGestureRecognizer } as! NSPressGestureRecognizer).minimumPressDuration + 0.15
        var start = CGPoint(x: model.frames[first]!.row.midX, y: model.frames[first]!.row.midY)
        let end = CGPoint(x: start.x, y: model.frames[last]!.row.maxY - 2)
        try event(.leftMouseDown, at: start, marker: marker, number: 201, tablet: true); try await drain(0.02)
        try require(model.contact.device == .pen, "Tablet-backed mouse events must be classified as pen")
        try event(.leftMouseDragged, at: end, marker: marker, number: 202, tablet: true); try await drain(0.05)
        try require(!model.contact.dragging && model.menu == nil, "Pen row motion must not reorder before a hold")
        try event(.leftMouseUp, at: end, marker: marker, number: 203, tablet: true); try await drain()
        try require(library.status["order"].array.map { $0.string } == order && manager.selection == selected,
            "Early pen motion must preserve order and row selection")
        try event(.leftMouseDown, at: start, marker: marker, number: 204, tablet: true); try await drain(hold)
        try require(model.contact.held && model.menu == first, "A native pen hold must open options")
        try event(.leftMouseUp, at: start, marker: marker, number: 205, tablet: true); try await drain()
        try require(model.menu == first && manager.selection == selected, "Pen lift must retain its menu without selecting the row")
        try key("\u{1b}", code: 53, window: window); try await drain()
        try require(model.menu == nil, "Escape must dismiss the row menu")
        try event(.leftMouseDown, at: start, marker: marker, number: 206, tablet: true); try await drain(hold)
        try event(.leftMouseDragged, at: end, marker: marker, number: 207, tablet: true); try await drain(0.05)
        try require(model.contact.dragging && model.menu == nil, "The held pen contact must continue into a drag")
        try event(.leftMouseUp, at: end, marker: marker, number: 208, tablet: true); try await drain(0.3)
        try require(library.status["order"].array.last?.string == first && manager.selection == selected,
            "A native pen drop must change shared order without previewing the dragged row")
        try await manager.run(JSON(["type": "edit_switcher", "edit": ["type": "move", "id": first, "before": order[1]]]))
        try await drain()
        let frame = model.frames[first]!
        start = CGPoint(x: frame.grip.midX, y: frame.grip.midY)
        try event(.leftMouseDown, at: start, marker: marker, number: 209, tablet: true); try await drain(0.02)
        try event(.leftMouseDragged, at: CGPoint(x: start.x, y: start.y + 20), marker: marker, number: 210, tablet: true); try await drain(0.05)
        try require(model.contact.dragging, "The pen grip must not require a hold")
        window.orderOut(nil); try await drain()
        try require(library.status["order"].array.map { $0.string } == order && model.contact.target == nil,
            "Cancelling a pen grip drag must preserve the original order")
        window.makeKeyAndOrderFront(nil); try await drain()
        try require(window.isKeyWindow, "The fixture must reacquire key focus after ordering its window out")
        try event(.leftMouseUp, at: start, marker: marker, number: 211, tablet: true); try await drain()
        try require(library.status["order"].array.map { $0.string } == order, "A late pen-up must not commit the cancelled drag")
        note("PASS: AppKit tablet subtype, early-motion rejection, hold/lift, same-contact drag, Escape and immediate pen grip")
    }
    @MainActor static func scrolling(marker: ReorderInputView, library: WorkspaceLibrary, manager: WorkspaceManager) async throws {
        try await library.finishLayoutPreview()
        for index in 0..<18 { _ = try await library.operation(["type": "new", "name": String(format: "Scrolling Task %02d", index)]) }
        try await manager.show("workspaces"); try await drain(0.2)
        let model = marker.model!, scroll = marker.enclosingScrollView!
        marker.window!.setContentSize(CGSize(width: 560, height: 240)); try await drain()
        scroll.contentView.scroll(to: .zero); scroll.reflectScrolledClipView(scroll.contentView); try await drain()
        let order = library.status["order"].array.map { $0.string }, first = order[0]
        try require(order.count == 21 && scroll.documentView!.bounds.height > scroll.contentView.bounds.height * 2,
            "The native long-list fixture must overflow its actual scroll viewport")
        let frame = model.frames[first]!, start = CGPoint(x: frame.grip.midX, y: frame.grip.midY)
        try event(.rightMouseDown, at: start, marker: marker, number: 291); try await drain(0.02)
        try event(.rightMouseUp, at: start, marker: marker, number: 292); try await drain()
        try require(model.menu == first, "The long-list scroll check must begin with an open native menu")
        for (phase, cgPhase): (NSEvent.Phase, CGScrollPhase) in [(.began, .began), (.changed, .changed), (.ended, .ended)] {
            guard let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1,
                wheel1: phase == .ended ? 0 : -80, wheel2: 0, wheel3: 0) else {
                throw HostFailure(message: "Missing native fixture scroll event")
            }
            cg.setIntegerValueField(.scrollWheelEventScrollPhase, value: Int64(cgPhase.rawValue))
            guard let wheel = NSEvent(cgEvent: cg), wheel.phase == phase else {
                throw HostFailure(message: "The fixture must deliver real native scroll phases")
            }
            scroll.scrollWheel(with: wheel); try await drain()
        }
        try require(scroll.contentView.bounds.minY > 20 && model.menu == nil,
            "Native scrolling must move the list and dismiss its menu")
        scroll.contentView.scroll(to: .zero); scroll.reflectScrolledClipView(scroll.contentView); try await drain()
        // Read the native clip after resizing. The adapter refreshes its cached
        // viewport on admission; using that pre-event cache targets the old size.
        let viewport = marker.convert(scroll.contentView.bounds, from: scroll.contentView)
        let edge = CGPoint(x: start.x, y: viewport.maxY - 10)
        let initialOffset = scroll.contentView.bounds.minY
        try event(.leftMouseDown, at: start, marker: marker, number: 301); try await drain(0.02)
        try event(.leftMouseDragged, at: edge, marker: marker, number: 302); try await drain(0.6)
        try require(scroll.contentView.bounds.minY > initialOffset + 100 && model.contact.target?.id == first,
            "Edge scrolling must retain capture as the source leaves the viewport: offset=\(scroll.contentView.bounds.minY - initialOffset), target=\(model.contact.target?.id ?? "nil"), dragging=\(model.contact.dragging), viewport=\(model.viewport), point=\(String(describing: model.drag?.point)), clip=\(scroll.contentView.bounds), document=\(scroll.documentView!.bounds), edge=\(edge)")
        let drop = CGPoint(x: start.x, y: marker.convert(scroll.contentView.bounds, from: scroll.contentView).midY)
        try event(.leftMouseDragged, at: drop, marker: marker, number: 303); try await drain()
        guard let hint = model.hint else { throw HostFailure(message: "No measured target after autoscroll") }
        var expected = order.filter { $0 != first }
        expected.insert(first, at: hint.before.flatMap { expected.firstIndex(of: $0) } ?? expected.count)
        try event(.leftMouseUp, at: drop, marker: marker, number: 304); try await drain(0.3)
        try require(library.status["order"].array.map { $0.string } == expected,
            "The shared drop must match the visible insertion hint after autoscroll")
        note("PASS: native scrolling dismisses menus; edge scrolling retains offscreen capture and commits the measured insertion target")
    }
    @MainActor static func run(_ platform: UInt32) async throws {
        note("Starting native input fixture \(platform)")
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-switcher-input-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        let editor = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
        let library = editor.workspaceLibrary!, manager = editor.workspaceManager
        let deadline = Date().addingTimeInterval(20)
        while !library.ready {
            if let error = library.error { throw HostFailure(message: error) }
            try require(Date() < deadline, "Workspace startup timed out"); try await drain(0.01)
        }
        note("Workspace ready")
        try await manager.show("workspaces")
        let window = NSWindow(contentRect: CGRect(x: 160, y: 160, width: 560, height: 320),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Workspace gesture check"; window.isReleasedWhenClosed = false
        window.animationBehavior = .none
        defer { window.contentView = nil; window.close() }
        let host = NSHostingView(rootView: WorkspaceSwitcherRows(manager: manager, library: library).padding(12))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        note("Window mounted")
        try await drain(0.3); host.layoutSubtreeIfNeeded()
        note("Window event pump returned")
        guard let marker = find(host), let model = marker.model else { throw HostFailure(message: "Native list marker was not mounted") }
        try require(marker.enclosingScrollView != nil && model.frames.count == 3 && model.viewport.height > 0,
            "Native capture must use the real scroll content and measured row rectangles")
        let first = manager.view["rows"][0]["id"].string
        let last = manager.view["rows"].array.last!["id"].string
        note("Native geometry ready")
        if ProcessInfo.processInfo.environment["CAPY_SWITCHER_INPUT_CHECK"] == "scroll" {
            try await scrolling(marker: marker, library: library, manager: manager)
            manager.presented = false; manager.dismissed()
            try await drain(0.1); try await library.close()
            note("PASS platform \(platform): focused native scrolling")
            return
        }
        try await penChecks(marker: marker, library: library, manager: manager)
        var start = CGPoint(x: model.frames[first]!.row.midX, y: model.frames[first]!.row.midY)
        try event(.leftMouseDown, at: start, marker: marker, number: 101); try await drain(0.65)
        try require(model.menu == nil && !model.contact.dragging, "A native mouse row hold must not open a context menu")
        note("Mouse hold observed")
        try event(.leftMouseUp, at: start, marker: marker, number: 102); try await drain(0.2)
        try require(manager.selection == first, "A stationary mouse row click must select the row")
        note("Mouse click selected row")
        try event(.rightMouseDown, at: start, marker: marker, number: 103); try await drain(0.02)
        try event(.rightMouseUp, at: start, marker: marker, number: 104); try await drain()
        try require(model.menu == first, "Native secondary click must open row options")
        note("Secondary menu observed")
        try key("\u{F701}", code: 125, window: window); try await drain()
        try key("\r", code: 36, window: window); try await drain(0.3)
        try require(model.menu == nil && library.status["order"][1].string == first,
            "Down/Return must skip the disabled Move Up choice and run Move Down")
        note("Keyboard menu action published shared order")
        start = CGPoint(x: model.frames[first]!.row.midX, y: model.frames[first]!.row.midY)
        let end = CGPoint(x: start.x, y: model.frames[last]!.row.maxY - 2)
        try event(.leftMouseDown, at: start, marker: marker, number: 105); try await drain(0.02)
        try event(.leftMouseDragged, at: CGPoint(x: start.x, y: start.y + 12), marker: marker, number: 106); try await drain(0.03)
        try event(.leftMouseDragged, at: end, marker: marker, number: 106); try await drain(0.05)
        try require(model.contact.dragging && model.hint != nil, "Native mouse row motion must immediately begin a measured reorder")
        try event(.leftMouseUp, at: end, marker: marker, number: 107); try await drain(0.3)
        try require(library.status["order"].array.last?.string == first, "The native drop must publish shared order")
        try require(model.drag == nil && model.hint == nil && model.menu == nil, "Drop must retire native feedback")
        let beforeCancel = library.status["order"].stableKey
        let source = model.frames[first]!
        start = CGPoint(x: source.grip.midX, y: source.grip.midY)
        let target = CGPoint(x: start.x, y: model.frames[last]!.row.minY + 2)
        try event(.leftMouseDown, at: start, marker: marker, number: 108); try await drain(0.02)
        try event(.leftMouseDragged, at: target, marker: marker, number: 109); try await drain(0.05)
        try require(model.contact.dragging, "The native grip must drag without waiting for a hold: target=\(String(describing: model.contact.target?.id)), start=\(start), viewport=\(model.viewport), enabled=\(model.enabled), frame=\(String(describing: model.frames[first]))")
        window.orderOut(nil); try await drain(0.05)
        try require(model.contact.target == nil && model.drag == nil && library.status["order"].stableKey == beforeCancel,
            "Focus loss must cancel the original contact without publishing a drop")
        window.makeKeyAndOrderFront(nil); try await drain()
        try require(window.isKeyWindow, "The fixture must restore native key-window focus")
        try event(.leftMouseUp, at: target, marker: marker, number: 110); try await drain(0.15)
        try require(library.status["order"].stableKey == beforeCancel, "A late mouse-up must not commit the cancelled drag")
        try event(.leftMouseDown, at: start, marker: marker, number: 111); try await drain(0.02)
        try event(.leftMouseDragged, at: target, marker: marker, number: 112); try await drain(0.05)
        try require(model.contact.dragging, "The teardown check must start with an active native drag")
        window.contentView = nil; try await drain()
        try require(model.contact.target == nil && model.drag == nil && library.status["order"].stableKey == beforeCancel,
            "Dismantling a captured list must retire it without publishing into SwiftUI's destroying graph")
        window.contentView = host; window.makeKeyAndOrderFront(nil); try await drain()
        guard let remounted = find(host) else { throw HostFailure(message: "Native list marker was not remounted") }
        try await scrolling(marker: remounted, library: library, manager: manager)
        manager.presented = false; manager.dismissed()
        try await drain(0.1); try await library.close()
        note("PASS platform \(platform): AppKit mouse click/hold, secondary/keyboard options, immediate row/grip pickup, shared drop, focus loss and active-list removal")
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
