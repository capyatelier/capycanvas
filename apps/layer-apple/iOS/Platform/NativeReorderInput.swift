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
final class ReorderInputView: UIView, UIGestureRecognizerDelegate, UIPointerInteractionDelegate {
    weak var model: (any NativeReorderModel)?
    private weak var attached: UIWindow?
    private weak var touch: UITouch?
    private var touchStart: TimeInterval = -1
    private var cancelling = false
    private weak var contactScroll: UIScrollView?
    private var lastPoint: CGPoint?
    private var pan: UIPanGestureRecognizer!
    private var windowDrag: UIWindowSceneDragInteraction?
    private var press: UILongPressGestureRecognizer!
    private var secondary: UITapGestureRecognizer!
    private var edgeDoublePress: UITapGestureRecognizer?
    private var edgeDoubleDevice = ReorderDevice.touch
    private var layerTouches: LayerTouchRecognizer?
    private var link: CADisplayLink?
    private var lastTrack: CFTimeInterval?
    private var observer: NSObjectProtocol?
    private var bubbles: BubbleRecognizer?
    private var edge: EdgeReachRecognizer?
    private var edgePointer: UIPointerInteraction?
    private var edgeHover: UIHoverGestureRecognizer?
    private let edgeCursorLayer = CAShapeLayer()
    private var activeEdgeCursor: String?
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
        attachEdges(); attachBubbles(); attachWindowDrag()
        updateViewport()
    }
    override func layoutSubviews() { super.layoutSubviews(); validate() }
    func detach() {
        guard attached != nil else { return }
        if let windowDrag { windowDrag.view?.removeInteraction(windowDrag); self.windowDrag = nil }
        releaseKeyboard()
        handledKeys.removeAll()
        cancel(detaching: true); attached?.removeGestureRecognizer(pan); attached?.removeGestureRecognizer(press)
        attached?.removeGestureRecognizer(secondary); attached = nil
        if let layerTouches { layerTouches.isEnabled = false; layerTouches.view?.removeGestureRecognizer(layerTouches); self.layerTouches = nil }
        if let bubbles { bubbles.view?.removeGestureRecognizer(bubbles); self.bubbles = nil }
        if let edge { edge.cancel(); edge.view?.removeGestureRecognizer(edge); self.edge = nil }
        if let edgePointer { edgePointer.view?.removeInteraction(edgePointer); self.edgePointer = nil }
        if let edgeHover { edgeHover.view?.removeGestureRecognizer(edgeHover); self.edgeHover = nil }
        activeEdgeCursor = nil; edgeCursorLayer.removeFromSuperlayer()
        if let edgeDoublePress { edgeDoublePress.view?.removeGestureRecognizer(edgeDoublePress); self.edgeDoublePress = nil }
        model?.bubbleInput?.hover(nil)
        if let bubbleHover { bubbleHover.view?.removeGestureRecognizer(bubbleHover); self.bubbleHover = nil }
    }
    func validate() {
        if layerTouches == nil, let layer = model?.layerTouchModel, attached != nil {
            let recognizer = LayerTouchRecognizer(marker: self, model: layer)
            layerTouches = recognizer; attached?.addGestureRecognizer(recognizer)
        }
        updateViewport(); attachEdges(); attachBubbles(); attachWindowDrag()
        if isFirstResponder, model?.bubbleInput?.acceptsIntensityKeys != true { releaseKeyboard() }
    }
    private func attachWindowDrag() {
        guard let tabs = model as? DrawingTabInteraction, !tabs.vertical else {
            if let windowDrag { windowDrag.view?.removeInteraction(windowDrag); self.windowDrag = nil }
            return
        }
        guard windowDrag == nil, attached != nil, !bounds.isEmpty else { return }
        let center = CGPoint(x: bounds.midX, y: bounds.midY)
        var ancestor = superview
        while let host = ancestor {
            if host.hitTest(host.convert(center, from: self), with: nil) != nil {
                guard host.convert(host.bounds, to: self) == bounds else { return }
                let interaction = UIWindowSceneDragInteraction()
                interaction.gestureForFailureRelationships.require(toFail: pan)
                host.addInteraction(interaction); windowDrag = interaction
                return
            }
            ancestor = host.superview
        }
    }
    private func attachEdges() {
        guard edge == nil, let attached, let workspace = model as? WorkspaceReorderInteraction else { return }
        let edge = EdgeReachRecognizer(marker: self, interaction: workspace)
        self.edge = edge; attached.addGestureRecognizer(edge)
        let pointer = UIPointerInteraction(delegate: self)
        edgePointer = pointer; attached.addInteraction(pointer)
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(hoveredEdge))
        hover.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.pencil.rawValue)]
        edgeHover = hover; attached.addGestureRecognizer(hover)
        edgeCursorLayer.bounds = CGRect(x: -12, y: -12, width: 24, height: 24)
        edgeCursorLayer.zPosition = 1000
        attached.layer.addSublayer(edgeCursorLayer)
        edgeCursorLayer.isHidden = true
        let doublePress = UITapGestureRecognizer(target: self, action: #selector(doublePressed))
        doublePress.numberOfTapsRequired = 2; doublePress.delegate = self
        doublePress.allowedTouchTypes = [UITouch.TouchType.direct, .pencil, .indirectPointer].map { NSNumber(value: $0.rawValue) }
        edgeDoublePress = doublePress; attached.addGestureRecognizer(doublePress)
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
    func pointerInteraction(_ interaction: UIPointerInteraction, regionFor request: UIPointerRegionRequest,
        defaultRegion: UIPointerRegion) -> UIPointerRegion? {
        guard let window, let edge else { return nil }
        let point = convert(request.location, from: window)
        let cursor = activeEdgeCursor ?? edge.candidate(at: point, hitView: window.hitTest(request.location, with: nil),
            device: .mouse).map { $0.vertical ? "row-resize" : "col-resize" }
        guard let cursor else { return nil }
        return UIPointerRegion(rect: CGRect(origin: request.location, size: CGSize(width: 1, height: 1)), identifier: cursor)
    }
    func pointerInteraction(_ interaction: UIPointerInteraction, styleFor region: UIPointerRegion) -> UIPointerStyle? {
        guard let cursor = region.identifier?.base as? String else { return nil }
        return UIPointerStyle(shape: .path(EdgeResizeCursor.path(cursor)))
    }
    @objc private func hoveredEdge(_ recognizer: UIHoverGestureRecognizer) {
        guard activeEdgeCursor == nil, let window else { return }
        guard recognizer.state == .began || recognizer.state == .changed, let edge else {
            showEdgeCursor(nil, at: .zero); return
        }
        let point = recognizer.location(in: self)
        let hit = window.hitTest(convert(point, to: window), with: nil)
        let cursor = edge.candidate(at: point, hitView: hit, device: .pen).map {
            $0.vertical ? "row-resize" : "col-resize"
        }
        showEdgeCursor(cursor, at: point)
    }
    fileprivate func showEdgeCursor(_ cursor: String?, at point: CGPoint) {
        guard let window, let cursor else { edgeCursorLayer.isHidden = true; return }
        CATransaction.begin(); CATransaction.setDisableActions(true)
        edgeCursorLayer.path = EdgeResizeCursor.path(cursor).cgPath
        edgeCursorLayer.fillColor = UIColor.label.resolvedColor(with: window.traitCollection).cgColor
        edgeCursorLayer.position = convert(point, to: window)
        edgeCursorLayer.isHidden = false
        CATransaction.commit()
    }
    fileprivate func clearEdgeCursor() {
        activeEdgeCursor = nil; edgeCursorLayer.isHidden = true
        edgePointer?.invalidate()
    }
    private func cancel(detaching: Bool = false, all: Bool = true) {
        endKeyRepeat()
        guard !cancelling else { return }; cancelling = true
        defer { cancelling = false }
        if all, let layerTouches { layerTouches.isEnabled = false; layerTouches.isEnabled = true }
        if detaching { model?.nativeInputDetached() } else if all { model?.cancel() } else { model?.cancelReorder() }
        if all { bubbles?.cancel(); edge?.cancel() }
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
    fileprivate func scrollAt(_ point: CGPoint) -> UIScrollView? {
        var node = window?.hitTest(convert(point, to: window), with: nil)
        while let view = node {
            if let scroll = view as? UIScrollView { return scroll }
            node = view.superview
        }
        return nil
    }
    fileprivate func claimEdge(_ touch: UITouch, cursor: String) {
        activeEdgeCursor = cursor; edgePointer?.invalidate()
        if touch.type == .pencil { showEdgeCursor(cursor, at: touch.location(in: self)) }
        model?.contact.transfer()
        model?.bubbleInput?.cancel()
        bubbles?.cancel()
        if let layerTouches { layerTouches.isEnabled = false; layerTouches.isEnabled = true }
        var node = touch.view
        while let view = node {
            if let canvas = view as? CanvasView { canvas.cancelEdgeContact(touch); break }
            node = view.superview
        }
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
        if gestureRecognizer === edgeDoublePress {
            edgeDoubleDevice = incoming.type == .pencil ? .pen : incoming.type == .indirectPointer ? .mouse : .touch
            let point = incoming.location(in: self)
            if let workspace = model as? WorkspaceReorderInteraction, workspace.isDividerHandle(at: point) {
                return workspace.edgeReach?.admission(at: point, device: edgeDoubleDevice, contact: .free) != nil
            }
            return edge?.candidate(for: incoming)?.contact == .free
        }
        if touch !== incoming || touchStart != incoming.timestamp {
            guard touch == nil || touch?.phase == .ended || touch?.phase == .cancelled || model.contact.target == nil else { cancel(); return false }
            updateViewport()
            let point = incoming.location(in: self)
            guard let target = model.source(at: point) else { return false }
            let device: ReorderDevice = incoming.type == .pencil ? .pen : incoming.type == .indirectPointer ? .mouse : .touch
            model.contact.prepare(target, device: device, origin: point,
                activate: edge?.candidate(for: incoming) == nil)
            touch = incoming; touchStart = incoming.timestamp
            contactScroll = scroll ?? scrollAt(point); lastPoint = nil
        }
        return gestureRecognizer !== press || model.contact.requiresHold || model.contact.device != .mouse
    }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldReceive event: UIEvent) -> Bool {
        return recognizer === secondary ? event.buttonMask.contains(.secondary) : !event.buttonMask.contains(.secondary)
    }
    override func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
        if recognizer === secondary { return model?.enabled == true }
        if recognizer === edgeDoublePress { return model?.enabled == true }
        guard let model, model.contact.validate() else { return false }
        if recognizer === pan && model.contact.requiresHold && !model.contact.held {
            if model.beginSwipe(at: touch?.location(in: self) ?? recognizer.location(in: self)) { return true }
            model.contact.cancel(); return false // The scroll view can keep this contact.
        }
        return true
    }
    @objc private func context(_ recognizer: UITapGestureRecognizer) { model?.context(at: recognizer.location(in: self)) }
    @objc private func doublePressed(_ recognizer: UITapGestureRecognizer) {
        guard recognizer.state == .ended, let workspace = model as? WorkspaceReorderInteraction,
            workspace.enabled, let edgeReach = workspace.edgeReach else { return }
        let point = recognizer.location(in: self)
        guard workspace.isDividerHandle(at: point)
            || (workspace.edgeReachSource(at: point, device: edgeDoubleDevice)?.contact ?? .free) == .free else { return }
        edgeReach.doublePress(device: edgeDoubleDevice, contact: .free, at: point)
    }
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
        return recognizer === pan && other === contactScroll?.panGestureRecognizer
            && model?.contact.target != nil && (model?.contact.requiresHold == false || model?.contact.held == true
                || model?.contact.target?.surface == .row)
    }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldRequireFailureOf other: UIGestureRecognizer) -> Bool {
        recognizer === pan && other === edge
    }
    @objc private func pressed(_ recognizer: UILongPressGestureRecognizer) {
        switch recognizer.state {
        case .began: edge?.cancel(activate: true); model?.recognizeHold()
        case .ended: finish()
        case .cancelled: if model?.swiping != true { cancel(all: false) }
        default: break
        }
    }
    @objc private func panned(_ recognizer: UIPanGestureRecognizer) {
        switch recognizer.state {
        case .began, .changed:
            if model?.swiping == true { edge?.cancel(activate: true); model?.moveSwipe(to: recognizer.location(in: self)); return }
            if moveReorder(recognizer.location(in: self)) {
                edge?.cancel()
                guard link == nil else { return }
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

private enum EdgeResizeCursor {
    static func path(_ name: String) -> UIBezierPath {
        let path = UIBezierPath(rect: CGRect(x: -6, y: -1, width: 12, height: 2))
        path.move(to: CGPoint(x: -10, y: 0))
        path.addLine(to: CGPoint(x: -4, y: -4))
        path.addLine(to: CGPoint(x: -4, y: 4))
        path.close()
        path.move(to: CGPoint(x: 10, y: 0))
        path.addLine(to: CGPoint(x: 4, y: -4))
        path.addLine(to: CGPoint(x: 4, y: 4))
        path.close()
        let angle: CGFloat = switch name {
        case "row-resize": .pi / 2
        case "nwse-resize": .pi / 4
        case "nesw-resize": -.pi / 4
        default: 0
        }
        path.apply(CGAffineTransform(rotationAngle: angle))
        return path
    }
}

private struct EdgeReachCandidate {
    let contact: EdgeReachContact
    let select: (() -> Void)?
    let admission: EdgeReachAdmission
    var vertical: Bool { admission.vertical }
}

final class EdgeReachRecognizer: UIGestureRecognizer {
    private weak var marker: ReorderInputView?
    private weak var interaction: WorkspaceReorderInteraction?
    private struct Contact {
        let touch: UITouch
        let id: UInt64
        let device: ReorderDevice
        let source: EdgeReachCandidate
        let origin: CGPoint
        var claimed = false
    }
    private var contact: Contact?
    private var nextID: UInt64 = 0
    func defersMotion(for touch: UITouch) -> Bool {
        guard let contact, contact.touch === touch, !contact.claimed, contact.source.vertical,
            let marker, let state = interaction?.edgeReach?.state(for: contact.id), state.pending && state.hold else { return false }
        let point = touch.location(in: marker)
        return hypot(point.x - contact.origin.x, point.y - contact.origin.y) > 10
    }
    init(marker: ReorderInputView, interaction: WorkspaceReorderInteraction) {
        self.marker = marker; self.interaction = interaction
        super.init(target: nil, action: nil)
        allowedTouchTypes = [UITouch.TouchType.direct, .pencil, .indirectPointer].map { NSNumber(value: $0.rawValue) }
        cancelsTouchesInView = true; delaysTouchesBegan = false; delaysTouchesEnded = false
    }
    fileprivate func candidate(for touch: UITouch) -> EdgeReachCandidate? {
        guard let marker else { return nil }
        let device: ReorderDevice = touch.type == .pencil ? .pen : touch.type == .indirectPointer ? .mouse : .touch
        return candidate(at: touch.location(in: marker), hitView: touch.view, device: device)
    }
    fileprivate func candidate(at point: CGPoint, hitView: UIView?, device: ReorderDevice) -> EdgeReachCandidate? {
        guard let marker, let interaction, let edgeReach = interaction.edgeReach else { return nil }
        guard marker.bounds.contains(point), interaction.enabled else { return nil }
        var node = hitView
        var canvas = false
        var nativeInput = false
        var textInput = false
        var horizontalScroll = false
        while let view = node {
            if view is CanvasView { canvas = true }
            if view is UITextInput { textInput = true }
            if view is UIControl || view is UITextInput || view is ControlInput.InputView
                || view is ColorWheelInput.ContactView || view is ParameterInput.ContactView
                || view.accessibilityTraits.contains(.button)
                || view.accessibilityTraits.contains(.adjustable) { nativeInput = true }
            if let scroll = view as? UIScrollView, scroll.contentSize.width > scroll.bounds.width + 1 {
                horizontalScroll = true
            }
            node = view.superview
        }
        if canvas && device != .touch { return nil }
        if marker.model?.bubbleInput?.hit(point) == true { return nil }
        let source = interaction.edgeReachSource(at: point, device: device)
        let contact: EdgeReachContact
        if let source {
            contact = textInput || source.contact == .free && nativeInput ? .input
                : source.contact == .free && horizontalScroll ? .pickup : source.contact
        } else { contact = nativeInput ? .input : horizontalScroll ? .pickup : .free }
        guard let admission = edgeReach.admission(at: point, device: device, contact: contact) else { return nil }
        return EdgeReachCandidate(contact: contact, select: source?.select, admission: admission)
    }
    override func canPrevent(_ preventedGestureRecognizer: UIGestureRecognizer) -> Bool { contact?.claimed == true }
    override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool { false }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        guard contact == nil, touches.count == 1, let touch = touches.first, let marker, let interaction,
            let edgeReach = interaction.edgeReach, let source = candidate(for: touch) else {
            cancel(); return
        }
        nextID &+= 1
        let id = nextID
        let point = touch.location(in: marker)
        contact = Contact(touch: touch, id: id, device: touch.type == .pencil ? .pen : touch.type == .indirectPointer ? .mouse : .touch,
            source: source, origin: point)
        if source.vertical { marker.scrollAt(point)?.panGestureRecognizer.require(toFail: self) }
        let state = edgeReach.begin(id: id, admission: source.admission, device: contact!.device,
            contact: source.contact, at: point,
            timeNs: UInt64(touch.timestamp * 1_000_000_000), select: source.select,
            update: { [weak self] state in
                guard let self, self.contact?.id == id, self.contact?.claimed != true else { return }
                if state == nil || (state?.pending == false && state?.claimed == false) {
                    self.marker?.model?.contact.activate()
                    self.state = .failed
                }
            }, claim: { [weak self, weak marker] cursor in
                guard let self, let marker, var current = self.contact, current.id == id, !current.claimed else { return }
                current.claimed = true; self.contact = current
                marker.claimEdge(touch, cursor: cursor)
                self.state = self.state == .possible ? .began : .changed
            })
        if !state.pending && !state.claimed { marker.model?.contact.activate(); self.state = .failed }
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let contact, touches.contains(contact.touch), let marker, let edgeReach = interaction?.edgeReach else { return }
        let point = contact.touch.location(in: marker)
        let state = edgeReach.move(id: contact.id, to: point, timeNs: UInt64(contact.touch.timestamp * 1_000_000_000))
        if contact.claimed {
            if contact.device == .pen, let cursor = state?.cursor { marker.showEdgeCursor(cursor, at: point) }
            self.state = .changed
        }
        else if state == nil || (state?.pending == false && state?.claimed == false) {
            marker.model?.contact.activate()
            self.state = .failed
        }
    }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let contact, touches.contains(contact.touch), let marker else { return }
        self.contact = nil
        interaction?.edgeReach?.end(id: contact.id, at: contact.touch.location(in: marker),
            timeNs: UInt64(contact.touch.timestamp * 1_000_000_000), cancelled: false)
        if contact.claimed { marker.clearEdgeCursor() }
        state = contact.claimed ? .ended : .failed
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { cancel() }
    fileprivate func cancel(activate: Bool = false) {
        let previous = contact
        contact = nil
        if let previous {
            interaction?.edgeReach?.cancel(id: previous.id)
            if activate && !previous.claimed { marker?.model?.contact.activate() }
            if previous.claimed { marker?.clearEdgeCursor() }
        }
        if state == .began || state == .changed { state = .cancelled }
        else if state == .possible { state = .failed }
    }
    override func reset() { cancel(); super.reset() }
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
