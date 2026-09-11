import SwiftUI
import UIKit

struct ColorWheelInput: UIViewRepresentable {
    let space: UInt32
    let context: String
    let pick: (UInt32, CGPoint, CGFloat) -> Void
    func makeUIView(context: Context) -> ContactView { ContactView() }
    func updateUIView(_ view: ContactView, context: Context) {
        if view.colorContext != self.context { view.contact.cancel() }
        view.space = space; view.colorContext = self.context; view.pick = pick
    }
    final class ContactView: UIView {
        var space: UInt32 = 0
        var colorContext = ""
        var pick: (UInt32, CGPoint, CGFloat) -> Void = { _, _, _ in }
        let contact = Contact()
        var size: CGFloat { min(bounds.width, bounds.height) }
        override init(frame: CGRect) {
            super.init(frame: frame)
            isOpaque = false; backgroundColor = .clear
            isAccessibilityElement = true; accessibilityLabel = "Color wheel"; accessibilityIdentifier = "color-wheel"
            addGestureRecognizer(contact)
        }
        required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
        override func point(inside point: CGPoint, with event: UIEvent?) -> Bool { hit(point) != 0 }
        func hit(_ point: CGPoint) -> UInt32 {
            capy_apple_color_hit(Float(point.x), Float(point.y), Float(size), space)
        }
        override func didMoveToWindow() { super.didMoveToWindow(); if window == nil { contact.cancel() } }
    }
    /// Begin at contact, before an ancestor scroll recognizer can claim a valid
    /// wheel drag. The start region remains latched while dragging outside it.
    final class Contact: UIGestureRecognizer {
        private weak var touch: UITouch?
        private var part: UInt32 = 0
        override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
            guard touch == nil, let touch = touches.first, let view = view as? ContactView else { return }
            let point = touch.location(in: view)
            part = view.hit(point)
            guard part != 0 else { state = .failed; return }
            self.touch = touch; state = .began
            view.pick(part, point, view.size)
        }
        override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
            guard let touch, touches.contains(touch), part != 0, let view = view as? ContactView else { return }
            state = .changed; view.pick(part, touch.location(in: view), view.size)
        }
        override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
            guard let touch, touches.contains(touch) else { return }
            if part != 0, let view = view as? ContactView { view.pick(part, touch.location(in: view), view.size) }
            state = .ended
        }
        override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { cancel() }
        func cancel() { if state == .began || state == .changed { state = .cancelled }; part = 0; touch = nil }
        override func reset() { super.reset(); part = 0; touch = nil }
    }
}
