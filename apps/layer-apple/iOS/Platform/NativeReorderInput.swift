import SwiftUI
import UIKit

struct NativeReorderInput: UIViewRepresentable {
    let model: WorkspaceRowInteraction
    func makeUIView(context: Context) -> ReorderInputView { ReorderInputView() }
    func updateUIView(_ view: ReorderInputView, context: Context) { view.model = model; view.validate() }
    static func dismantleUIView(_ view: ReorderInputView, coordinator: ()) {
        let model = view.model; view.model = nil; view.detach(); model?.nativeInputDetached()
    }
}

/// Recognizers stay on the containing window while SwiftUI rows scroll/update.
/// The transparent marker supplies the list's local coordinate system only.
final class ReorderInputView: UIView, UIGestureRecognizerDelegate {
    weak var model: WorkspaceRowInteraction?
    private weak var attached: UIWindow?
    private weak var touch: UITouch?
    private var touchStart: TimeInterval = -1
    private var cancelling = false
    private var pan: UIPanGestureRecognizer!
    private var press: UILongPressGestureRecognizer!
    private var secondary: UITapGestureRecognizer!
    private var link: CADisplayLink?
    private var observer: NSObjectProtocol?
    override init(frame: CGRect) {
        super.init(frame: frame); isUserInteractionEnabled = false
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
        observer = NotificationCenter.default.addObserver(forName: UIApplication.willResignActiveNotification,
            object: nil, queue: .main) { [weak self] _ in MainActor.assumeIsolated { self?.cancel() } }
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    deinit { if let observer { NotificationCenter.default.removeObserver(observer) } }
    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard window !== attached else { return }
        detach(); attached = window
        attached?.addGestureRecognizer(pan); attached?.addGestureRecognizer(press); attached?.addGestureRecognizer(secondary)
        updateViewport()
    }
    override func layoutSubviews() { super.layoutSubviews(); updateViewport() }
    func detach() {
        cancel(); attached?.removeGestureRecognizer(pan); attached?.removeGestureRecognizer(press)
        attached?.removeGestureRecognizer(secondary); attached = nil
    }
    func validate() {
        updateViewport()
    }
    private func cancel() {
        guard !cancelling else { return }; cancelling = true
        defer { cancelling = false }
        model?.cancel(); touch = nil; link?.invalidate(); link = nil
        pan.isEnabled = false; press.isEnabled = false
        pan.isEnabled = true; press.isEnabled = true
    }
    private var scroll: UIScrollView? {
        var node = superview
        while let view = node { if let scroll = view as? UIScrollView { return scroll }; node = view.superview }
        return nil
    }
    private func updateViewport() {
        model?.viewport = scroll.map { convert($0.bounds, from: $0) } ?? bounds
    }
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive incoming: UITouch) -> Bool {
        guard let model, incoming.type == .direct || incoming.type == .pencil || incoming.type == .indirectPointer else { return false }
        if gestureRecognizer === secondary {
            updateViewport()
            let point = incoming.location(in: self)
            return model.enabled && model.viewport.contains(point) && model.frames.values.contains(where: { $0.row.contains(point) })
        }
        if touch !== incoming || touchStart != incoming.timestamp {
            guard touch == nil || touch?.phase == .ended || touch?.phase == .cancelled || model.contact.target == nil else { cancel(); return false }
            updateViewport()
            let point = incoming.location(in: self)
            guard let target = model.source(at: point) else { return false }
            let device: ReorderDevice = incoming.type == .pencil ? .pen : incoming.type == .indirectPointer ? .mouse : .touch
            model.contact.prepare(target, device: device, origin: point); touch = incoming; touchStart = incoming.timestamp
        }
        return gestureRecognizer !== press || model.contact.requiresHold || model.contact.device != .mouse
    }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldReceive event: UIEvent) -> Bool {
        recognizer === secondary ? event.buttonMask.contains(.secondary) : !event.buttonMask.contains(.secondary)
    }
    override func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
        if recognizer === secondary { return model?.enabled == true }
        guard let model, model.contact.validate() else { return false }
        if recognizer === pan && model.contact.requiresHold && !model.contact.held {
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
        recognizer === pan && other === scroll?.panGestureRecognizer
            && model?.contact.target != nil && (model?.contact.requiresHold == false || model?.contact.held == true)
    }
    @objc private func pressed(_ recognizer: UILongPressGestureRecognizer) {
        switch recognizer.state {
        case .began: model?.contact.recognizeHold()
        case .ended: finish()
        case .cancelled: cancel()
        default: break
        }
    }
    @objc private func panned(_ recognizer: UIPanGestureRecognizer) {
        switch recognizer.state {
        case .began, .changed:
            if model?.contact.move(to: recognizer.location(in: self)) == true && link == nil {
                let link = CADisplayLink(target: self, selector: #selector(track)); link.add(to: .main, forMode: .common); self.link = link
            }
        case .ended: finish()
        case .cancelled: cancel()
        default: break
        }
    }
    private func finish() {
        model?.contact.release(at: pan.location(in: self)); touch = nil
        link?.invalidate(); link = nil
    }
    @objc private func track() {
        guard let model, model.contact.dragging else { link?.invalidate(); link = nil; return }
        updateViewport()
        let point = pan.location(in: self)
        if let scroll, model.viewport.contains(point) {
            let delta: CGFloat = point.y < model.viewport.minY + 28 ? -8 : point.y > model.viewport.maxY - 28 ? 8 : 0
            if delta != 0 {
                let top = -scroll.adjustedContentInset.top
                let bottom = max(top, scroll.contentSize.height - scroll.bounds.height + scroll.adjustedContentInset.bottom)
                scroll.setContentOffset(CGPoint(x: scroll.contentOffset.x, y: max(top, min(bottom, scroll.contentOffset.y + delta))), animated: false)
                updateViewport()
            }
        }
        _ = model.contact.move(to: pan.location(in: self))
    }
}
