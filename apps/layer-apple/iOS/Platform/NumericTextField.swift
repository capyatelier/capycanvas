import SwiftUI
import UIKit

/// UIKit owns keyboard focus and selection; all numeric edits stay shared.
struct NumericTextField: UIViewRepresentable {
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
    func makeUIView(context: Context) -> Field {
        let field = Field()
        field.borderStyle = .none; field.textAlignment = .right
        field.autocorrectionType = .no; field.spellCheckingType = .no
        field.autocapitalizationType = .none; field.smartDashesType = .no; field.smartQuotesType = .no
        field.returnKeyType = .done; field.delegate = context.coordinator
        field.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .editingChanged)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return field
    }
    func updateUIView(_ field: Field, context: Context) {
        context.coordinator.parent = self
        if field.text != text { field.text = text }
        field.placeholder = label; field.font = .systemFont(ofSize: fontSize)
        field.textColor = UIColor(color); field.isEnabled = enabled
        field.accessibilityLabel = label; field.accessibilityIdentifier = identifier
        field.cancel = cancel; field.step = step; field.submit = submit
        if focused && !field.isFirstResponder {
            let coordinator = context.coordinator
            DispatchQueue.main.async { [weak field] in
                guard coordinator.parent.focused, let field, !field.isFirstResponder else { return }
                if field.becomeFirstResponder() { field.selectAll(nil) }
            }
        } else if !focused && field.isFirstResponder { field.resignFirstResponder() }
    }
    final class Coordinator: NSObject, UITextFieldDelegate {
        var parent: NumericTextField
        init(_ parent: NumericTextField) { self.parent = parent }
        @objc func changed(_ field: UITextField) { parent.text = field.text ?? "" }
        func textFieldDidBeginEditing(_ textField: UITextField) { if !parent.focused { parent.focused = true } }
        func textFieldDidEndEditing(_ textField: UITextField) { if parent.focused { parent.focused = false } }
        func textFieldShouldReturn(_ textField: UITextField) -> Bool { parent.submit() }
    }
    final class Field: UITextField {
        var cancel: () -> Void = {}
        var step: (Int) -> Void = { _ in }
        var submit: () -> Bool = { true }
        override var keyCommands: [UIKeyCommand]? {
            let commands = [UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(cancelEdit)),
                UIKeyCommand(input: UIKeyCommand.inputUpArrow, modifierFlags: [], action: #selector(increase)),
                UIKeyCommand(input: UIKeyCommand.inputDownArrow, modifierFlags: [], action: #selector(decrease))]
            commands.forEach { $0.wantsPriorityOverSystemBehavior = true }
            return commands + (super.keyCommands ?? [])
        }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
            if presses.contains(where: { $0.key?.keyCode == .keyboardEscape }) { cancel(); return }
            if presses.contains(where: { $0.key?.keyCode == .keyboardTab }), !submit() { return }
            super.pressesBegan(presses, with: event)
        }
        override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
            if [#selector(cancelEdit), #selector(increase), #selector(decrease)].contains(action) { return true }
            return super.canPerformAction(action, withSender: sender)
        }
        @objc private func cancelEdit() { cancel() }
        @objc private func increase() { step(1) }
        @objc private func decrease() { step(-1) }
    }
}
