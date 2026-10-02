import SwiftUI

struct CanvasSizeDialog: ViewModifier {
    @ObservedObject var store: EditorStore
    private var view: JSON { store.state["layer_tools"]["canvas_size"] }
    func body(content: Content) -> some View {
        content.sheet(isPresented: Binding(get: { !view.isNull }, set: { if !$0 && !view.isNull { store.canvasSize(["op": "cancel"]) } })) {
            CanvasSizeForm(store: store).presentationSizing(.fitted).modifier(EditorPopupPresentation())
        }
    }
}

private extension EditorStore {
    func canvasSize(_ action: [String: Any]) { dispatch(["type": "canvas_size", "action": action]) }
}

private struct CanvasSizeForm: View {
    @ObservedObject var store: EditorStore
    @State private var admissions: [String: (Bool) -> Bool] = [:]
    private func finish(_ discard: Bool) -> Bool {
        let results = admissions.values.map { $0(discard) }
        return results.allSatisfy { $0 }
    }
    private var view: JSON { store.state["layer_tools"]["canvas_size"] }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(view["title"].string).font(.headline)
            ForEach(["width", "height"].indices, id: \.self) { axis in
                let op = axis == 0 ? "width" : "height"
                NumberControl(store: store, label: view["labels"][axis].string, value: view["values"][axis].number,
                    control: view["numeric"][axis], identifier: "canvas-size-" + op, entryWidth: 96, registerAdmission: { key, admission in admissions[key] = admission }) { value, completion in
                    store.canvasSize(["op": op, "value": value]); completion(nil)
                }.id(view["numeric"][axis].stableKey)
            }
            CanvasSizeChoices(store: store, view: view, finish: finish)
        }.padding(20).frame(width: 380)
    }
}

private struct CanvasSizeChoices: View {
    let store: EditorStore
    let view: JSON
    let finish: (Bool) -> Bool
    private func choose(_ action: [String: Any]) {
        guard finish(false) else { return }
        store.canvasSize(action)
    }
    var body: some View {
        let palette = EditorPalette(source: store.state["palette"])
        Picker("Unit", selection: Binding(get: { view["unit"].string }, set: { choose(["op": "unit", "unit": $0]) })) {
            ForEach(view["units"].array.indices, id: \.self) { index in
                Text(view["units"][index]["label"].string).tag(view["units"][index]["unit"].string)
            }
        }.pickerStyle(.segmented).labelsHidden().accessibilityIdentifier("canvas-size-unit")
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
        Text(view["message"].string).foregroundStyle(palette["text"].opacity(0.7))
            .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("canvas-size-message")
        HStack {
            Spacer()
            Button(view["cancel_label"].string) { _ = finish(true); store.canvasSize(["op": "cancel"]) }.keyboardShortcut(.cancelAction)
                .accessibilityIdentifier("canvas-size-cancel")
            Button(view["apply_label"].string) { choose(["op": "apply"]) }.keyboardShortcut(.defaultAction).disabled(!view["can_apply"].bool)
                .accessibilityIdentifier("canvas-size-apply")
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
