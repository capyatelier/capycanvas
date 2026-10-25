import SwiftUI
import AppKit
import AppKit.NSGestureRecognizerSubclass

struct NativeReorderInput: NSViewRepresentable {
    let model: any NativeReorderModel
    func makeNSView(context: Context) -> ReorderInputView { ReorderInputView() }
    func updateNSView(_ view: ReorderInputView, context: Context) { view.model = model; view.validate() }
    static func dismantleNSView(_ view: ReorderInputView, coordinator: ()) {
        view.detach(); view.model = nil
    }
}

final class ReorderInputView: NSView, NSGestureRecognizerDelegate {
    weak var model: (any NativeReorderModel)?
    private weak var attached: NSView?
    private var downEvent: NSEvent?
    private var cancelling = false
    private var lastPoint: CGPoint?
    private var pan: NSPanGestureRecognizer!
    private var press: NSPressGestureRecognizer!
    private var secondary: NSClickGestureRecognizer!
    private var layerTouches: LayerTouchRecognizer?
    private var timer: Timer?
    private var lastTrack: TimeInterval?
    private var observers: [NSObjectProtocol] = []
    private lazy var bubbles = BubbleInputCapture(marker: self)
    override var isFlipped: Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        pan = NSPanGestureRecognizer(target: self, action: #selector(panned)); pan.delegate = self
        press = NSPressGestureRecognizer(target: self, action: #selector(pressed)); press.delegate = self
        secondary = NSClickGestureRecognizer(target: self, action: #selector(context)); secondary.buttonMask = 2; secondary.delegate = self
        // AppKit supplies the user's hold duration/slop. Delaying primary events
        // lets a failed short gesture retain its click and a recognized drag own it.
        pan.delaysPrimaryMouseButtonEvents = true; press.delaysPrimaryMouseButtonEvents = true
        for name in [NSWindow.didResignKeyNotification, NSApplication.didResignActiveNotification] {
            observers.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] event in
                MainActor.assumeIsolated {
                    guard let self, !(event.object is NSWindow) || event.object as? NSWindow === self.window else { return }
                    self.cancel()
                }
            })
        }
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    deinit {
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
    }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        attach()
    }
    private func attach() {
        let next = window?.contentView
        guard next !== attached else { return }
        detach(); attached = next
        if let layer = model?.layerTouchModel {
            let recognizer = LayerTouchRecognizer(marker: self, model: layer)
            layerTouches = recognizer; attached?.addGestureRecognizer(recognizer)
        }
        for gesture in [pan!, press!, secondary!] { attached?.addGestureRecognizer(gesture) }
        bubbles.attach()
        updateViewport()
    }
    override func layout() { super.layout(); attach(); updateViewport() }
    func detach() {
        guard attached != nil else { return }
        bubbles.detach(); cancel(detaching: true); for gesture in [pan!, press!, secondary!] { attached?.removeGestureRecognizer(gesture) }
        if let layerTouches { layerTouches.isEnabled = false; attached?.removeGestureRecognizer(layerTouches); self.layerTouches = nil }
        attached = nil
    }
    func validate() {
        attach(); updateViewport()
        if layerTouches == nil, let layer = model?.layerTouchModel, attached != nil {
            let recognizer = LayerTouchRecognizer(marker: self, model: layer)
            layerTouches = recognizer; attached?.addGestureRecognizer(recognizer)
        }
        bubbles.attach()
    }
    private func cancel(detaching: Bool = false, all: Bool = true) {
        guard !cancelling else { return }; cancelling = true
        defer { cancelling = false }
        if all, let layerTouches { layerTouches.isEnabled = false; layerTouches.isEnabled = true }
        if detaching { model?.nativeInputDetached() } else if all { model?.cancel() } else { model?.cancelReorder() }
        timer?.invalidate(); timer = nil; downEvent = nil
        pan.isEnabled = false; press.isEnabled = false; pan.isEnabled = true; press.isEnabled = true
    }
    private func scrollAt(_ point: CGPoint) -> NSScrollView? {
        guard let attached else { return nil }
        return attached.hitTest(convert(point, to: attached.superview))?.enclosingScrollView
    }
    fileprivate func layerTouchPoint(_ touch: NSTouch, event: NSEvent, origin: CGPoint?, anchor: CGPoint?) -> (CGPoint, UInt64)? {
        guard let model = model?.layerTouchModel else { return nil }
        updateViewport()
        let cursor = convert(event.locationInWindow, from: nil)
        let point = CGPoint(x: touch.normalizedPosition.x * touch.deviceSize.width,
            y: (1 - touch.normalizedPosition.y) * touch.deviceSize.height)
        guard let origin, let anchor else { return model.touchRow(at: cursor).map { (cursor, $0) } }
        let pointsPerMillimeter: CGFloat = 72 / 25.4
        let position = CGPoint(x: anchor.x + (point.x - origin.x) * pointsPerMillimeter,
            y: anchor.y + (point.y - origin.y) * pointsPerMillimeter)
        return (position, model.touchRow(at: position) ?? 0)
    }
    @discardableResult private func move(_ point: CGPoint, force: Bool = false) -> Bool {
        guard let model else { return false }
        if !force && model.contact.dragging && point == lastPoint { return true }
        lastPoint = point
        return model.contact.move(to: point)
    }
    private func updateViewport() {
        model?.viewport = enclosingScrollView.map { convert($0.contentView.bounds, from: $0.contentView) } ?? bounds
    }
    func gestureRecognizer(_ recognizer: NSGestureRecognizer, shouldAttemptToRecognizeWith event: NSEvent) -> Bool {
        guard let model else { return false }
        updateViewport()
        let point = convert(event.locationInWindow, from: nil)
        // The marker remains mounted when another panel covers its rows.
        // Only the scroll view actually under the contact may admit pickup.
        if let scroll = enclosingScrollView, scrollAt(point) !== scroll { return false }
        if recognizer === secondary {
            return event.type == .rightMouseDown && model.acceptsContext(at: point)
        }
        guard event.type == .leftMouseDown else { return false }
        // Pan and press share this mouse-down object. Injected native events
        // can reuse eventNumber across different contacts.
        if downEvent !== event {
            guard let target = model.source(at: point) else {
                return false
            }
            let device: ReorderDevice = event.subtype == .tabletPoint ? .pen : .mouse
            model.contact.prepare(target, device: device, origin: point); downEvent = event
            lastPoint = nil
        }
        return recognizer !== press || model.contact.requiresHold || model.contact.device != .mouse
    }
    func gestureRecognizerShouldBegin(_ recognizer: NSGestureRecognizer) -> Bool {
        if recognizer === secondary { return model?.enabled == true }
        guard let model, model.contact.validate() else { return false }
        if recognizer === pan && model.contact.requiresHold && !model.contact.held {
            let point = NSApp.currentEvent.map { convert($0.locationInWindow, from: nil) } ?? recognizer.location(in: self)
            if model.beginSwipe(at: point) { return true }
            model.contact.cancel(); return false
        }
        return true
    }
    func precedesScrolling(_ recognizer: NSGestureRecognizer) -> Bool {
        recognizer === pan && model?.contact.target != nil
    }
    func gestureRecognizer(_ recognizer: NSGestureRecognizer, shouldRecognizeSimultaneouslyWith other: NSGestureRecognizer) -> Bool {
        // SwiftUI's button recognizer begins on mouse-down. Keep the retained
        // row contact alive beside it; consumeClick suppresses activation after
        // pickup. Options/text editing never admit a reorder contact.
        (recognizer === pan || recognizer === press) && model?.contact.target != nil
    }
    @objc private func context(_ recognizer: NSClickGestureRecognizer) {
        model?.context(at: recognizer.location(in: self))
    }
    @objc private func pressed(_ recognizer: NSPressGestureRecognizer) {
        switch recognizer.state {
        case .began: model?.recognizeHold()
        case .ended: finish()
        case .cancelled: if model?.swiping != true { cancel(all: false) }
        default: break
        }
    }
    @objc private func panned(_ recognizer: NSPanGestureRecognizer) {
        switch recognizer.state {
        case .began, .changed:
            if model?.swiping == true { model?.moveSwipe(to: recognizer.location(in: self)); return }
            if move(recognizer.location(in: self)) && timer == nil {
                lastTrack = nil
                let timer = Timer(timeInterval: 1.0 / 60, repeats: true) { [weak self] _ in MainActor.assumeIsolated { self?.track() } }
                RunLoop.main.add(timer, forMode: .common); self.timer = timer
            }
        case .ended: finish()
        case .cancelled: cancel(all: false)
        default: break
        }
    }
    private func finish() {
        model?.finishSwipe(cancelled: false)
        model?.contact.release(at: pan.location(in: self)); downEvent = nil
        timer?.invalidate(); timer = nil
    }
    private func track() {
        guard let model, model.contact.dragging else { timer?.invalidate(); timer = nil; return }
        updateViewport()
        let point = pan.location(in: self)
        var scrolled = false
        if let scroll = enclosingScrollView ?? scrollAt(point) {
            let viewport = convert(scroll.contentView.bounds, from: scroll.contentView)
            let delta: CGFloat
            if let edge = model.edgeScroll {
                let now = ProcessInfo.processInfo.systemUptime
                delta = edge.delta(at: point, in: viewport, elapsed: lastTrack.map { now - $0 } ?? 0)
                lastTrack = now
            } else {
                guard viewport.contains(point) else { _ = move(point); return }
                delta = point.y < viewport.minY + 28 ? -8 : point.y > viewport.maxY - 28 ? 8 : 0
            }
            if delta != 0 {
                let clip = scroll.contentView
                let limit = max(0, (scroll.documentView?.bounds.height ?? 0) - clip.bounds.height)
                let previous = clip.bounds.origin
                clip.scroll(to: CGPoint(x: clip.bounds.minX, y: max(0, min(limit, clip.bounds.minY + delta))))
                scrolled = previous != clip.bounds.origin
                scroll.reflectScrolledClipView(clip); updateViewport()
            }
        }
        _ = move(pan.location(in: self), force: scrolled)
    }
}

