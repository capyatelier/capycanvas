import SwiftUI
import AppKit

/// Only the shared hit regions capture mouse/tablet input. Empty wheel corners
/// remain available to the enclosing scroll view.
struct ColorWheelInput: NSViewRepresentable {
    let shape: UInt32
    let context: String
    let value: String
    var touches: ColorTouchSurface? = nil
    var contact: ((String) -> Void)? = nil
    let pick: (UInt32, CGPoint, CGFloat) -> Void
    func makeNSView(context: Context) -> ContactView { ContactView() }
    func updateNSView(_ view: ContactView, context: Context) {
        if view.colorContext != self.context { view.cancel() }
        view.shape = shape; view.colorContext = self.context; view.pick = pick; view.phase = contact ?? { _ in }
        view.setAccessibilityValue(value)
        view.setAccessibilityLabel(context.environment.capyNativeCopy["color"]["wheel"].string)
    }
    final class ContactView: NSView {
        var shape: UInt32 = 0
        var colorContext = ""
        var part: UInt32 = 0
        var pick: (UInt32, CGPoint, CGFloat) -> Void = { _, _, _ in }
        var phase: (String) -> Void = { _ in }
        override var isFlipped: Bool { true }
        private var size: CGFloat { min(bounds.width, bounds.height) }
        override init(frame: NSRect) {
            super.init(frame: frame)
            setAccessibilityElement(true); setAccessibilityRole(.group)
            setAccessibilityIdentifier("color-wheel")
        }
        required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
        override func hitTest(_ point: NSPoint) -> NSView? {
            let local = convert(point, from: superview)
            return hit(local) == 0 ? nil : self
        }
        private func hit(_ point: CGPoint) -> UInt32 {
            capy_apple_color_hit(Float(point.x), Float(point.y), Float(size), shape)
        }
        override func mouseDown(with event: NSEvent) {
            let point = convert(event.locationInWindow, from: nil)
            part = hit(point)
            if part != 0 { phase("down"); pick(part, point, size) }
        }
        override func mouseDragged(with event: NSEvent) {
            if part != 0 { pick(part, convert(event.locationInWindow, from: nil), size) }
        }
        override func mouseUp(with event: NSEvent) {
            mouseDragged(with: event)
            if part != 0 { phase("up") }
            part = 0
        }
        func cancel() { if part != 0 { phase("cancel") }; part = 0 }
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); if window == nil { cancel() } }
    }
}
