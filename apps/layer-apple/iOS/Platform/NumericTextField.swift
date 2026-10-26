import SwiftUI
import UIKit

/// UIKit owns keyboard focus and selection; all numeric edits stay shared.
struct NumericTextField: UIViewRepresentable {
    @Environment(\.capyInterfaceLanguage) private var interfaceLanguage
    let label: String
    @Binding var text: String
    @Binding var focused: Bool
    let fontSize: CGFloat
    let color: Color
    let identifier: String
    let submit: (_ returnToCanvas: Bool) -> Bool
    let cancel: () -> Void
    var step: ((Int) -> Void)? = nil
    var placeholder: String?
    var alignment: NSTextAlignment = .right
    var returnKey: UIReturnKeyType = .done
    var selectsReplacedText = false
    var borderStyle: UITextField.BorderStyle = .none
    var keyboardType: UIKeyboardType = .default
    var selectsOnFocus = true
    var monospaced = true
    var rounded = false
    @Environment(\.isEnabled) private var enabled
    func makeCoordinator() -> Coordinator { Coordinator(self) }
    func makeUIView(context: Context) -> Field {
        let field = Field()
        field.text = text
        field.borderStyle = .none
        field.autocorrectionType = .no; field.spellCheckingType = .no
        field.autocapitalizationType = .none; field.smartDashesType = .no; field.smartQuotesType = .no
        field.delegate = context.coordinator
        field.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .editingChanged)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return field
    }
    func updateUIView(_ field: Field, context: Context) {
        context.coordinator.parent = self
        if field.markedTextRange == nil && field.text != text {
            context.coordinator.updateText(field)
        }
        if field.textAlignment != alignment { field.textAlignment = alignment }
        let border = rounded ? UITextField.BorderStyle.roundedRect : borderStyle
        if field.borderStyle != border { field.borderStyle = border }
        if field.keyboardType != keyboardType { field.keyboardType = keyboardType }
        if field.returnKeyType != returnKey { field.returnKeyType = returnKey }
        if field.placeholder != placeholder ?? label { field.placeholder = placeholder ?? label }
        let font = monospaced ? UIFont.monospacedDigitSystemFont(ofSize: fontSize, weight: .regular) : .systemFont(ofSize: fontSize)
        if field.font != font { field.font = font }
        field.textColor = UIColor(color); field.isEnabled = enabled
        field.accessibilityLanguage = interfaceLanguage.isEmpty ? nil : interfaceLanguage
        field.interfaceLanguage = interfaceLanguage
        field.accessibilityLabel = label; field.accessibilityIdentifier = identifier
        field.cancel = cancel; field.step = step; field.submit = submit
        if focused && !field.isFirstResponder {
            let coordinator = context.coordinator
            DispatchQueue.main.async { [weak field] in
                guard coordinator.parent.focused, let field, !field.isFirstResponder else { return }
                field.becomeFirstResponder()
                if !coordinator.parent.selectsOnFocus { field.selectedTextRange = field.textRange(from: field.endOfDocument, to: field.endOfDocument) }
            }
        } else if !focused && field.isFirstResponder {
            // UIKit consults the hosting view's responder graph while resigning.
            // Defer that work until SwiftUI has finished its current update.
            let coordinator = context.coordinator
            DispatchQueue.main.async { [weak field] in
                guard !coordinator.parent.focused, let field, field.isFirstResponder else { return }
                field.resignFirstResponder()
            }
        }
    }
    final class Coordinator: NSObject, UITextFieldDelegate {
        var parent: NumericTextField
        private var textUpdatePending = false
        init(_ parent: NumericTextField) { self.parent = parent }
        func updateText(_ field: UITextField) {
            guard !textUpdatePending else { return }
            textUpdatePending = true
            DispatchQueue.main.async { [weak field] in
                self.textUpdatePending = false
                guard let field, field.markedTextRange == nil else { return }
                let text = self.parent.text
                guard field.text != text else { return }
                field.text = text
                if self.parent.selectsReplacedText && field.isFirstResponder { field.selectAll(nil) }
            }
        }
        @objc func changed(_ field: UITextField) { parent.text = field.text ?? "" }
        func textFieldDidBeginEditing(_ textField: UITextField) {
            if !parent.focused { parent.focused = true }
            if parent.selectsOnFocus {
                DispatchQueue.main.async { [weak textField] in
                    if let textField, textField.isFirstResponder { textField.selectAll(nil) }
                }
            }
        }
        func textFieldDidEndEditing(_ textField: UITextField) { if parent.focused { parent.focused = false } }
        func textFieldShouldReturn(_ textField: UITextField) -> Bool {
            guard textField.markedTextRange == nil else { return false }
            return parent.submit(true)
        }
    }
    final class Field: UITextField {
        var interfaceLanguage = ""
        override var textInputContextIdentifier: String? {
            interfaceLanguage.isEmpty ? super.textInputContextIdentifier : "capy.numeric.\(interfaceLanguage)"
        }
        var cancel: () -> Void = {}
        var step: ((Int) -> Void)?
        var submit: (Bool) -> Bool = { _ in true }
        override var keyCommands: [UIKeyCommand]? {
            guard markedTextRange == nil else { return super.keyCommands }
            var commands = [UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(cancelEdit))]
            if step != nil {
                commands += [UIKeyCommand(input: UIKeyCommand.inputUpArrow, modifierFlags: [], action: #selector(increase)),
                    UIKeyCommand(input: UIKeyCommand.inputDownArrow, modifierFlags: [], action: #selector(decrease))]
            }
            commands.forEach { $0.wantsPriorityOverSystemBehavior = true }
            return commands + (super.keyCommands ?? [])
        }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            guard markedTextRange == nil else { super.pressesBegan(presses, with: event); return }
            if presses.contains(where: { $0.key?.keyCode == .keyboardEscape }) { cancel(); return }
            if presses.contains(where: { $0.key?.keyCode == .keyboardTab }), !submit(false) { return }
            super.pressesBegan(presses, with: event)
        }
        override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
            if [#selector(increase), #selector(decrease)].contains(action) { return markedTextRange == nil && step != nil }
            if action == #selector(cancelEdit) { return markedTextRange == nil }
            return super.canPerformAction(action, withSender: sender)
        }
        @objc private func cancelEdit() { cancel() }
        @objc private func increase() { step?(1) }
        @objc private func decrease() { step?(-1) }
    }
}