private final class LayerTouchRecognizer: NSGestureRecognizer, NSGestureRecognizerDelegate {
    private weak var marker: ReorderInputView?
    private weak var model: (any NativeLayerTouchModel)?
    private struct Contact { let identity: NSCopying & NSObjectProtocol; let id: UInt64; let row: UInt64; var point: CGPoint }
    private var contacts: [Contact] = []
    private var nextID: UInt64 = 0
    private var origin: CGPoint?
    private var anchor: CGPoint?
    init(marker: ReorderInputView, model: any NativeLayerTouchModel) {
        self.marker = marker; self.model = model
        super.init(target: nil, action: nil)
        allowedTouchTypes = .indirect; delegate = self
    }
    private func send(_ contact: Contact, phase: String, timestamp: TimeInterval) {
        model?.layerTouch(contact.id, phase: phase, point: contact.point, row: contact.row,
            timestamp: timestamp, scale: marker?.window?.backingScaleFactor ?? 1)
    }
    private func route(_ event: NSEvent, matching phase: NSTouch.Phase, name: String) {
        for touch in event.touches(matching: phase, in: view) where touch.type == .indirect {
            if phase == .began, let (point, row) = marker?.layerTouchPoint(touch, event: event, origin: origin, anchor: anchor), row != 0 {
                if origin == nil {
                    origin = CGPoint(x: touch.normalizedPosition.x * touch.deviceSize.width,
                        y: (1 - touch.normalizedPosition.y) * touch.deviceSize.height)
                    anchor = point
                }
                nextID &+= 1; contacts.append(Contact(identity: touch.identity, id: nextID, row: row, point: point))
            }
            guard let index = contacts.firstIndex(where: { $0.identity.isEqual(touch.identity) }) else { continue }
            if let (point, _) = marker?.layerTouchPoint(touch, event: event, origin: origin, anchor: anchor) { contacts[index].point = point }
            send(contacts[index], phase: name, timestamp: event.timestamp)
            if phase == .ended || phase == .cancelled { contacts.remove(at: index) }
        }
        if contacts.count >= 2 && state == .possible { model?.cancel(); state = .began }
        else if contacts.isEmpty { state = state == .began || state == .changed ? .ended : .failed }
        else if state == .began { state = .changed }
    }
    override func touchesBegan(with event: NSEvent) { route(event, matching: .began, name: "down") }
    override func touchesMoved(with event: NSEvent) { route(event, matching: .moved, name: "move") }
    override func touchesEnded(with event: NSEvent) { route(event, matching: .ended, name: "up") }
    override func touchesCancelled(with event: NSEvent) {
        for contact in contacts { send(contact, phase: "cancel", timestamp: event.timestamp) }
        contacts.removeAll(); state = .cancelled
    }
    override func reset() {
        for contact in contacts { send(contact, phase: "cancel", timestamp: ProcessInfo.processInfo.systemUptime) }
        contacts.removeAll(); origin = nil; anchor = nil; super.reset()
    }
    func gestureRecognizer(_ recognizer: NSGestureRecognizer, shouldRecognizeSimultaneouslyWith other: NSGestureRecognizer) -> Bool {
        !(other is NSClickGestureRecognizer) && !(other is NSPressGestureRecognizer)
    }
}
