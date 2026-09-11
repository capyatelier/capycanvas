import UIKit
import SwiftUI

struct ShortcutKeyCapture: UIViewRepresentable {
    let captured: (String, Bool, Bool, Bool) -> Void
    func makeUIView(context: Context) -> CaptureView { CaptureView() }
    func updateUIView(_ view: CaptureView, context: Context) { view.captured = captured }
    static func dismantleUIView(_ view: CaptureView, coordinator: ()) { view.resignFirstResponder(); view.captured = nil }
    final class CaptureView: UIView {
        var captured: ((String, Bool, Bool, Bool) -> Void)?
        override var canBecomeFirstResponder: Bool { true }
        override func didMoveToWindow() {
            super.didMoveToWindow()
            if window != nil { becomeFirstResponder() }
        }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
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
            79: "arrowright", 80: "arrowleft", 81: "arrowdown", 82: "arrowup", 88: "enter"]
        let code = key.keyCode.rawValue
        let function = (58...69).contains(code) ? "f\(code - 57)" : (104...115).contains(code) ? "f\(code - 91)" : nil
        return names[code] ?? function ?? key.charactersIgnoringModifiers
    }
}
