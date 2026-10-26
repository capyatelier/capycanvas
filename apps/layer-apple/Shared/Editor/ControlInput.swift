import SwiftUI

struct ControlModifiers {
    var shift = false
    var fine = false
}

struct ControlContact {
    let phase: String
    let point: CGPoint
    let surfacePoint: CGPoint
    let source: String
    let inside: Bool
    let modifiers: ControlModifiers
}

#if os(macOS)
import AppKit

struct ControlInput: NSViewRepresentable {
    @Environment(\.isEnabled) private var enabled
    var identity = ""
    var contact: ((ControlContact) -> Void)? = nil
    var wheel: ((Double) -> Void)? = nil
    var key: ((String, Bool, ControlModifiers) -> Bool)? = nil
    var secondary: ((CGPoint, CGSize) -> Void)? = nil
    var doublePress: ((CGPoint, CGSize) -> Void)? = nil
    func makeNSView(context: Context) -> InputView { InputView() }
    func updateNSView(_ view: InputView, context: Context) {
        if view.identity != identity || !enabled { view.cancel() }
        view.identity = identity; view.enabled = enabled
        view.contact = contact; view.wheel = wheel; view.key = key; view.secondary = secondary; view.doublePress = doublePress
    }
    static func dismantleNSView(_ view: InputView, coordinator: ()) { view.detach() }
    final class InputView: NSView {
        var identity = "", enabled = true
        var contact: ((ControlContact) -> Void)?
        var wheel: ((Double) -> Void)?
        var key: ((String, Bool, ControlModifiers) -> Bool)?
        var secondary: ((CGPoint, CGSize) -> Void)?
        var doublePress: ((CGPoint, CGSize) -> Void)?
        private var inactive: NSObjectProtocol?
        private var keyActive = false
        fileprivate var keyboardFocus = false
        private var active = false
        private var source = "mouse"
        private var point = CGPoint.zero
        override var isFlipped: Bool { true }
        override var acceptsFirstResponder: Bool { enabled && key != nil }
        private func modifiers(_ event: NSEvent) -> ControlModifiers {
            ControlModifiers(shift: event.modifierFlags.contains(.shift), fine: !event.modifierFlags.intersection([.option, .control]).isEmpty)
        }
        override func hitTest(_ point: NSPoint) -> NSView? {
            enabled && (contact != nil || secondary != nil) ? super.hitTest(point) : nil
        }
        override func mouseDown(with event: NSEvent) {
            guard enabled, contact != nil else { super.mouseDown(with: event); return }
            if key != nil { window?.makeFirstResponder(self) }; keyboardFocus = false
            if event.clickCount == 2, let doublePress { doublePress(convert(event.locationInWindow, from: nil), bounds.size); return }
            active = true; source = event.subtype == .tabletPoint || event.type == .tabletPoint ? "pen" : "mouse"
            send("down", event)
        }
        override func mouseDragged(with event: NSEvent) { if active { send("move", event) } }
        override func mouseUp(with event: NSEvent) { if active { active = false; send("up", event) } }
        override func rightMouseDown(with event: NSEvent) {
            guard enabled, let secondary else { super.rightMouseDown(with: event); return }
            cancel(); secondary(convert(event.locationInWindow, from: nil), bounds.size)
        }
        override func scrollWheel(with event: NSEvent) {
            guard enabled, let wheel, event.scrollingDeltaY != 0 else { super.scrollWheel(with: event); return }
            wheel(event.scrollingDeltaY > 0 ? 1 : -1)
        }
        override func keyDown(with event: NSEvent) {
            guard !NativeTextContext.composing else { super.keyDown(with: event); return }
            if event.keyCode == 53, active { cancel(); return }
            if let name = Self.keyName(event.keyCode), takes(name, event.modifierFlags), key?(name, true, modifiers(event)) == true { keyActive = name != "Escape"; return }
            if let canvas = canvas(window?.contentView) { canvas.keyDown(with: event) } else { super.keyDown(with: event) }
        }
        override func keyUp(with event: NSEvent) {
            guard !NativeTextContext.composing else { super.keyUp(with: event); return }
            if let name = Self.keyName(event.keyCode), takes(name, event.modifierFlags), key?(name, false, modifiers(event)) == true { keyActive = false; return }
            if let canvas = canvas(window?.contentView) { canvas.keyUp(with: event) } else { super.keyUp(with: event) }
        }
        override func cancelOperation(_ sender: Any?) {
            guard !NativeTextContext.composing, active || keyActive else {
                _ = nextResponder?.tryToPerform(#selector(NSResponder.cancelOperation(_:)), with: sender); return
            }
            cancel()
        }
        override func becomeFirstResponder() -> Bool { keyboardFocus = true; return super.becomeFirstResponder() }
        override func resignFirstResponder() -> Bool { keyboardFocus = false; cancel(); return true }
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow(); detach()
            if let window { inactive = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.cancel() }
            } }
        }
        func detach() { if let inactive { NotificationCenter.default.removeObserver(inactive); self.inactive = nil }; cancel() }
        private func send(_ phase: String, _ event: NSEvent) {
            point = convert(event.locationInWindow, from: nil)
            contact?(ControlContact(phase: phase, point: point, surfacePoint: CGPoint(x: event.locationInWindow.x, y: -event.locationInWindow.y), source: source, inside: bounds.contains(point), modifiers: modifiers(event)))
        }
        func cancel() {
            if active { active = false; contact?(ControlContact(phase: "cancel", point: point, surfacePoint: point, source: source, inside: false, modifiers: ControlModifiers())) }
            if keyActive { keyActive = false; _ = key?("Escape", true, ControlModifiers()) }
        }
        private func canvas(_ view: NSView?) -> MacCanvasView? {
            if let canvas = view as? MacCanvasView { return canvas }
            return view?.subviews.lazy.compactMap { self.canvas($0) }.first
        }
        private func takes(_ name: String, _ flags: NSEvent.ModifierFlags) -> Bool {
            !["Enter", "Space"].contains(name) || name.withCString {
                capy_apple_control_takes_key($0, !flags.intersection([.command, .control]).isEmpty, flags.contains(.option), keyboardFocus)
            }
        }
        private static func keyName(_ code: UInt16) -> String? {
            switch code {
            case 123: "ArrowLeft"
            case 124: "ArrowRight"
            case 125: "ArrowDown"
            case 126: "ArrowUp"
            case 116: "PageUp"
            case 121: "PageDown"
            case 115: "Home"
            case 119: "End"
            case 53: "Escape"
            case 36, 76: "Enter"
            case 49: "Space"
            default: nil
            }
        }
    }
}
#else
import UIKit

