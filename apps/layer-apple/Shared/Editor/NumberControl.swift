import SwiftUI

struct NumberControl: View {
    @ObservedObject var store: EditorStore
    let label: String
    let value: Double
    let control: JSON
    var identifier = ""
    var valueOnly = false
    var inline = false
    let change: (Double, @escaping @MainActor (String?) -> Void) -> Void
    @State private var field = NumericEditState()
    @State private var formatted = JSON()
    @State private var showsEntry = false
    @State private var horizontalDrag: Bool?
    @State private var editing = false
    @State private var inlineMeasure = ""
    @Environment(\.isEnabled) private var enabled
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var slider: Bool { control["kind"].string == "slider" }
    private var key: String { identifier.isEmpty ? label : identifier }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if inline {
                HStack(spacing: 4) {
                    sliderTrack
                    InlineNumberValueLayout(entrySize: showsEntry || field.dirty ? min(10, max(3, formatted["edit"].string.count)) : nil) {
                        Text(inlineMeasure).monospacedDigit().padding(.horizontal, 6).hidden().accessibilityHidden(true)
                        Text("8").monospacedDigit().hidden().accessibilityHidden(true)
                        if showsEntry || field.dirty { numericEntry }
                        else { valueButton }
                    }
                }
            } else if valueOnly { numericEntry }
            else {
                HStack(spacing: 6) {
                    Text(label).lineLimit(1).truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 6)
                        .modifier(NumberControlMeasurement(id: key + ":label"))
                    if !slider {
                        HStack(spacing: 0) {
                            numericEntry
                            stepButton(-1)
                            stepButton(1)
                        }.background(palette["input"], in: RoundedRectangle(cornerRadius: 6))
                            .clipShape(RoundedRectangle(cornerRadius: 6)).fixedSize()
                    } else if showsEntry || field.dirty {
                        numericEntry.fixedSize()
                    } else {
                        valueButton.fixedSize()
                    }
                }.modifier(NumberControlMeasurement(id: key + ":header"))
            }
            if slider && !valueOnly && !inline {
                HStack(spacing: 6) {
                    stepButton(-1)
                    sliderTrack
                    stepButton(1)
                }
            }
            if let error = field.error, !inline {
                Text(error).font(.caption).foregroundStyle(.red).padding(.leading, 6)
                    .accessibilityIdentifier("number-error-" + key)
            }
        }
        .modifier(NumberControlMeasurement(id: key + ":root"))
        .onAppear { field.receive(value); format(); measureInlineRange() }
        .onChange(of: value) { _, next in field.receive(next); format() }
        .onChange(of: editing) { _, focused in
            if focused {
                if !field.dirty { field.text = formatted["edit"].string }
            } else if commit() { showsEntry = false; format() }
        }
    }
    private var valueButton: some View {
        Button { showsEntry = true } label: {
            Text(formatted["text"].string).monospacedDigit()
                .frame(maxWidth: inline ? .infinity : nil, alignment: .trailing)
                .padding(.horizontal, 6).frame(height: 24)
        }.buttonStyle(EditorControlButtonStyle()).opacity(enabled ? 1 : 0.36)
            // An asynchronous rejection can arrive after text entry closes.
            // Keep compact errors visible without expanding the layer header.
            .overlay {
                if inline && field.error != nil {
                    RoundedRectangle(cornerRadius: 6).stroke(.red, lineWidth: 1).allowsHitTesting(false)
                }
            }
            .modifier(NumberControlMeasurement(id: key + ":value"))
            .accessibilityLabel(label).accessibilityValue(formatted["text"].string)
            .accessibilityHint(field.error ?? "").help(field.error ?? label)
            .accessibilityIdentifier("number-value-" + key)
    }
    private var sliderTrack: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                Capsule().fill(palette["input"])
                Rectangle().fill(palette["panel"])
                    .overlay(palette["text"].opacity(0.5))
                    .frame(width: max(0, geometry.size.width * formatted["fill"].number))
            }.frame(height: 4).clipShape(Capsule()).frame(maxHeight: .infinity)
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
        }.frame(height: 24).modifier(NumberControlMeasurement(id: key + ":track"))
            .accessibilityElement().accessibilityLabel(label)
            .accessibilityValue(formatted["text"].string)
            .accessibilityAdjustableAction { direction in
                switch direction {
                case .increment: step(1)
                case .decrement: step(-1)
                @unknown default: break
                }
            }.accessibilityIdentifier("number-track-" + key)
    }
    private var numericEntry: some View {
        NumericTextField(label: label,
            text: Binding(get: { field.text }, set: { field.text = $0; field.dirty = true }),
            focused: $editing, fontSize: max(1, store.catalog["text_size_pt"].number * 4 / 3),
            color: palette["text"], identifier: "number-entry-" + key,
            submit: finish, cancel: cancel, step: step)
            .frame(width: valueOnly || inline ? nil : slider ? 80 : 48)
            .padding(.horizontal, 6).frame(height: valueOnly || slider ? 24 : 32)
            .background(palette["input"], in: RoundedRectangle(cornerRadius: 6))
            .modifier(NumberControlMeasurement(id: key + ":entry"))
            .accessibilityHint(field.error ?? "")
            .help(field.error ?? label)
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(field.error == nil ? Color.clear : Color.red, lineWidth: 1)
                .allowsHitTesting(false))
            .onAppear { if showsEntry { editing = true } }
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
        let active = enabled && (direction < 0 ? field.value > control["min"].number : field.value < control["max"].number)
        return Button { step(direction) } label: {
            SharedIcon(name: direction < 0 ? "minus" : "plus").frame(width: slider ? 24 : 32, height: slider ? 24 : 32)
                .contentShape(Rectangle())
        }.buttonStyle(EditorControlButtonStyle()).opacity(active ? 1 : 0.36)
            .modifier(NumberControlMeasurement(id: key + (direction < 0 ? ":minus" : ":plus")))
            .disabled(!active)
            .accessibilityLabel((direction < 0 ? "Decrease " : "Increase ") + label)
            .accessibilityIdentifier("number-\(direction < 0 ? "decrease" : "increase")-" + key)
    }
    private func format() {
        do {
            formatted = try store.resolveNumber(control, value: field.value, operation: ["type": "format"])
            if !field.dirty && !editing { field.text = formatted[slider ? "edit" : "text"].string }
        } catch { field.error = error.localizedDescription }
    }
    private func measureInlineRange() {
        guard inline else { return }
        do {
            let minimum = try store.resolveNumber(control, value: control["min"].number, operation: ["type": "format"])["text"].string
            let maximum = try store.resolveNumber(control, value: control["max"].number, operation: ["type": "format"])["text"].string
            inlineMeasure = (minimum.count >= maximum.count ? minimum : maximum).map { $0.isNumber ? "8" : String($0) }.joined()
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

/// Reserve the formatted range's widest digit string in readout mode. Text
/// entry additionally reserves its initial character count, like input.size.
private struct InlineNumberValueLayout: Layout {
    let entrySize: Int?
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard subviews.count == 3 else { return .zero }
        let measure = subviews[0].sizeThatFits(.unspecified).width
        let entry = entrySize.map { CGFloat($0) * ceil(subviews[1].sizeThatFits(.unspecified).width) + 12 } ?? 0
        return CGSize(width: max(measure, entry), height: 24)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for view in subviews {
            view.place(at: bounds.origin, anchor: .topLeading,
                proposal: ProposedViewSize(width: bounds.width, height: bounds.height))
        }
    }
}

// Disabled in ordinary editors; direct component captures opt in without
// opening a window or relying on platform accessibility traversal.
private struct MeasureNumberControls: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var measureNumberControls: Bool {
        get { self[MeasureNumberControls.self] }
        set { self[MeasureNumberControls.self] = newValue }
    }
}
struct NumberControlFrames: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue()) { _, new in new }
    }
}
private struct NumberControlMeasurement: ViewModifier {
    let id: String
    @Environment(\.measureNumberControls) private var enabled
    func body(content: Content) -> some View {
        if enabled {
            content.background(GeometryReader { proxy in
                Color.clear.preference(key: NumberControlFrames.self,
                    value: [id: proxy.frame(in: .named("number-capture"))])
            })
        } else { content }
    }
}
