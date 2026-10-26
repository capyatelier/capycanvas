import SwiftUI

enum ParameterInputRegion: Equatable {
    case parameter(hdr: Bool), rectangle
    func hit(_ point: CGPoint, in bounds: CGRect) -> UInt32 {
        switch self {
        case .parameter(let hdr): return capy_apple_parameter_hit(Float(point.x), Float(point.y), Float(bounds.width), hdr)
        case .rectangle: return bounds.contains(point) ? 1 : 0
        }
    }
}

#if os(macOS)
import AppKit
struct ParameterInput: NSViewRepresentable {
    @Environment(\.isEnabled) private var enabled
    let region: ParameterInputRegion
    let identity: String
    var nudge: ((String, UInt32, [Double]) -> Void)? = nil
    var touches: ColorTouchSurface? = nil
    let event: (String, UInt32, CGPoint, CGFloat) -> Void
    func makeNSView(context: Context) -> ContactView { ContactView() }
    func updateNSView(_ view: ContactView, context: Context) {
        if view.identity != identity || view.region != region || !enabled { view.cancel() }
        view.identity = identity; view.region = region; view.enabled = enabled; view.event = event; view.nudge = nudge
    }
    static func dismantleNSView(_ view: ContactView, coordinator: ()) { view.cancel() }
    final class ContactView: NSView {
        var region = ParameterInputRegion.parameter(hdr: false), enabled = true
        var identity = "", part: UInt32 = 0
        var event: (String, UInt32, CGPoint, CGFloat) -> Void = { _, _, _, _ in }
        var point = CGPoint.zero
        var nudge: ((String, UInt32, [Double]) -> Void)?
        var selected: UInt32 = 1
        var keyActive = false
        override var isFlipped: Bool { true }
        override var acceptsFirstResponder: Bool { true }
        var size: CGFloat { bounds.width }
        func hit(_ p: CGPoint) -> UInt32 { enabled ? region.hit(p, in: bounds) : 0 }
        override func hitTest(_ p: NSPoint) -> NSView? { hit(convert(p, from: superview)) == 0 ? nil : self }
        override func mouseDown(with e: NSEvent) {
            point = convert(e.locationInWindow, from: nil); part = hit(point)
            if part != 0 { selected = part }
            let reset = e.clickCount == 2 && region != .rectangle
            if part != 0 { window?.makeFirstResponder(self); event(reset ? "reset" : "down", part, point, size) }
            if reset { part = 0 }
        }
        override func mouseDragged(with e: NSEvent) {
            point = convert(e.locationInWindow, from: nil)
            if part != 0 { event("move", part, point, size) }
        }
        override func mouseUp(with e: NSEvent) {
            point = convert(e.locationInWindow, from: nil)
            if part != 0 { event("up", part, point, size); part = 0 }
        }
        override func cancelOperation(_ sender: Any?) { cancel() }
        override func keyDown(with e: NSEvent) {
            if e.keyCode == 53 { cancel(); return }
            guard enabled, let nudge, let delta = parameterArrow(e.keyCode, large: e.modifierFlags.contains(.shift)) else { super.keyDown(with: e); return }
            nudge(keyActive ? "move" : "down", selected, delta); keyActive = true
        }
        override func keyUp(with e: NSEvent) {
            if parameterArrow(e.keyCode, large: false) != nil && keyActive { nudge?("up", selected, [0, 0]); keyActive = false }
            else { super.keyUp(with: e) }
        }
        override func resignFirstResponder() -> Bool { cancel(); return true }
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); if window == nil { cancel() } }
        func cancel() {
            if part != 0 { event("cancel", part, point, size); part = 0 }
            if keyActive { nudge?("cancel", selected, [0, 0]); keyActive = false }
        }
    }
}
#else
import UIKit
struct ParameterInput: UIViewRepresentable {
    @Environment(\.isEnabled) private var enabled
    let region: ParameterInputRegion
    let identity: String
    var nudge: ((String, UInt32, [Double]) -> Void)? = nil
    var touches: ColorTouchSurface? = nil
    let event: (String, UInt32, CGPoint, CGFloat) -> Void
    func makeUIView(context: Context) -> ContactView { ContactView() }
    func updateUIView(_ view: ContactView, context: Context) {
        if view.identity != identity || view.region != region || !enabled { view.cancel() }
        view.identity = identity; view.region = region; view.enabled = enabled; view.event = event; view.nudge = nudge
        view.attach(touches)
    }
    static func dismantleUIView(_ view: ContactView, coordinator: ()) { view.cancel() }
    final class ContactView: UIView {
        var region = ParameterInputRegion.parameter(hdr: false), enabled = true
        var identity = ""
        var event: (String, UInt32, CGPoint, CGFloat) -> Void = { _, _, _, _ in }
        let contact = Contact()
        var nudge: ((String, UInt32, [Double]) -> Void)?
        var selected: UInt32 = 1
        var keyActive = false
        private var touchInput: ColorTouchRecognizer?
        override var canBecomeFirstResponder: Bool { true }
        var size: CGFloat { bounds.width }
        override init(frame: CGRect) { super.init(frame: frame); isOpaque = false; backgroundColor = .clear; addGestureRecognizer(contact) }
        required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
        func hit(_ p: CGPoint) -> UInt32 { enabled ? region.hit(p, in: bounds) : 0 }
        override func point(inside p: CGPoint, with event: UIEvent?) -> Bool { hit(p) != 0 }
        func attach(_ surface: ColorTouchSurface?) {
            guard touchInput?.surface !== surface else { return }
            if let touchInput { removeGestureRecognizer(touchInput); touchInput.surface?.remove(self); self.touchInput = nil }
            guard let surface else { return }
            let recognizer = ColorTouchRecognizer(surface: surface)
            addGestureRecognizer(recognizer); touchInput = recognizer
            surface.onClaim(self) { [weak self] in self?.cancel() }
        }
        override func didMoveToWindow() { super.didMoveToWindow(); if window == nil { cancel() } }
        override func resignFirstResponder() -> Bool { cancel(); return super.resignFirstResponder() }
        func cancel() {
            contact.cancel()
            if keyActive { nudge?("cancel", selected, [0, 0]); keyActive = false }
        }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            guard enabled, let key = presses.first?.key else { super.pressesBegan(presses, with: event); return }
            if key.keyCode == .keyboardEscape { cancel(); return }
            let codes: [UIKeyboardHIDUsage: UInt16] = [.keyboardLeftArrow: 123, .keyboardRightArrow: 124, .keyboardDownArrow: 125, .keyboardUpArrow: 126]
            guard let code = codes[key.keyCode], let delta = parameterArrow(code, large: key.modifierFlags.contains(.shift)), let nudge else { super.pressesBegan(presses, with: event); return }
            nudge(keyActive ? "move" : "down", selected, delta); keyActive = true
        }
        override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            if keyActive { nudge?("up", selected, [0, 0]); keyActive = false }
            else { super.pressesEnded(presses, with: event) }
        }
        override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) { cancel(); super.pressesCancelled(presses, with: event) }
    }
    final class Contact: UIGestureRecognizer {
        private weak var touch: UITouch?
        private var part: UInt32 = 0
        private var point = CGPoint.zero
        override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
            guard touch == nil, let t = touches.first, let v = view as? ContactView else { return }
            point = t.location(in: v); part = v.hit(point)
            guard part != 0 else { state = .failed; return }
            v.selected = part; v.becomeFirstResponder()
            let reset = t.tapCount == 2 && v.region != .rectangle
            touch = t; state = .began; v.event(reset ? "reset" : "down", part, point, v.size)
            if reset { part = 0 }
        }
        override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
            guard let t = touch, touches.contains(t), part != 0, let v = view as? ContactView else { return }
            point = t.location(in: v); state = .changed; v.event("move", part, point, v.size)
        }
        override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
            guard let t = touch, touches.contains(t), let v = view as? ContactView else { return }
            if part != 0 { v.event("up", part, t.location(in: v), v.size) }; part = 0; state = .ended
        }
        override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { cancel() }
        func cancel() {
            if part != 0, let v = view as? ContactView { v.event("cancel", part, point, v.size) }
            part = 0; touch = nil
            if state == .began || state == .changed { state = .cancelled }
        }
        override func reset() { cancel(); super.reset() }
    }
}
#endif

private func parameterArrow(_ code: UInt16, large: Bool) -> [Double]? {
    let step: Double = large ? 10 : 1
    switch code { case 123: return [-step, 0]; case 124: return [step, 0]; case 125: return [0, -step]; case 126: return [0, step]; default: return nil }
}