struct ControlInput: UIViewRepresentable {
    @Environment(\.isEnabled) private var enabled
    var identity = ""
    var contact: ((ControlContact) -> Void)? = nil
    var wheel: ((Double) -> Void)? = nil
    var key: ((String, Bool, ControlModifiers) -> Bool)? = nil
    var secondary: ((CGPoint, CGSize) -> Void)? = nil
    var doublePress: ((CGPoint, CGSize) -> Void)? = nil
    func makeUIView(context: Context) -> InputView { InputView() }
    func updateUIView(_ view: InputView, context: Context) {
        if view.identity != identity || !enabled { view.cancel() }
        view.identity = identity; view.isUserInteractionEnabled = enabled
        view.contact = contact; view.wheel = wheel; view.key = key; view.secondary = secondary; view.doublePress = doublePress
    }
    static func dismantleUIView(_ view: InputView, coordinator: ()) { view.detach() }
    final class InputView: UIView {
        var identity = ""
        var contact: ((ControlContact) -> Void)?
        var wheel: ((Double) -> Void)?
        var key: ((String, Bool, ControlModifiers) -> Bool)?
        var secondary: ((CGPoint, CGSize) -> Void)?
        var doublePress: ((CGPoint, CGSize) -> Void)?
        private var inactive: NSObjectProtocol?
        private var keyActive = false
        fileprivate var keyboardFocus = false
        private let pointer = Pointer()
        private var scroll: UIPanGestureRecognizer!
        override var canBecomeFirstResponder: Bool { key != nil }
        override init(frame: CGRect) {
            super.init(frame: frame); isOpaque = false; backgroundColor = .clear
            addGestureRecognizer(pointer)
            scroll = UIPanGestureRecognizer(target: self, action: #selector(scrolled))
            scroll.allowedTouchTypes = []; scroll.allowedScrollTypesMask = .all
            addGestureRecognizer(scroll)
        }
        required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
        override func didMoveToWindow() {
            super.didMoveToWindow(); detach()
            if let scene = window?.windowScene { inactive = NotificationCenter.default.addObserver(forName: UIScene.willDeactivateNotification, object: scene, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.cancel() }
            } }
        }
        func detach() { if let inactive { NotificationCenter.default.removeObserver(inactive); self.inactive = nil }; cancel() }
        override func becomeFirstResponder() -> Bool { keyboardFocus = true; return super.becomeFirstResponder() }
        override func resignFirstResponder() -> Bool { keyboardFocus = false; cancel(); return super.resignFirstResponder() }
        @objc private func scrolled(_ gesture: UIPanGestureRecognizer) {
            guard wheel != nil, gesture.state == .began || gesture.state == .changed else { return }
            let delta = gesture.translation(in: self).y
            if delta != 0 { wheel?(delta > 0 ? 1 : -1); gesture.setTranslation(.zero, in: self) }
        }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            guard !NativeTextContext.composing else { super.pressesBegan(presses, with: event); return }
            var pending = Set<UIPress>()
            for press in presses {
                guard let native = press.key, let name = Self.keyName(native.keyCode), takes(name, native.modifierFlags),
                    key?(name, true, Self.modifiers(native.modifierFlags)) == true else { pending.insert(press); continue }
                keyActive = name != "Escape"
            }
            if !pending.isEmpty {
                if let canvas = canvas(window) { canvas.pressesBegan(pending, with: event) } else { super.pressesBegan(pending, with: event) }
            }
        }
        override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            guard !NativeTextContext.composing else { super.pressesEnded(presses, with: event); return }
            var pending = Set<UIPress>()
            for press in presses {
                guard let native = press.key, let name = Self.keyName(native.keyCode), takes(name, native.modifierFlags),
                    key?(name, false, Self.modifiers(native.modifierFlags)) == true else { pending.insert(press); continue }
                keyActive = false
            }
            if !pending.isEmpty {
                if let canvas = canvas(window) { canvas.pressesEnded(pending, with: event) } else { super.pressesEnded(pending, with: event) }
            }
        }
        override func pressesCancelled(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            cancel()
            if let canvas = canvas(window) { canvas.pressesCancelled(presses, with: event) } else { super.pressesCancelled(presses, with: event) }
        }
        func cancel() { pointer.cancel(); if keyActive { keyActive = false; _ = key?("Escape", true, ControlModifiers()) } }
        static func modifiers(_ flags: UIKeyModifierFlags) -> ControlModifiers {
            ControlModifiers(shift: flags.contains(.shift), fine: !flags.intersection([.alternate, .control]).isEmpty)
        }
        private func canvas(_ view: UIView?) -> CanvasView? {
            if let canvas = view as? CanvasView { return canvas }
            return view?.subviews.lazy.compactMap { self.canvas($0) }.first
        }
        private func takes(_ name: String, _ flags: UIKeyModifierFlags) -> Bool {
            !["Enter", "Space"].contains(name) || name.withCString {
                capy_apple_control_takes_key($0, !flags.intersection([.command, .control]).isEmpty, flags.contains(.alternate), keyboardFocus)
            }
        }
        private static func keyName(_ code: UIKeyboardHIDUsage) -> String? {
            switch code {
            case .keyboardLeftArrow: "ArrowLeft"
            case .keyboardRightArrow: "ArrowRight"
            case .keyboardDownArrow: "ArrowDown"
            case .keyboardUpArrow: "ArrowUp"
            case .keyboardPageUp: "PageUp"
            case .keyboardPageDown: "PageDown"
            case .keyboardHome: "Home"
            case .keyboardEnd: "End"
            case .keyboardEscape: "Escape"
            case .keyboardReturnOrEnter, .keypadEnter: "Enter"
            case .keyboardSpacebar: "Space"
            default: nil
            }
        }
    }
    final class Pointer: UIGestureRecognizer {
        private weak var touch: UITouch?
        private var point = CGPoint.zero
        private var source = "touch"
        override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
            guard touch == nil, let native = touches.first, let view = view as? InputView else { return }
            point = native.location(in: view); view.keyboardFocus = false
            if event.buttonMask.contains(.secondary), let secondary = view.secondary {
                secondary(point, view.bounds.size); state = .recognized; return
            }
            if native.tapCount == 2, let doublePress = view.doublePress { doublePress(point, view.bounds.size); state = .recognized; return }
            guard view.contact != nil else { state = .failed; return }
            touch = native; source = native.type == .pencil ? "pen" : native.type == .indirectPointer ? "mouse" : "touch"
            if view.key != nil { view.becomeFirstResponder() }; view.keyboardFocus = false
            state = .began; send("down")
        }
        override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
            guard let touch, touches.contains(touch), let view else { return }
            point = touch.location(in: view); state = .changed; send("move")
        }
        override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
            guard let touch, touches.contains(touch), let view else { return }
            point = touch.location(in: view); self.touch = nil; send("up"); state = .ended
        }
        override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { cancel() }
        private func send(_ phase: String) {
            (view as? InputView)?.contact?(ControlContact(phase: phase, point: point, surfacePoint: view?.convert(point, to: nil) ?? point, source: source, inside: view?.bounds.contains(point) == true, modifiers: InputView.modifiers(modifierFlags)))
        }
        func cancel() { let owned = touch != nil; touch = nil; if owned { send("cancel") }; if state == .began || state == .changed { state = .cancelled } }
        override func reset() { cancel(); super.reset() }
    }
}
#endif
