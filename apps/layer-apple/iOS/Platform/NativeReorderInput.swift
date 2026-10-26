import SwiftUI
import UIKit
import UIKit.UIGestureRecognizerSubclass

struct NativeReorderInput: View {
    let model: any NativeReorderModel
    var body: some View { ReorderInputHost(model: model).allowsHitTesting(false) }
}

private struct ReorderInputHost: UIViewRepresentable {
    let model: any NativeReorderModel
    func makeUIView(context: Context) -> ReorderInputView { ReorderInputView() }
    func updateUIView(_ view: ReorderInputView, context: Context) { view.model = model; view.validate() }
    static func dismantleUIView(_ view: ReorderInputView, coordinator: ()) {
        view.detach(); view.model = nil
    }
}

/// Window recognizers share this marker's coordinates and control-key focus.
final class ReorderInputView: UIView, UIGestureRecognizerDelegate {
    weak var model: (any NativeReorderModel)?
    private weak var attached: UIWindow?
    private weak var touch: UITouch?
    private var touchStart: TimeInterval = -1
    private var cancelling = false
    private weak var contactScroll: UIScrollView?
    private var lastPoint: CGPoint?
    private var pan: UIPanGestureRecognizer!
    private var press: UILongPressGestureRecognizer!
    private var secondary: UITapGestureRecognizer!
    private var layerTouches: LayerTouchRecognizer?
    private var link: CADisplayLink?
    private var lastTrack: CFTimeInterval?
    private var observer: NSObjectProtocol?
    private var bubbles: BubbleRecognizer?
    private var bubbleHover: UIHoverGestureRecognizer?
    private var handledKeys: Set<UIKeyboardHIDUsage> = []
    private var keyRepeat: KeyRepeat?
    override var canBecomeFirstResponder: Bool { true }
    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? { nil }
    override var keyCommands: [UIKeyCommand]? {
        guard isFirstResponder, model?.bubbleInput?.acceptsIntensityKeys == true else { return super.keyCommands }
        let home = UIKeyCommand(input: UIKeyCommand.inputHome, modifierFlags: [], action: #selector(resetIntensity))
        home.wantsPriorityOverSystemBehavior = true
        return (super.keyCommands ?? []) + [home]
    }
    @objc private func resetIntensity(_ command: UIKeyCommand) { _ = model?.bubbleInput?.intensityKey("home") }
    override init(frame: CGRect) {
        super.init(frame: frame)
        pan = UIPanGestureRecognizer(target: self, action: #selector(panned))
        pan.maximumNumberOfTouches = 1; pan.delegate = self
        press = UILongPressGestureRecognizer(target: self, action: #selector(pressed))
        press.delegate = self
        secondary = UITapGestureRecognizer(target: self, action: #selector(context))
        secondary.buttonMaskRequired = .secondary; secondary.delegate = self
        for recognizer in [pan!, press!] {
            recognizer.allowedTouchTypes = [UITouch.TouchType.direct, .pencil, .indirectPointer].map { NSNumber(value: $0.rawValue) }
        }
        // Use UIKit's native hold duration, movement slop and cancellation.
        observer = NotificationCenter.default.addObserver(forName: UIScene.willDeactivateNotification,
            object: nil, queue: .main) { [weak self] notification in MainActor.assumeIsolated {
                guard let self, let scene = self.window?.windowScene,
                    notification.object as? UIScene === scene else { return }
                self.cancel()
            } }
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    deinit { if let observer { NotificationCenter.default.removeObserver(observer) } }
    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard window !== attached else { return }
        detach(); attached = window
        if let layer = model?.layerTouchModel {
            let recognizer = LayerTouchRecognizer(marker: self, model: layer)
            layerTouches = recognizer; attached?.addGestureRecognizer(recognizer)
        }
        attached?.addGestureRecognizer(pan); attached?.addGestureRecognizer(press); attached?.addGestureRecognizer(secondary)
        attachBubbles()
        updateViewport()
    }
    override func layoutSubviews() { super.layoutSubviews(); validate() }
    func detach() {
        guard attached != nil else { return }
        releaseKeyboard()
        handledKeys.removeAll()
        cancel(detaching: true); attached?.removeGestureRecognizer(pan); attached?.removeGestureRecognizer(press)
        attached?.removeGestureRecognizer(secondary); attached = nil
        if let layerTouches { layerTouches.isEnabled = false; layerTouches.view?.removeGestureRecognizer(layerTouches); self.layerTouches = nil }
        if let bubbles { bubbles.view?.removeGestureRecognizer(bubbles); self.bubbles = nil }
        model?.bubbleInput?.hover(nil)
        if let bubbleHover { bubbleHover.view?.removeGestureRecognizer(bubbleHover); self.bubbleHover = nil }
    }
    func validate() {
        if layerTouches == nil, let layer = model?.layerTouchModel, attached != nil {
            let recognizer = LayerTouchRecognizer(marker: self, model: layer)
            layerTouches = recognizer; attached?.addGestureRecognizer(recognizer)
        }
        updateViewport(); attachBubbles()
        if isFirstResponder, model?.bubbleInput?.acceptsIntensityKeys != true { releaseKeyboard() }
    }
    func focusBubbleKeyboard() {
        if model?.bubbleInput?.acceptsIntensityKeys == true, !NativeTextContext.composing { becomeFirstResponder() }
        else if isFirstResponder { releaseKeyboard() }
    }
    override func resignFirstResponder() -> Bool { endKeyRepeat(); return super.resignFirstResponder() }
    private func endKeyRepeat() { keyRepeat?.timer.invalidate(); keyRepeat = nil }
    private func releaseKeyboard() {
        endKeyRepeat()
        guard isFirstResponder else { return }
        model?.bubbleInput?.releaseIntensityKeys()
        if isFirstResponder { _ = resignFirstResponder() }
    }
    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var unhandled = Set<UIPress>()
        for press in presses {
            guard window?.isKeyWindow == true, let key = press.key else { unhandled.insert(press); continue }
            let name = AppleKeyName.name(key), flags = key.modifierFlags
            let command = !flags.intersection([.command, .control]).isEmpty, alt = flags.contains(.alternate)
            guard model?.bubbleInput?.intensityKey(name, command: command, alt: alt) == true else { unhandled.insert(press); continue }
            handledKeys.insert(key.keyCode)
            endKeyRepeat()
            if key.keyCode == .keyboardLeftArrow || key.keyCode == .keyboardRightArrow
                || key.keyCode == .keyboardDownArrow || key.keyCode == .keyboardUpArrow {
                let timer = Timer(timeInterval: KeyRepeat.interval, repeats: true) { [weak self] _ in
                    MainActor.assumeIsolated {
                        guard let self, self.isFirstResponder, self.window?.isKeyWindow == true,
                            self.model?.bubbleInput?.intensityKey(name, command: command, alt: alt) == true else { self?.endKeyRepeat(); return }
                    }
                }
                timer.fireDate = Date(timeIntervalSinceNow: KeyRepeat.delay)
                RunLoop.main.add(timer, forMode: .common)
                keyRepeat = KeyRepeat(code: key.keyCode, timer: timer)
            }
        }
        if !unhandled.isEmpty {
            releaseKeyboard()
            if let target = FocusedResponder.find(), target !== self { target.pressesBegan(unhandled, with: event) }
            else { super.pressesBegan(unhandled, with: event) }
        }
    }
    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        let unhandled = Set(presses.filter { press in
            guard let code = press.key?.keyCode else { return true }
            if keyRepeat?.code == code { endKeyRepeat() }
            return handledKeys.remove(code) == nil
        })
        if !unhandled.isEmpty {
            if let target = FocusedResponder.find(), target !== self { target.pressesEnded(unhandled, with: event) }
            else { super.pressesEnded(unhandled, with: event) }
        }
    }
    override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) { pressesEnded(presses, with: event) }
    private func attachBubbles() {
        guard bubbles == nil, model?.bubbleInput != nil, let window else { return }
        let bubbles = BubbleRecognizer(marker: self); self.bubbles = bubbles; window.addGestureRecognizer(bubbles)
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(hoveredBubbles))
        hover.allowedTouchTypes = [UITouch.TouchType.pencil, .indirectPointer].map { NSNumber(value: $0.rawValue) }
        hover.name = "bubble-hover"
        bubbleHover = hover; window.addGestureRecognizer(hover)
    }
    @objc func hoveredBubbles(_ recognizer: UIHoverGestureRecognizer) {
        guard let input = model?.bubbleInput else { return }
        input.viewport = bounds.size
        let point = recognizer.location(in: self)
        input.hover(window != nil && input.enabled && bounds.contains(point)
            && (recognizer.state == .began || recognizer.state == .changed) ? point : nil)
    }
    private func cancel(detaching: Bool = false, all: Bool = true) {
        endKeyRepeat()
        guard !cancelling else { return }; cancelling = true
        defer { cancelling = false }
        if all, let layerTouches { layerTouches.isEnabled = false; layerTouches.isEnabled = true }
        if detaching { model?.nativeInputDetached() } else if all { model?.cancel() } else { model?.cancelReorder() }
        if all { bubbles?.cancel() }
        if all { model?.bubbleInput?.hover(nil) }
        touch = nil; link?.invalidate(); link = nil
        pan.isEnabled = false; press.isEnabled = false
        pan.isEnabled = true; press.isEnabled = true
    }
    private var scroll: UIScrollView? {
        var node = superview
        while let view = node { if let scroll = view as? UIScrollView { return scroll }; node = view.superview }
        return nil
    }
    fileprivate func layerTouchRow(_ incoming: UITouch) -> (CGPoint, UInt64)? {
        guard incoming.type == .direct, let model = model?.layerTouchModel,
              let scroll, incoming.view?.isDescendant(of: scroll) == true else { return nil }
        updateViewport()
        let point = incoming.location(in: self)
        return model.touchRow(at: point).map { (point, $0) }
    }
    private func scrollAt(_ point: CGPoint) -> UIScrollView? {
        var node = window?.hitTest(convert(point, to: window), with: nil)
        while let view = node {
            if let scroll = view as? UIScrollView { return scroll }
            node = view.superview
        }
        return nil
    }
    @discardableResult func moveReorder(_ point: CGPoint, force: Bool = false) -> Bool {
        guard let model else { return false }
        updateViewport()
        if !force && model.contact.dragging && point == lastPoint { return true }
        lastPoint = point
        return model.contact.move(to: point)
    }
    private func updateViewport() {
        model?.viewport = scroll.map { convert($0.bounds, from: $0) } ?? bounds
    }
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive incoming: UITouch) -> Bool {
        guard let model, incoming.type == .direct || incoming.type == .pencil || incoming.type == .indirectPointer else { return false }
        // Window-root recognition must not pick rows behind another panel.
        if let scroll, incoming.view?.isDescendant(of: scroll) != true { return false }
        if gestureRecognizer === secondary {
            updateViewport()
            let point = incoming.location(in: self)
            return model.acceptsContext(at: point)
        }
        if touch !== incoming || touchStart != incoming.timestamp {
            guard touch == nil || touch?.phase == .ended || touch?.phase == .cancelled || model.contact.target == nil else { cancel(); return false }
            updateViewport()
            let point = incoming.location(in: self)
            guard let target = model.source(at: point) else { return false }
            let device: ReorderDevice = incoming.type == .pencil ? .pen : incoming.type == .indirectPointer ? .mouse : .touch
            model.contact.prepare(target, device: device, origin: point); touch = incoming; touchStart = incoming.timestamp
            contactScroll = scroll ?? scrollAt(point); lastPoint = nil
        }
        return gestureRecognizer !== press || model.contact.requiresHold || model.contact.device != .mouse
    }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldReceive event: UIEvent) -> Bool {
        return recognizer === secondary ? event.buttonMask.contains(.secondary) : !event.buttonMask.contains(.secondary)
    }
    override func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
        if recognizer === secondary { return model?.enabled == true }
        guard let model, model.contact.validate() else { return false }
        if recognizer === pan && model.contact.requiresHold && !model.contact.held {
            if model.beginSwipe(at: touch?.location(in: self) ?? recognizer.location(in: self)) { return true }
            model.contact.cancel(); return false // The scroll view can keep this contact.
        }
        return true
    }
    @objc private func context(_ recognizer: UITapGestureRecognizer) { model?.context(at: recognizer.location(in: self)) }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
        if (recognizer === pan && other === press) || (recognizer === press && other === pan) { return true }
        // Retain pickup beside SwiftUI's early button recognition, while native
        // scrolling and other manipulation recognizers keep exclusive ownership.
        guard !(other is UIPanGestureRecognizer), !(other is UIPinchGestureRecognizer) else { return false }
        return (recognizer === pan || recognizer === press) && model?.contact.target != nil
    }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldBeRequiredToFailBy other: UIGestureRecognizer) -> Bool {
        // A grip or mouse row gets the first chance at movement. Touch/pen row
        // bodies still fail early pans so the list can scroll before a hold.
        recognizer === pan && other === contactScroll?.panGestureRecognizer
            && model?.contact.target != nil && (model?.contact.requiresHold == false || model?.contact.held == true
                || model?.contact.target?.surface == .row)
    }
    @objc private func pressed(_ recognizer: UILongPressGestureRecognizer) {
        switch recognizer.state {
        case .began: model?.recognizeHold()
        case .ended: finish()
        case .cancelled: if model?.swiping != true { cancel(all: false) }
        default: break
        }
    }
    @objc private func panned(_ recognizer: UIPanGestureRecognizer) {
        switch recognizer.state {
        case .began, .changed:
            if model?.swiping == true { model?.moveSwipe(to: recognizer.location(in: self)); return }
            if moveReorder(recognizer.location(in: self)) && link == nil {
                lastTrack = nil
                let link = CADisplayLink(target: self, selector: #selector(track)); link.add(to: .main, forMode: .common); self.link = link
            }
        case .ended: finish()
        case .cancelled: cancel(all: false)
        default: break
        }
    }
    private func finish() {
        model?.finishSwipe(cancelled: false)
        model?.contact.release(at: pan.location(in: self)); touch = nil
        link?.invalidate(); link = nil
    }
    @objc private func track() {
        guard let model, model.contact.dragging else { link?.invalidate(); link = nil; return }
        trackReorder { pan.location(in: self) }
    }
    /// Grips and held row bodies use the same scrolling path.
    /// Read the contact again after scrolling: its content coordinate has moved.
    @discardableResult func trackReorder(location: () -> CGPoint) -> Bool {
        updateViewport()
        var scrolled = false
        if let (scroll, next) = reorderScrollTarget(at: location()) {
            scroll.setContentOffset(next, animated: false)
            scrolled = true
        }
        _ = moveReorder(location(), force: scrolled)
        return scrolled
    }
    func needsReorderScrolling(at point: CGPoint) -> Bool { reorderScrollTarget(at: point) != nil }
    private func reorderScrollTarget(at point: CGPoint) -> (UIScrollView, CGPoint)? {
        guard let scroll = scroll ?? scrollAt(point) else { return nil }
        let viewport = convert(scroll.bounds, from: scroll)
        let delta: CGFloat
        if let edge = model?.edgeScroll {
            let now = CACurrentMediaTime()
            delta = edge.delta(at: point, in: viewport, elapsed: lastTrack.map { now - $0 } ?? 0)
            lastTrack = now
        } else {
            guard viewport.contains(point) else { return nil }
            delta = point.y < viewport.minY + 28 ? -8 : point.y > viewport.maxY - 28 ? 8 : 0
        }
        guard delta != 0 else { return nil }
        let top = -scroll.adjustedContentInset.top
        let bottom = max(top, scroll.contentSize.height - scroll.bounds.height + scroll.adjustedContentInset.bottom)
        let next = CGPoint(x: scroll.contentOffset.x, y: max(top, min(bottom, scroll.contentOffset.y + delta)))
        return next == scroll.contentOffset ? nil : (scroll, next)
    }
}

