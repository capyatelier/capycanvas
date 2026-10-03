import SwiftUI

struct SizeDialogs: ViewModifier {
    @ObservedObject var store: EditorStore
    private var tools: JSON { store.state["layer_tools"] }
    func body(content: Content) -> some View {
        content.sheet(isPresented: presented("canvas_size")) {
            SizeForm(store: store, kind: .canvas).presentationSizing(.fitted).modifier(EditorPopupPresentation())
        }.sheet(isPresented: presented("image_size")) {
            SizeForm(store: store, kind: .image).presentationSizing(.fitted).modifier(EditorPopupPresentation())
        }
    }
    private func presented(_ type: String) -> Binding<Bool> {
        Binding(get: { !tools[type].isNull }, set: { if !$0 && !tools[type].isNull { store.dispatch(["type": type, "action": ["op": "cancel"]]) } })
    }
}

private enum SizeKind: String {
    case canvas = "canvas_size", image = "image_size"
    var identifier: String { rawValue.replacingOccurrences(of: "_", with: "-") }
}

private struct SizeForm: View {
    @ObservedObject var store: EditorStore
    let kind: SizeKind
    @State private var admissions: [String: (Bool) -> Bool] = [:]
    private var view: JSON { store.state["layer_tools"][kind.rawValue] }
    private func send(_ action: [String: Any]) { store.dispatch(["type": kind.rawValue, "action": action]) }
    private func finish(_ discard: Bool) -> Bool {
        let results = admissions.values.map { $0(discard) }
        return results.allSatisfy { $0 }
    }
    private func choose(_ action: [String: Any]) {
        guard finish(false) else { return }
        send(action)
    }
    var body: some View {
        let palette = EditorPalette(source: store.state["palette"])
        VStack(alignment: .leading, spacing: 12) {
            Text(view["title"].string).font(.headline)
            ForEach(["width", "height"].indices, id: \.self) { axis in
                let op = axis == 0 ? "width" : "height"
                number(view["labels"][axis].string, value: view["values"][axis].number, control: view["numeric"][axis], op: op)
            }
            Picker("Unit", selection: Binding(get: { view["unit"].string }, set: { choose(["op": "unit", "unit": $0]) })) {
                ForEach(view["units"].array.indices, id: \.self) { index in
                    Text(view["units"][index]["label"].string).tag(view["units"][index]["unit"].string)
                }
            }.pickerStyle(.segmented).labelsHidden().accessibilityIdentifier(kind.identifier + "-unit")
            switch kind {
            case .canvas: canvasChoices(palette)
            case .image: imageChoices
            }
            Text(view["message"].string).foregroundStyle(palette["text"].opacity(0.7))
                .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier(kind.identifier + "-message")
            HStack {
                Spacer()
                Button(view["cancel_label"].string) { _ = finish(true); send(["op": "cancel"]) }.keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier(kind.identifier + "-cancel")
                Button(view["apply_label"].string) { choose(["op": "apply"]) }.keyboardShortcut(.defaultAction).disabled(!view["can_apply"].bool)
                    .accessibilityIdentifier(kind.identifier + "-apply")
            }
        }.padding(20).frame(width: 380)
    }
    private func number(_ label: String, value: Double, control: JSON, op: String) -> some View {
        NumberControl(store: store, label: label, value: value, control: control, identifier: kind.identifier + "-" + op, entryWidth: 96,
            registerAdmission: { key, admission in admissions[key] = admission }) { value, completion in
            send(["op": op, "value": value]); completion(nil)
        }.id(control.stableKey)
    }
    @ViewBuilder private var imageChoices: some View {
        number(view["resolution_label"].string, value: view["resolution"].number, control: view["resolution_numeric"], op: "resolution")
        Toggle(view["constrain_label"].string, isOn: Binding(get: { view["constrain"].bool }, set: { choose(["op": "constrain", "constrain": $0]) }))
            .accessibilityIdentifier("image-size-constrain")
        Picker(view["resample_label"].string, selection: Binding(get: { view["resample"].string }, set: { choose(["op": "resample", "resample": $0]) })) {
            ForEach(view["resamples"].array.indices, id: \.self) { index in
                Text(view["resamples"][index]["label"].string).tag(view["resamples"][index]["resample"].string)
            }
        }.pickerStyle(.menu).accessibilityIdentifier("image-size-resample")
    }
    @ViewBuilder private func canvasChoices(_ palette: EditorPalette) -> some View {
        Toggle(view["relative_label"].string, isOn: Binding(get: { view["relative"].bool }, set: { choose(["op": "relative", "relative": $0]) }))
            .accessibilityIdentifier("canvas-size-relative")
        HStack(alignment: .top) {
            Text(view["anchor_label"].string)
            Spacer()
            Grid(horizontalSpacing: 2, verticalSpacing: 2) {
                ForEach(0..<3, id: \.self) { row in
                    GridRow {
                        ForEach(0..<3, id: \.self) { column in anchor(view["anchors"][row * 3 + column], palette: palette) }
                    }
                }
            }
        }
    }
    private func anchor(_ choice: JSON, palette: EditorPalette) -> some View {
        let selected = choice["anchor"].string == view["anchor"].string
        return Button { choose(["op": "anchor", "anchor": choice["anchor"].string]) } label: {
            SharedIcon(name: "rectangle-fill", size: 16).opacity(selected ? 1 : 0).frame(width: 40, height: 40)
                .background(selected ? palette.active : palette["input"], in: SquircleShape.control).contentShape(Rectangle())
        }.buttonStyle(.plain).focusable(false)
            .accessibilityLabel(choice["label"].string).accessibilityAddTraits(selected ? .isSelected : [])
            .accessibilityIdentifier("canvas-size-anchor-" + choice["anchor"].string)
    }
}
