// Actual app-local AppKit contacts against the shared LayerPanel and Rust owner.
// No system menu actions, AX traversal, desktop coordinates or artist storage.
import AppKit
import SwiftUI

private final class LayerTouchEvent: NSEvent {
    var targetWindow: NSWindow?
    var point = CGPoint.zero
    var values: Set<NSTouch> = []
    var eventType: NSEvent.EventType = .gesture
    var time: TimeInterval = 1
    override var window: NSWindow? { targetWindow }
    override var locationInWindow: CGPoint { point }
    override var type: NSEvent.EventType { eventType }
    override var timestamp: TimeInterval { time }
    override func touches(matching phase: NSTouch.Phase, in view: NSView?) -> Set<NSTouch> {
        Set(values.filter { !$0.phase.intersection(phase).isEmpty })
    }
}

private final class LayerTouchValue: NSTouch, @unchecked Sendable {
    let key: NSString
    let state: NSTouch.Phase
    let position: CGPoint
    let kind: NSTouch.TouchType
    init(_ key: String, phase: NSTouch.Phase, position: CGPoint, type: NSTouch.TouchType = .indirect) {
        self.key = key as NSString; state = phase; self.position = position; kind = type
        super.init()
    }
    override var identity: NSCopying & NSObjectProtocol { key }
    override var phase: NSTouch.Phase { state }
    override var normalizedPosition: CGPoint { position }
    override var deviceSize: CGSize { CGSize(width: 200, height: 100) }
    override var type: NSTouch.TouchType { kind }
}

@MainActor private final class LayerTouchRecorder: NativeLayerTouchModel {
    struct Record {
        let id: UInt64
        let phase: String
        let point: CGPoint
        let row: UInt64
        let timestamp: TimeInterval
        let scale: CGFloat
    }
    let contact = ReorderContact()
    var viewport = CGRect.zero
    var enabled = true
    var row: UInt64? = 7
    var records: [Record] = []
    var cancellations = 0
    var layerTouchModel: (any NativeLayerTouchModel)? { self }
    func source(at point: CGPoint) -> ReorderTarget? { nil }
    func acceptsContext(at point: CGPoint) -> Bool { false }
    func context(at point: CGPoint) {}
    func cancel() { cancellations += 1; contact.cancel() }
    func touchRow(at point: CGPoint) -> UInt64? { viewport.contains(point) ? row : nil }
    func layerTouch(_ id: UInt64, phase: String, point: CGPoint, row: UInt64, timestamp: TimeInterval, scale: CGFloat) {
        records.append(Record(id: id, phase: phase, point: point, row: row, timestamp: timestamp, scale: scale))
    }
}

private final class LayerTouchDocument: NSView {
    var mouseEvents: [Int] = []
    override var isFlipped: Bool { true }
    override func mouseDown(with event: NSEvent) { mouseEvents.append(event.eventNumber) }
    override func mouseUp(with event: NSEvent) { mouseEvents.append(event.eventNumber) }
}

