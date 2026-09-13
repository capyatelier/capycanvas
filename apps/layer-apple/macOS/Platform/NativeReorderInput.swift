import SwiftUI
import AppKit

struct NativeReorderInput: NSViewRepresentable {
    let model: any NativeReorderModel
    func makeNSView(context: Context) -> ReorderInputView { ReorderInputView() }
    func updateNSView(_ view: ReorderInputView, context: Context) { view.model = model; view.validate() }
    static func dismantleNSView(_ view: ReorderInputView, coordinator: ()) {
        let model = view.model; view.model = nil; view.detach(); model?.nativeInputDetached()
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
    private var timer: Timer?
    private var observers: [NSObjectProtocol] = []
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
        for gesture in [pan!, press!, secondary!] { attached?.addGestureRecognizer(gesture) }
        updateViewport()
    }
    override func layout() { super.layout(); attach(); updateViewport() }
    func detach() {
        cancel(); for gesture in [pan!, press!, secondary!] { attached?.removeGestureRecognizer(gesture) }; attached = nil
    }
    func validate() {
        attach(); updateViewport()
    }
    private func cancel() {
        guard !cancelling else { return }; cancelling = true
        defer { cancelling = false }
        model?.cancel(); timer?.invalidate(); timer = nil; downEvent = nil
        pan.isEnabled = false; press.isEnabled = false; pan.isEnabled = true; press.isEnabled = true
    }
    private func scrollAt(_ point: CGPoint) -> NSScrollView? {
        guard let attached else { return nil }
        return attached.hitTest(convert(point, to: attached.superview))?.enclosingScrollView
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
        if recognizer === pan && model.contact.requiresHold && !model.contact.held { model.contact.cancel(); return false }
        return true
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
        case .cancelled: cancel()
        default: break
        }
    }
    @objc private func panned(_ recognizer: NSPanGestureRecognizer) {
        switch recognizer.state {
        case .began, .changed:
            if move(recognizer.location(in: self)) && timer == nil {
                let timer = Timer(timeInterval: 1.0 / 60, repeats: true) { [weak self] _ in MainActor.assumeIsolated { self?.track() } }
                RunLoop.main.add(timer, forMode: .common); self.timer = timer
            }
        case .ended: finish()
        case .cancelled: cancel()
        default: break
        }
    }
    private func finish() {
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
            guard viewport.contains(point) else { _ = move(point); return }
            let delta: CGFloat = point.y < viewport.minY + 28 ? -8 : point.y > viewport.maxY - 28 ? 8 : 0
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
