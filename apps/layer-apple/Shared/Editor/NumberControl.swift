import SwiftUI

struct NumberControl: View {
    @ObservedObject var store: EditorStore
    let label: String
    let value: Double
    let control: JSON
    var identifier = ""
    let change: (Double, @escaping @MainActor (String?) -> Void) -> Void
    @State private var field = NumericEditState()
    @State private var formatted = JSON()
    @State private var showsEntry = false
    @State private var horizontalDrag: Bool?
    @State private var editing = false
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var slider: Bool { control["kind"].string == "slider" }
    private var key: String { identifier.isEmpty ? label : identifier }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                Text(label).padding(.leading, 6)
                Spacer(minLength: 0)
                if !slider { stepButton(-1) }
                if showsEntry || !slider || field.dirty {
                    NumericTextField(label: label,
                        text: Binding(get: { field.text }, set: { field.text = $0; field.dirty = true }),
                        focused: $editing, fontSize: max(1, store.catalog["text_size_pt"].number * 4 / 3),
                        color: palette["text"], identifier: "number-entry-" + key,
                        submit: finish, cancel: cancel, step: step)
                        .frame(width: slider ? 80 : 60)
                        .padding(.horizontal, 6).frame(height: slider ? 24 : 32)
                        .background(palette["input"], in: RoundedRectangle(cornerRadius: 6))
                        .overlay(RoundedRectangle(cornerRadius: 6).stroke(field.error == nil ? Color.clear : Color.red, lineWidth: 1))
                        .onAppear { if showsEntry { editing = true } }
                } else {
                    Button { showsEntry = true } label: {
                        Text(formatted["text"].string).padding(.horizontal, 6).frame(height: 24)
                    }.buttonStyle(.plain).accessibilityLabel(label)
                        .accessibilityValue(formatted["text"].string)
                        .accessibilityIdentifier("number-value-" + key)
                }
                if !slider { stepButton(1) }
            }
            if slider {
                HStack(spacing: 6) {
                    stepButton(-1)
                    GeometryReader { geometry in
                        ZStack(alignment: .leading) {
                            Capsule().fill(palette["input"])
                            Capsule().fill(palette["text"].opacity(0.5))
                                .frame(width: max(0, geometry.size.width * formatted["fill"].number))
                        }.frame(height: 4).frame(maxHeight: .infinity)
                            .contentShape(Rectangle())
                            .onTapGesture { location in position(location.x / max(1, geometry.size.width)) }
                            // Preserve vertical scrolling through long tool-settings lists.
                            .simultaneousGesture(DragGesture(minimumDistance: 6).onChanged { event in
                                if horizontalDrag == nil {
                                    horizontalDrag = abs(event.translation.width) > abs(event.translation.height)
                                }
                                if horizontalDrag == true {
                                    position(event.location.x / max(1, geometry.size.width))
                                }
                            }.onEnded { _ in horizontalDrag = nil })
                    }.frame(height: 24)
                        .accessibilityElement().accessibilityLabel(label)
                        .accessibilityValue(formatted["text"].string)
                        .accessibilityAdjustableAction { direction in
                            switch direction {
                            case .increment: step(1)
                            case .decrement: step(-1)
                            @unknown default: break
                            }
                        }.accessibilityIdentifier("number-track-" + key)
                    stepButton(1)
                }
            }
            if let error = field.error {
                Text(error).font(.caption).foregroundStyle(.red).padding(.leading, 6)
                    .accessibilityIdentifier("number-error-" + key)
            }
        }
        .onAppear { field.receive(value); format() }
        .onChange(of: value) { _, next in field.receive(next); format() }
        .onChange(of: editing) { _, focused in
            if focused {
                if !field.dirty { field.text = formatted["edit"].string }
            } else if commit() { showsEntry = false }
        }
    }
    private func finish() -> Bool {
        guard commit() else { return false }
        showsEntry = false; editing = false
        return true
    }
    private func cancel() {
        field.dirty = false; field.error = nil; showsEntry = false; editing = false; format()
    }
    private func stepButton(_ direction: Int) -> some View {
        Button { step(direction) } label: {
            SharedIcon(name: direction < 0 ? "minus" : "plus").frame(width: slider ? 24 : 32, height: slider ? 24 : 32)
                .background(slider ? Color.clear : palette["input"], in: RoundedRectangle(cornerRadius: 6))
        }.buttonStyle(.plain)
            .disabled(direction < 0 ? field.value <= control["min"].number : field.value >= control["max"].number)
            .accessibilityLabel((direction < 0 ? "Decrease " : "Increase ") + label)
            .accessibilityIdentifier("number-\(direction < 0 ? "decrease" : "increase")-" + key)
    }
    private func format() {
        do {
            formatted = try store.resolveNumber(control, value: field.value, operation: ["type": "format"])
            if !field.dirty && !editing { field.text = formatted["edit"].string }
        } catch { field.error = error.localizedDescription }
    }
    @discardableResult private func commit() -> Bool {
        guard field.dirty else { return true }
        return resolve(["type": "expression", "text": field.text])
    }
    private func step(_ direction: Int) {
        guard commit() else { return }
        _ = resolve(["type": "step", "steps": direction])
    }
    private func position(_ position: Double) {
        field.dirty = false; showsEntry = false; editing = false
        _ = resolve(["type": "position", "position": min(1, max(0, position))])
    }
    @discardableResult private func resolve(_ operation: [String: Any]) -> Bool {
        do {
            let result = try store.resolveNumber(control, value: field.value, operation: operation)
            field.dirty = false; field.error = nil
            field.text = result["edit"].string
            let next = result["value"].number
            if Float(next) != Float(field.value) {
                let token = field.submit(next)
                change(next) { error in field.complete(token, error: error); format() }
            }
            format()
            return true
        } catch {
            field.error = error.localizedDescription
            return false
        }
    }
}