@main final class LayerRowInputChecks: NativeWorkspaceInputFixture {
    @MainActor static func rawTouches() async throws {
        let window = NSWindow(contentRect: CGRect(x: 160, y: 160, width: 360, height: 240),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false; window.animationBehavior = .none
        let root = NSView(frame: CGRect(x: 0, y: 0, width: 360, height: 240))
        root.allowedTouchTypes = .direct
        let scroll = NSScrollView(frame: CGRect(x: 24, y: 32, width: 160, height: 100))
        let document = LayerTouchDocument(frame: CGRect(x: 0, y: 0, width: 160, height: 100))
        let marker = ReorderInputView(frame: document.bounds)
        document.addSubview(marker); scroll.documentView = document; root.addSubview(scroll)
        window.contentView = root; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { marker.detach(); marker.model = nil; window.contentView = nil; window.close() }
        try await drain(0.3)
        try require(marker.window === window && root.allowedTouchTypes == .direct,
            "A mounted reorder marker starts without invalid indirect gesture-recognizer types or a touch lease")
        let model = LayerTouchRecorder()
        marker.model = model
        let input = LayerTouchInput(marker: marker, model: model, view: root)
        let second = LayerTouchInput(marker: marker, model: model, view: root)
        defer { input.stop(); second.stop() }
        try require(root.allowedTouchTypes == [.direct, .indirect], "Both raw owners share one indirect NSView lease")
        second.stop()
        try require(root.allowedTouchTypes == [.direct, .indirect], "One stopped owner cannot revoke another owner's indirect touches")
        try self.event(.leftMouseDown, at: CGPoint(x: 40, y: 30), marker: marker, number: 9001)
        try self.event(.leftMouseUp, at: CGPoint(x: 40, y: 30), marker: marker, number: 9002)
        try await drain()
        try require(document.mouseEvents == [9001, 9002], "The active raw collector preserves ordinary native mouse event delivery")
        let event = LayerTouchEvent()
        event.targetWindow = window; event.point = marker.convert(CGPoint(x: 40, y: 30), to: nil)
        func send(_ phase: NSTouch.Phase, key: String = "primary", at point: CGPoint = CGPoint(x: 0.25, y: 0.75),
            type: NSTouch.TouchType = .indirect) {
            event.time += 1; event.values = [LayerTouchValue(key, phase: phase, position: point, type: type)]
            input.handle(event)
        }
        model.enabled = false; send(.began); model.enabled = true
        model.row = nil; send(.began); model.row = 0; send(.began); model.row = 7
        let cover = NSView(frame: root.convert(scroll.bounds, from: scroll))
        root.addSubview(cover); send(.began); cover.removeFromSuperview()
        event.point = marker.convert(CGPoint(x: 180, y: 30), to: nil); send(.began)
        try require(model.records.isEmpty, "Disabled, missing, zero, covered and outside rows cannot admit the first indirect Down")
        event.point = marker.convert(CGPoint(x: 40, y: 30), to: nil)
        send(.began, type: .direct)
        try require(model.records.isEmpty, "Direct touches never enter the trackpad collector")
        send(.began)
        try require(model.records.count == 1 && model.records[0].phase == "down"
            && model.records[0].point == CGPoint(x: 40, y: 30) && model.records[0].row == 7,
            "The first indirect Down anchors at the measured cursor and row")
        let contact = model.records[0].id
        send(.began)
        try require(model.records.count == 1, "Duplicate began phases cannot restart or duplicate a captured Down")
        send(.cancelled, type: .direct); send(.cancelled, key: "unrelated")
        try require(model.records.count == 1, "Unrelated or direct cancellations cannot cancel the captured indirect contact")
        let foreign = NSWindow(contentRect: .zero, styleMask: [], backing: .buffered, defer: true)
        foreign.isReleasedWhenClosed = false
        event.targetWindow = foreign; send(.moved); foreign.close()
        event.targetWindow = nil; send(.moved); event.targetWindow = window
        try require(model.records.count == 1, "Foreign and nil-window events cannot route or retire the owned contact")
        event.point = marker.convert(CGPoint(x: 140, y: 80), to: nil)
        send(.moved, at: CGPoint(x: 0.5, y: 0.5))
        try require(model.records.last?.point == CGPoint(x: 90, y: 55)
            && model.records.last?.id == contact && model.records.last?.row == 7
            && model.records.last?.scale == window.backingScaleFactor && model.records.last?.timestamp == event.time,
            "Device points and inverted normalized Y retain the first cursor anchor and forward backing scale once")
        send(.moved, at: CGPoint(x: 1, y: 0))
        try require(model.records.last?.point == CGPoint(x: 190, y: 105) && model.records.last?.row == 7,
            "An admitted contact stays captured beyond its row and viewport")
        let count = model.records.count
        for type: NSEvent.EventType in [.leftMouseDown, .scrollWheel] {
            event.eventType = type; send(.cancelled)
        }
        event.eventType = .gesture
        try require(model.records.count == count, "Ordinary mouse and wheel events do not route or cancel raw touches")
        send(.ended, at: CGPoint(x: 1, y: 0))
        try require(model.records.last?.phase == "up" && model.records.last?.id == contact,
            "An outside release ends the original contact exactly once")
        send(.ended)
        try require(model.records.count == count + 1, "A duplicate Up cannot reach the model")
        event.point = marker.convert(CGPoint(x: 40, y: 30), to: nil)
        send(.began); send(.cancelled)
        try require(model.records.suffix(2).map(\.phase) == ["down", "cancel"], "Captured indirect cancellation emits one terminal Cancel")
        send(.began); model.enabled = false; send(.moved); model.enabled = true
        try require(model.records.suffix(2).map(\.phase) == ["down", "cancel"], "Disable retires the captured contact")
        let cancellations = model.cancellations
        send(.began); send(.began, key: "second")
        try require(model.records.suffix(2).map(\.phase) == ["down", "down"] && model.cancellations == cancellations + 1,
            "Two admitted raw contacts retire competing mouse recognition once")
        send(.moved, key: "second")
        try require(model.cancellations == cancellations + 1, "Captured motion cannot repeat the two-contact claim")
        input.cancel()
        try require(model.records.suffix(2).map(\.phase) == ["cancel", "cancel"], "Cancelling the raw owner retires both identities")
        send(.began); input.stop()
        try require(model.records.suffix(2).map(\.phase) == ["down", "cancel"] && root.allowedTouchTypes == .direct,
            "Stop cancels the contact and restores the last shared NSView lease")
        let stopped = model.records.count
        send(.began); input.stop()
        try require(model.records.count == stopped && root.allowedTouchTypes == .direct, "Stopped collectors and repeated stop stay inert")
        let unmounted = ReorderInputView(frame: marker.bounds)
        unmounted.model = model
        let unmountedInput = LayerTouchInput(marker: unmounted, model: model, view: root)
        event.targetWindow = nil; unmountedInput.handle(event)
        try require(model.records.count == stopped, "Nil event and marker windows cannot admit an unmounted contact")
        unmountedInput.stop(); event.targetWindow = window
        var temporary: LayerTouchInput? = LayerTouchInput(marker: marker, model: model, view: root)
        weak var released = temporary
        temporary = nil
        try require(released == nil && root.allowedTouchTypes == .direct, "Collector deinit releases its monitor and final touch lease")
        marker.validate()
        try require(root.allowedTouchTypes == [.direct, .indirect], "The mounted marker acquires its model's raw collector")
        marker.model = nil; marker.validate()
        try require(root.allowedTouchTypes == .direct, "Removing the model stops its automatic collector")
        let replacement = LayerTouchRecorder()
        marker.model = replacement; marker.validate()
        try require(root.allowedTouchTypes == [.direct, .indirect] && !input.follows(replacement),
            "A replacement model owns a fresh collector rather than following the retired model")
        marker.detach()
        try require(root.allowedTouchTypes == .direct, "Detaching the mounted marker releases its replacement collector")
        marker.model = nil
        note("PASS raw layer collector: mounted startup, shared leases, native phases/identity, points/Y/anchor/scale, row admission, outside capture, cancellation/window filtering and stop/model replacement")
    }
    @MainActor static func order(_ store: EditorStore) -> [UInt64] { store.state["layers"].array.map { $0["id"].uint } }
    @MainActor static func run(_ platform: UInt32, theme: ColorScheme) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        try await wait("Layer startup timed out") { order(store).count == 2 }
        try await store.apply(["type": "set_theme", "theme": theme == .dark ? "dark" : "light"])
        let original = order(store)[0]
        try await store.apply(["type": "layer", "action": ["op": "new", "group": false, "clipped": false]])
        let initial = order(store), added = initial[0]
        let window = NSWindow(contentRect: CGRect(x: 160, y: 160, width: 380, height: 260),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Layer row input check"; window.isReleasedWhenClosed = false; window.animationBehavior = .none
        window.acceptsMouseMovedEvents = true
        window.appearance = NSAppearance(named: theme == .dark ? .darkAqua : .aqua)
        let panel = JSON(["controls": [["control": "layers", "visible_in_panel": true]]])
        let host = NSHostingView(rootView: LayerPanel(store: store, panel: panel)
            .padding(.leading, 24).padding(.top, 20).environment(\.colorScheme, theme))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { window.contentView = nil; window.close() }
        try await drain(0.3); host.layoutSubtreeIfNeeded()
        guard let marker = find(host), let model = marker.model as? LayerRowInteraction else {
            throw HostFailure(message: "The native layer list marker was not mounted")
        }
        try await wait("Measured layer rows missing") { model.frames.count == 3 && model.enabled }
        func body(_ id: UInt64) -> CGPoint { let row = model.frames[id]!.name; return CGPoint(x: row.midX, y: row.midY) }
        func row(_ id: UInt64) -> JSON { store.state["layers"].array.first { $0["id"].uint == id }! }
        func pickup(_ id: UInt64, handle: Bool) -> CGPoint {
            let frame = handle ? model.frames[id]!.grip : model.frames[id]!.name
            return CGPoint(x: frame.midX, y: frame.midY)
        }
        func lock(_ id: UInt64) async throws {
            try await store.apply(["type": "layer", "action": ["op": "lock", "id": id, "value": true]])
        }
        func undoLock(_ id: UInt64, expected: [UInt64]) async throws {
            try await store.apply(["type": "invoke", "command": "undo"])
            try require(!row(id)["locked"].bool && order(store) == expected,
                "One Undo removes only the lock; rejected pickup adds no history")
        }
        func lockedSources(_ id: UInt64, owner: UInt64) async throws {
            let expected = order(store)
            for device: ReorderDevice in [.mouse, .touch, .pen] {
                for handle in [false, true] {
                    try await lock(owner)
                    try require(row(id)["locked"].bool, "Shared effective lock reaches the native row")
                    let point = pickup(id, handle: handle)
                    guard let target = model.source(at: point) else { throw HostFailure(message: "Locked row source missing") }
                    try require(!target.canDrag, "Locked body and handle retain context without pickup")
                    model.contact.prepare(target, device: device, origin: point)
                    model.contact.recognizeHold()
                    if device == .mouse { model.context(at: point) }
                    try await wait("Locked row context remains available") { !model.menu.isNull }
                    try require(!model.contact.move(to: body(added)) && model.drag == nil && !model.menu.isNull,
                        "Every locked contact keeps its context without a drag preview")
                    try await undoLock(owner, expected: expected)
                    try require(!row(id)["locked"].bool && model.contact.target?.id == target.id
                        && model.contact.target?.canDrag == false && !model.menu.isNull
                        && !model.contact.move(to: body(added)) && model.drag == nil,
                        "Unlock preserves the original context-only capture and never promotes pickup")
                    model.contact.release(at: body(added)); model.cancel()
                }
            }
        }
        try await lockedSources(original, owner: original)
        for device: ReorderDevice in [.mouse, .touch, .pen] {
            for handle in [false, true] {
                let point = pickup(original, handle: handle)
                guard let target = model.source(at: point) else { throw HostFailure(message: "Unlocked row source missing") }
                try require(target.canDrag, "Unlocked row exposes its shared native pickup")
                model.contact.prepare(target, device: device, origin: point)
                try await lock(original)
                try await wait("Lock retires the contact before its first movement") { model.contact.target == nil && model.drag == nil }
                try require(!model.contact.move(to: body(added)), "A retired contact cannot start a ghost")
                model.contact.release(at: body(added))
                try await undoLock(original, expected: initial)
                if target.dragDevices.contains(device) {
                    guard let active = model.source(at: point) else { throw HostFailure(message: "Active row source missing") }
                    model.contact.prepare(active, device: device, origin: point)
                    model.contact.recognizeHold(openContext: false)
                    try require(model.contact.move(to: body(added)) && model.drag?.id == original,
                        "An admitted contact creates the native preview before locking")
                    try await lock(original)
                    try await wait("Lock retires an active drag and its preview") { model.contact.target == nil && model.drag == nil }
                    model.contact.release(at: body(added))
                    try await undoLock(original, expected: initial)
                    try await drain()
                    try require(model.drag == nil, "Late drop feedback cannot restore a cancelled preview")
                }
                model.cancel()
            }
        }
        try await store.apply(["type": "layer", "action": ["op": "new", "group": true, "clipped": false]])
        let group = store.state["layer_tools"]["editing_layer"]["id"].uint
        try await store.apply(["type": "layer", "action": ["op": "new", "group": false, "clipped": false]])
        let child = store.state["layer_tools"]["editing_layer"]["id"].uint
        try await wait("Inherited-lock rows are measured") { model.frames[group] != nil && model.frames[child] != nil }
        try require(row(child)["depth"].uint > row(group)["depth"].uint, "The new row belongs to its ordinary group")
        try await lockedSources(child, owner: group)
        try await store.apply(["type": "invoke", "command": "undo"])
        try await store.apply(["type": "invoke", "command": "undo"])
        try await wait("Lock fixtures restore the original layer order") { order(store) == initial && model.frames[original] != nil && model.frames[added] != nil }
        try await store.apply(["type": "layer", "action": ["op": "select", "id": added, "mask": false]])
        let hold = try holdDuration(marker)
        var start = body(original), end = CGPoint(x: start.x, y: model.frames[added]!.row.minY + 2)
        // A floating panel can cover measured rows without unmounting them.
        // Window-root recognizers must respect the actual view under the contact.
        let blocker = NSView(frame: host.convert(model.frames[original]!.row, from: marker))
        host.addSubview(blocker)
        try event(.leftMouseDown, at: start, marker: marker, number: 1001); try await drain(0.02)
        let admittedCoveredRow = model.contact.target != nil
        try event(.leftMouseUp, at: start, marker: marker, number: 1002); try await drain()
        blocker.removeFromSuperview()
        try require(!admittedCoveredRow, "A covered layer row must not take another view's contact")
        try await store.apply(["type": "layer", "action": ["op": "select", "id": original, "mask": false]])
        window.setContentSize(CGSize(width: 380, height: 90)); try await drain()
        guard let scroll = marker.enclosingScrollView else { throw HostFailure(message: "Layer scroll view missing") }
        func hoverPoint() -> CGPoint {
            let bounds = scroll.contentView.bounds
            return marker.convert(CGPoint(x: bounds.midX, y: bounds.midY), from: scroll.contentView)
        }
        let viewport = scroll.contentView.bounds
        try event(.mouseMoved, at: hoverPoint(), marker: marker, number: 1000)
        try await wait("Mouse input must show the permitted native scrollbar") { scroll.hasVerticalScroller && !scroll.hasHorizontalScroller }
        try require(scroll.contentView.bounds == viewport, "Mouse indicators preserve the native scroll viewport and position")
        scroll.scrollerStyle = .legacy
        host.layoutSubtreeIfNeeded(); try await drain()
        try require(scroll.scrollerStyle == .overlay
            && abs(scroll.contentView.frame.width - scroll.bounds.width) < 0.5,
            "Native scroll reconfiguration must preserve the full shared panel width")
        let scrollStart = body(added), scrollEnd = CGPoint(x: scrollStart.x, y: scrollStart.y - 60)
        try event(.leftMouseDown, at: scrollStart, marker: marker, number: 1003, tablet: true); try await drain(0.02)
        try event(.leftMouseDragged, at: scrollEnd, marker: marker, number: 1004, tablet: true); try await drain(0.03)
        try event(.leftMouseUp, at: scrollEnd, marker: marker, number: 1005, tablet: true); try await drain()
        try require(scroll.contentView.bounds.minY > 0 && model.drag == nil && model.menu.isNull && order(store) == initial
            && store.state["layer_tools"]["editing_layer"]["id"].uint == original,
            "Pre-hold pen movement scrolls the native list without editing or opening a menu")
        try require(!scroll.hasVerticalScroller && !scroll.hasHorizontalScroller,
            "Pen input leaves the row handle edge clear of native scrollbars")
        let scrolled = scroll.contentView.bounds
        try event(.mouseMoved, at: hoverPoint(), marker: marker, number: 1009)
        try await wait("Mouse hover restores the native scrollbar") { scroll.hasVerticalScroller }
        try require(scroll.contentView.bounds == scrolled, "Mouse hover preserves the scrolled list")
        try event(.mouseMoved, at: hoverPoint(), marker: marker, number: 1018, tablet: true)
        try await wait("Pen hover clears the native scrollbar") { !scroll.hasVerticalScroller }
        try require(scroll.contentView.bounds == scrolled, "Pen hover preserves the scrolled list")
        scroll.contentView.scroll(to: .zero); scroll.reflectScrolledClipView(scroll.contentView)
        window.setContentSize(CGSize(width: 380, height: 260)); try await drain()
        guard let pan = scroll.gestureRecognizers.compactMap({ $0 as? NSPanGestureRecognizer }).first(where: {
            guard let delegate = $0.delegate else { return false }
            return String(describing: type(of: delegate)).hasSuffix("PenScrollController")
        }) else {
            throw HostFailure(message: "Native pen scroll recognizer missing")
        }
        let scrollFrame = scroll.frame
        scroll.setFrameOrigin(CGPoint(x: 24, y: 68))
        let windowPoint = marker.convert(body(added), to: nil)
        guard let down = NSEvent.mouseEvent(with: .leftMouseDown, location: windowPoint, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 1006, clickCount: 1, pressure: 0.5), let cg = down.cgEvent else {
            throw HostFailure(message: "Offset pen contact missing")
        }
        cg.setIntegerValueField(.mouseEventSubtype, value: Int64(CGEventMouseSubtype.tabletPoint.rawValue))
        guard let penDown = NSEvent(cgEvent: cg) else { throw HostFailure(message: "Invalid offset pen contact") }
        let admitted = pan.delegate?.gestureRecognizer?(pan, shouldAttemptToRecognizeWith: penDown) == true
        scroll.frame = scrollFrame
        try require(admitted, "An offset native scroll view must accept a pen contact over its layer row")
        start = body(original); end = CGPoint(x: start.x, y: model.frames[added]!.row.minY + 2)
        let actions = store.state["layers"].array.first { $0["id"].uint == original }!["swipe_actions"].array
        try require(actions.map { $0["id"].string } == ["lock", "duplicate", "delete"],
            "Paint rows must expose Lock, Duplicate and Delete")
        for (index, action) in actions.enumerated() {
            start = body(original)
            let row = model.frames[original]!.row, extent = min(row.width, CGFloat(actions.count) * 72)
            let swipeEnd = CGPoint(x: start.x - 90, y: start.y)
            try event(.leftMouseDown, at: start, marker: marker, number: 1010, tablet: true); try await drain(0.02)
            try event(.leftMouseDragged, at: swipeEnd, marker: marker, number: 1011, tablet: true); try await drain(0.04)
            try event(.leftMouseUp, at: swipeEnd, marker: marker, number: 1012, tablet: true); try await drain()
            try require(store.layerSwipe.offset == extent && store.layerSwipe.layer == original && model.drag == nil && model.menu.isNull,
                "A pen row swipe reveals every shared action without reordering or opening a menu")
            try require(order(store) == initial, "Revealing swipe actions does not edit the document")
            try require(!store.layerSwipe.atRest(owner: model.swipeOwner, layer: original), "A displaced row disables pickup")
            let grip = model.frames[original]!.grip
            if grip != .zero, let target = model.source(at: CGPoint(x: grip.midX, y: grip.midY)) {
                try require(target.surface != .handle && !target.canDrag, "A displaced grip cannot admit reorder")
            }
            let target = CGPoint(x: row.maxX - extent + (CGFloat(index) + 0.5) * extent / CGFloat(actions.count), y: row.midY)
            try await drain(0.2)
            try event(.leftMouseDown, at: target, marker: marker, number: 1013)
            try event(.leftMouseUp, at: target, marker: marker, number: 1014)
            try await wait("Revealed \(action["label"].string) must invoke its shared action") {
                switch action["id"].string {
                case "lock": return store.state["layers"].array.first { $0["id"].uint == original }!["locked"].bool
                case "duplicate": return order(store).count == initial.count + 1
                default: return !order(store).contains(original)
                }
            }
            try await store.apply(["type": "invoke", "command": "undo"])
            try require(order(store) == initial && !store.state["layers"].array.first { $0["id"].uint == original }!["locked"].bool,
                "One Undo restores the row order and lock after \(action["label"].string)")
            try await drain()
        }
        start = body(original)
        let returnEnd = CGPoint(x: start.x - 90, y: start.y)
        try event(.leftMouseDown, at: start, marker: marker, number: 1019, tablet: true); try await drain(0.02)
        try event(.leftMouseDragged, at: returnEnd, marker: marker, number: 1020, tablet: true); try await drain(0.04)
        let revealWidth = min(CGFloat(store.state["layers"].array.first { $0["id"].uint == original }!["swipe_actions"].array.count) * 72, model.frames[original]!.root.width)
        let progress = store.layerSwipe.progress(owner: model.swipeOwner, layer: original, revealWidth: revealWidth)
        try require(progress > 0 && progress < 1 && model.frames[original]!.grip.width > 0,
            "A partial swipe fades its retained grip with distance")
        try event(.leftMouseUp, at: returnEnd, marker: marker, number: 1021, tablet: true); try await drain()
        try require(store.layerSwipe.progress(owner: model.swipeOwner, layer: original, revealWidth: revealWidth) == 1,
            "A fully revealed row fades its grip completely")
        let visibleRow = model.frames[original]!.row.intersection(model.viewport)
        let returnPoint = CGPoint(x: visibleRow.midX, y: visibleRow.midY)
        guard !visibleRow.isEmpty, let displaced = model.source(at: returnPoint) else { throw HostFailure(message: "Displaced row contact missing") }
        model.contact.prepare(displaced, device: .mouse, origin: returnPoint)
        store.layerSwipe.close()
        try require(!store.layerSwipe.atRest(owner: model.swipeOwner, layer: original), "Pickup stays disabled through swipe return")
        try await wait("The returned row did not restore its grip") { store.layerSwipe.atRest(owner: model.swipeOwner, layer: original) }
        try require(!model.contact.move(to: body(added)) && model.drag == nil,
            "A contact admitted on a displaced row cannot gain pickup when its swipe returns")
        model.cancel()
        for device: ReorderDevice in [.touch, .pen] {
            guard let target = model.source(at: body(original)) else { throw HostFailure(message: "A resting body must admit context input") }
            model.contact.prepare(target, device: device, origin: body(original))
            model.contact.recognizeHold(openContext: true)
            try await wait("A held direct-contact body did not open its menu") { !model.menu.isNull }
            try require(!model.contact.move(to: body(added)) && !model.contact.dragging && model.drag == nil,
                "Touch and pen body holds cannot reorder")
            model.cancel(); model.closeMenu()
        }
        try await drain()
        start = body(original); end = CGPoint(x: start.x, y: model.frames[added]!.row.minY + 2)
        try event(.leftMouseDown, at: start, marker: marker, number: 1, tablet: true); try await drain(0.02)
        try event(.leftMouseDragged, at: end, marker: marker, number: 2, tablet: true); try await drain(0.04)
        try event(.leftMouseUp, at: end, marker: marker, number: 3, tablet: true); try await drain()
        try require(order(store) == initial && model.drag == nil, "Pen body motion cannot reorder")
        try event(.leftMouseDown, at: start, marker: marker, number: 4, tablet: true); try await drain(hold)
        try await wait("The held pen row did not retain its context menu") { !model.menu.isNull }
        try require(model.contact.held && model.contact.device == .pen,
            "The menu must retain the pen contact: key=\(window.isKeyWindow), held=\(model.contact.held), target=\(String(describing: model.contact.target?.id))")
        try event(.leftMouseDragged, at: end, marker: marker, number: 5, tablet: true); try await drain()
        try require(!model.contact.dragging && model.drag == nil && !model.menu.isNull, "A held pen row body keeps its menu without starting a reorder")
        try event(.leftMouseUp, at: end, marker: marker, number: 6, tablet: true)
        try await drain()
        try require(order(store) == initial && model.drag == nil, "Releasing a held pen row body cannot reorder")
        model.closeMenu()
        start = CGPoint(x: model.frames[original]!.grip.midX, y: model.frames[original]!.grip.midY)
        end = CGPoint(x: start.x, y: model.frames[added]!.row.minY + 2)
        try event(.leftMouseDown, at: start, marker: marker, number: 1006, tablet: true); try await drain(0.02)
        try event(.leftMouseDragged, at: end, marker: marker, number: 1007, tablet: true); try await drain()
        try require(model.contact.dragging && !model.contact.requiresHold, "The pen grip reorders immediately")
        try event(.leftMouseUp, at: end, marker: marker, number: 1008, tablet: true)
        try await wait("Pen grip drop did not publish shared order") { order(store)[0] == original }
        let moved = order(store)
        try await store.apply(["type": "invoke", "command": "undo"])
        try require(order(store) == initial, "One Undo must restore the completed row move")
        try await store.apply(["type": "invoke", "command": "redo"])
        try require(order(store) == moved, "Redo must restore the same row move")
        try await drain()
        start = body(added); end = CGPoint(x: start.x, y: model.frames[original]!.row.minY + 2)
        try event(.leftMouseDown, at: start, marker: marker, number: 7); try await drain(0.02)
        try event(.leftMouseDragged, at: end, marker: marker, number: 8); try await drain()
        try require(model.contact.dragging && model.menu.isNull, "Mouse layer-row bodies must drag immediately")
        try event(.leftMouseUp, at: end, marker: marker, number: 9)
        try await wait("Mouse body drop did not publish shared order") { order(store) == initial }
        start = CGPoint(x: model.frames[added]!.grip.midX, y: model.frames[added]!.grip.midY)
        end = CGPoint(x: start.x, y: start.y + 25)
        try event(.leftMouseDown, at: start, marker: marker, number: 10, tablet: true); try await drain(0.02)
        try event(.leftMouseDragged, at: end, marker: marker, number: 11, tablet: true); try await drain()
        try require(model.contact.dragging && !model.contact.requiresHold, "The pen grip must drag immediately")
        window.orderOut(nil); try await drain()
        try require(model.contact.target == nil && model.drag == nil && order(store) == initial,
            "Focus loss must cancel a layer grip without committing")
        window.makeKeyAndOrderFront(nil); try await drain()
        let paper = initial.last!, paperPoint = body(paper)
        guard let paperTarget = model.source(at: paperPoint) else {
            throw HostFailure(message: "Paper must retain its context source")
        }
        model.contact.prepare(paperTarget, device: .mouse, origin: paperPoint)
        model.contact.recognizeHold(openContext: false)
        try require(!row(paper)["drag_from_handle"].bool && row(paper)["drag_from_body"].array.isEmpty
            && !paperTarget.canDrag && !model.contact.move(to: body(added)) && model.drag == nil,
            "Paper retains context without pickup under the shared row contract")
        model.cancel()
        try require(order(store) == initial, "Cancelling the Paper drag preserves its authored order")
        try await store.apply(["type": "invoke", "command": "quick_mask"])
        try await wait("Quick Mask row geometry") { model.frames[0] != nil }
        try await drain(); host.layoutSubtreeIfNeeded(); try await drain()
        func hasEditableName(_ view: NSView) -> Bool {
            if let field = view as? NSTextField, field.isEditable,
                marker.convert(field.bounds, from: field).intersects(model.frames[0]!.name) { return true }
            return view.subviews.contains(where: hasEditableName)
        }
        try require(!hasEditableName(host) && (window.firstResponder as? NSTextView)?.isFieldEditor != true,
            "Quick Mask must not mount or focus a name editor without a rename owner")
        window.makeFirstResponder(nil)
        model.openMenu(id: original, mask: false)
        try await wait("Quick Mask keeps the native layer menu available") { !model.menu.isNull }
        try await drain()
        try require(store.failure == nil && store.state["layer_tools"]["rename_layer"].isNull,
            "Leaving Quick Mask focus and opening a menu cannot rename layer zero")
        model.closeMenu()
        try require(!store.state["layer_tools"]["quick_mask"].bool && !order(store).contains(0),
            "A normal layer context menu returns from Quick Mask to its artwork target")
        try await store.apply(["type": "invoke", "command": "quick_mask"])
        try await wait("Re-entered Quick Mask row geometry") {
            guard let frame = model.frames[0] else { return false }
            return store.state["layer_tools"]["quick_mask"].bool && order(store).contains(0)
                && [frame.row, frame.name].allSatisfy {
                    $0.width > 0 && $0.height > 0 && [$0.minX, $0.minY, $0.width, $0.height].allSatisfy(\.isFinite)
                }
        }
        let maskPoint = body(0)
        guard let maskTarget = model.source(at: maskPoint) else { throw HostFailure(message: "Quick Mask row missing") }
        model.contact.prepare(maskTarget, device: .pen, origin: maskPoint)
        model.contact.recognizeHold(openContext: false)
        try require(!model.contact.move(to: body(added)) && model.drag == nil,
            "Holding Quick Mask must preserve its nondraggable shared row")
        model.cancel()
        try await store.apply(["type": "invoke", "command": "quick_mask"])
        try await drain()
        try await store.apply(["type": "layer", "action": ["op": "begin_rename", "id": added]])
        try require(model.source(at: body(added)) == nil, "Native name editing owns its contact")
        try await store.apply(["type": "layer", "action": ["op": "cancel_rename"]])
        try await store.apply(["type": "layer", "action": ["op": "select", "id": original, "mask": false]])
        try await store.apply(["type": "layer", "action": ["op": "toggle_selection", "id": added]])
        let selected = Set(store.state["layers"].array.filter { $0["selected"].bool }.map { $0["id"].uint })
        model.openMenu(id: original, mask: false)
        try await wait("Shared context menu did not load") { !model.menu.isNull }
        try require(Set(store.state["layers"].array.filter { $0["selected"].bool }.map { $0["id"].uint }) == selected
            && store.state["layer_tools"]["editing_layer"]["id"].uint == original,
            "Opening a checked row's menu preserves multiselection and sets the drawing target")
        model.closeMenu()
        try await store.apply(["type": "layer", "action": ["op": "toggle_selection", "id": added]])
        try await store.apply(["type": "layer", "action": ["op": "delete_selected"]])
        model.openMenu(id: original, mask: false)
        try require(model.menu.isNull && model.menuSource == nil, "A removed row cannot open a menu")
        try require(store.failure == nil, store.failure ?? "")
        note("PASS platform \(platform): actual covered-row rejection, layer pen body hold/menu and grip drag, immediate mouse body and pen grip, drop/Undo/Redo/focus; shared Paper, rename, multiselection and removed-menu policies")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try await rawTouches(); for platform: UInt32 in [0, 1] { for theme: ColorScheme in [.light, .dark] { try await run(platform, theme: theme) } }; exit(0) }
            catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
