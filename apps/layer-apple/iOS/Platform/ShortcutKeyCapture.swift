import UIKit
import SwiftUI

struct ShortcutKeyCapture: UIViewRepresentable {
    let captured: (String, Bool, Bool, Bool) -> Void
    func makeUIView(context: Context) -> CaptureView { CaptureView() }
    func updateUIView(_ view: CaptureView, context: Context) { view.captured = captured }
    static func dismantleUIView(_ view: CaptureView, coordinator: ()) { view.restoreFocus(); view.captured = nil }
    final class CaptureView: UIView {
        var captured: ((String, Bool, Bool, Bool) -> Void)?
        private weak var previousResponder: UIView?
        override var canBecomeFirstResponder: Bool { true }
        override func didMoveToWindow() {
            super.didMoveToWindow()
            if let window {
                previousResponder = Self.firstResponder(in: window)
                becomeFirstResponder()
            }
        }
        func restoreFocus() {
            guard isFirstResponder else { return }
            if let previousResponder, previousResponder.window === window { previousResponder.becomeFirstResponder() }
            if isFirstResponder { resignFirstResponder() }
        }
        private static func firstResponder(in view: UIView) -> UIView? {
            if view.isFirstResponder { return view }
            return view.subviews.lazy.compactMap { firstResponder(in: $0) }.first
        }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            guard !NativeTextContext.composing else { super.pressesBegan(presses, with: event); return }
            for press in presses {
                guard let key = press.key else { continue }
                let name = AppleKeyName.name(key)
                let flags = key.modifierFlags
                if !name.isEmpty { captured?(name, !flags.intersection([.command, .control]).isEmpty,
                    flags.contains(.shift), flags.contains(.alternate)) }
            }
        }
        override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {}
    }
}

enum AppleKeyName {
    static func name(_ key: UIKey) -> String {
        let names: [Int: String] = [40: "enter", 41: "escape", 42: "backspace", 43: "tab",
            73: "insert", 74: "home", 75: "pageup", 76: "delete", 77: "end", 78: "pagedown",
            79: "arrowright", 80: "arrowleft", 81: "arrowdown", 82: "arrowup", 88: "enter",
            224: "Control", 225: "Shift", 226: "Alt", 227: "Meta", 228: "Control", 229: "Shift", 230: "Alt", 231: "Meta"]
        let code = key.keyCode.rawValue
        let function = (58...69).contains(code) ? "f\(code - 57)" : (104...115).contains(code) ? "f\(code - 91)" : nil
        return names[code] ?? function ?? key.charactersIgnoringModifiers
    }
}
