import SwiftUI
import AppKit

/// Only the shared hit regions capture mouse/tablet input. Empty wheel corners
/// remain available to the enclosing scroll view.
struct ColorWheelInput: NSViewRepresentable {
    let space: UInt32
    let context: String
    let pick: (UInt32, CGPoint, CGFloat) -> Void
    func makeNSView(context: Context) -> ContactView { ContactView() }
    func updateNSView(_ view: ContactView, context: Context) {
        if view.colorContext != self.context { view.part = 0 }
        view.space = space; view.colorContext = self.context; view.pick = pick
    }
    final class ContactView: NSView {
        var space: UInt32 = 0
        var colorContext = ""
        var part: UInt32 = 0
        var pick: (UInt32, CGPoint, CGFloat) -> Void = { _, _, _ in }
        override var isFlipped: Bool { true }
        private var size: CGFloat { min(bounds.width, bounds.height) }
        override init(frame: NSRect) {
            super.init(frame: frame)
            setAccessibilityElement(true); setAccessibilityRole(.group)
            setAccessibilityLabel("Color wheel"); setAccessibilityIdentifier("color-wheel")
        }
        required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
        override func hitTest(_ point: NSPoint) -> NSView? {
            let local = convert(point, from: superview)
            return hit(local) == 0 ? nil : self
        }
        private func hit(_ point: CGPoint) -> UInt32 {
            capy_apple_color_hit(Float(point.x), Float(point.y), Float(size), space)
        }
        override func mouseDown(with event: NSEvent) {
            let point = convert(event.locationInWindow, from: nil)
            part = hit(point)
            if part != 0 { pick(part, point, size) }
        }
        override func mouseDragged(with event: NSEvent) {
            if part != 0 { pick(part, convert(event.locationInWindow, from: nil), size) }
        }
        override func mouseUp(with event: NSEvent) { mouseDragged(with: event); part = 0 }
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); if window == nil { part = 0 } }
    }
}