private final class LayerTouchRecognizer: UIGestureRecognizer {
    private weak var marker: ReorderInputView?
    private weak var model: (any NativeLayerTouchModel)?
    private struct Contact { let touch: UITouch; let id: UInt64; let row: UInt64 }
    private var contacts: [ObjectIdentifier: Contact] = [:]
    private var nextID: UInt64 = 0
    init(marker: ReorderInputView, model: any NativeLayerTouchModel) {
        self.marker = marker; self.model = model
        super.init(target: nil, action: nil)
        allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]
        cancelsTouchesInView = true; delaysTouchesBegan = false; delaysTouchesEnded = false
    }
    private func send(_ contact: Contact, phase: String, timestamp: TimeInterval) {
        guard let marker else { return }
        model?.layerTouch(contact.id, phase: phase, point: contact.touch.location(in: marker), row: contact.row,
            timestamp: timestamp, scale: marker.window?.screen.scale ?? 1)
    }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        for touch in touches {
            guard let (_, row) = marker?.layerTouchRow(touch) else { continue }
            nextID &+= 1
            let contact = Contact(touch: touch, id: nextID, row: row)
            contacts[ObjectIdentifier(touch)] = contact; send(contact, phase: "down", timestamp: event.timestamp)
        }
        if contacts.count >= 2 && state == .possible { model?.cancel(); state = .began }
        else if contacts.isEmpty { state = .failed }
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        for touch in touches { if let contact = contacts[ObjectIdentifier(touch)] { send(contact, phase: "move", timestamp: event.timestamp) } }
        if state == .began || state == .changed { state = .changed }
    }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        let claimed = state == .began || state == .changed
        for touch in touches { if let contact = contacts.removeValue(forKey: ObjectIdentifier(touch)) { send(contact, phase: "up", timestamp: event.timestamp) } }
        if contacts.isEmpty { state = claimed ? .ended : .failed }
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        cancelContacts(); state = .cancelled
    }
    private func cancelContacts() {
        for contact in contacts.values { send(contact, phase: "cancel", timestamp: ProcessInfo.processInfo.systemUptime) }
        contacts.removeAll()
    }
    override func reset() { cancelContacts(); super.reset() }
    override func canPrevent(_ preventedGestureRecognizer: UIGestureRecognizer) -> Bool { contacts.count >= 2 }
    override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool { contacts.count < 2 }
}
