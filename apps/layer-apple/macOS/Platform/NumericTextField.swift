import SwiftUI
import AppKit

/// AppKit owns text selection, first responder and text-editor key commands.
struct NumericTextField: NSViewRepresentable {
    let label: String
    @Binding var text: String
    @Binding var focused: Bool
    let fontSize: CGFloat
    let color: Color
    let identifier: String
    let submit: () -> Bool
    let cancel: () -> Void
    let step: (Int) -> Void
    @Environment(\.isEnabled) private var enabled
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeNSView(context: Context) -> NSTextField {
        let field = NSTextField()
        field.isBordered = false; field.drawsBackground = false
        field.focusRingType = .none; field.alignment = .right
        field.lineBreakMode = .byClipping; field.maximumNumberOfLines = 1
        field.delegate = context.coordinator
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return field
    }
    func updateNSView(_ field: NSTextField, context: Context) {
        context.coordinator.parent = self
        if field.stringValue != text { field.stringValue = text }
        field.placeholderString = label
        field.font = .systemFont(ofSize: fontSize)
        field.textColor = NSColor(color); field.isEnabled = enabled
        field.setAccessibilityLabel(label); field.setAccessibilityIdentifier(identifier)
        if focused && field.currentEditor() == nil {
            let coordinator = context.coordinator
            DispatchQueue.main.async { [weak field] in
                guard coordinator.parent.focused, let field, field.currentEditor() == nil else { return }
                field.window?.makeFirstResponder(field)
                field.currentEditor()?.selectAll(nil)
            }
        } else if !focused && field.currentEditor() != nil {
            field.window?.makeFirstResponder(nil)
        }
    }
    final class Coordinator: NSObject, NSTextFieldDelegate {
        var parent: NumericTextField
        init(_ parent: NumericTextField) { self.parent = parent }
        func controlTextDidBeginEditing(_ notification: Notification) { if !parent.focused { parent.focused = true } }
        func controlTextDidChange(_ notification: Notification) {
            if let field = notification.object as? NSTextField { parent.text = field.stringValue }
        }
        func controlTextDidEndEditing(_ notification: Notification) { if parent.focused { parent.focused = false } }
        func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
            switch selector {
            case #selector(NSResponder.cancelOperation(_:)): parent.cancel(); return true
            case #selector(NSResponder.insertNewline(_:)): _ = parent.submit(); return true
            case #selector(NSResponder.insertTab(_:)), #selector(NSResponder.insertBacktab(_:)):
                return !parent.submit()
            case #selector(NSResponder.moveUp(_:)): parent.step(1); return true
            case #selector(NSResponder.moveDown(_:)): parent.step(-1); return true
            default: return false
            }
        }
    }
}
