import SwiftUI

#if os(macOS)
import AppKit
struct ParameterInput: NSViewRepresentable {
    let hdr: Bool
    let identity: String
    var nudge: ((String, UInt32, [Double]) -> Void)? = nil
    let event: (String, UInt32, CGPoint, CGFloat) -> Void
    func makeNSView(context: Context) -> ContactView { ContactView() }
    func updateNSView(_ view: ContactView, context: Context) {
        if view.identity != identity { view.cancel() }
        view.identity = identity; view.hdr = hdr; view.event = event; view.nudge = nudge
    }
    static func dismantleNSView(_ view: ContactView, coordinator: ()) { view.cancel() }
    final class ContactView: NSView {
        var hdr = false, identity = "", part: UInt32 = 0
        var event: (String, UInt32, CGPoint, CGFloat) -> Void = { _, _, _, _ in }
        var point = CGPoint.zero
        var nudge: ((String, UInt32, [Double]) -> Void)?
        var selected: UInt32 = 1
        var keyActive = false
        override var isFlipped: Bool { true }
        override var acceptsFirstResponder: Bool { true }
        var size: CGFloat { bounds.width }
        func hit(_ p: CGPoint) -> UInt32 { capy_apple_parameter_hit(Float(p.x), Float(p.y), Float(size), hdr) }
        override func hitTest(_ p: NSPoint) -> NSView? { hit(convert(p, from: superview)) == 0 ? nil : self }
        override func mouseDown(with e: NSEvent) {
            point = convert(e.locationInWindow, from: nil); part = hit(point)
            if part != 0 { selected = part }
            if part != 0 { window?.makeFirstResponder(self); event(e.clickCount == 2 ? "reset" : "down", part, point, size) }
            if e.clickCount == 2 { part = 0 }
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
            guard let nudge, let delta = parameterArrow(e.keyCode, large: e.modifierFlags.contains(.shift)) else { super.keyDown(with: e); return }
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
    let hdr: Bool
    let identity: String
    var nudge: ((String, UInt32, [Double]) -> Void)? = nil
    let event: (String, UInt32, CGPoint, CGFloat) -> Void
    func makeUIView(context: Context) -> ContactView { ContactView() }
    func updateUIView(_ view: ContactView, context: Context) {
        if view.identity != identity { view.cancel() }
        view.identity = identity; view.hdr = hdr; view.event = event; view.nudge = nudge
    }
    static func dismantleUIView(_ view: ContactView, coordinator: ()) { view.cancel() }
    final class ContactView: UIView {
        var hdr = false, identity = ""
        var event: (String, UInt32, CGPoint, CGFloat) -> Void = { _, _, _, _ in }
        let contact = Contact()
        var nudge: ((String, UInt32, [Double]) -> Void)?
        var selected: UInt32 = 1
        var keyActive = false
        override var canBecomeFirstResponder: Bool { true }
        var size: CGFloat { bounds.width }
        override init(frame: CGRect) { super.init(frame: frame); isOpaque = false; backgroundColor = .clear; addGestureRecognizer(contact) }
        required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
        func hit(_ p: CGPoint) -> UInt32 { capy_apple_parameter_hit(Float(p.x), Float(p.y), Float(size), hdr) }
        override func point(inside p: CGPoint, with event: UIEvent?) -> Bool { hit(p) != 0 }
        override func didMoveToWindow() { super.didMoveToWindow(); if window == nil { cancel() } }
        override func resignFirstResponder() -> Bool { cancel(); return super.resignFirstResponder() }
        func cancel() {
            contact.cancel()
            if keyActive { nudge?("cancel", selected, [0, 0]); keyActive = false }
        }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            guard let key = presses.first?.key else { super.pressesBegan(presses, with: event); return }
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
            touch = t; state = .began; v.event(t.tapCount == 2 ? "reset" : "down", part, point, v.size)
            if t.tapCount == 2 { part = 0 }
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
