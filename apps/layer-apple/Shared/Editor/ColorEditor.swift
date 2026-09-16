import SwiftUI

struct ManagedColorButton: View {
    let label: String
    let identifier: String
    let value: JSON
    let documentSpace: String
    let change: (JSON) -> Void
    @State private var editing = false
    var body: some View {
        let preview = ColorUI.preview(value)
        HStack {
            Text(label).frame(maxWidth: .infinity, alignment: .leading)
            Button { editing = true } label: {
                ColorSwatch(rgba: preview["rgba"]).frame(width: 48, height: 28)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(.primary.opacity(0.3), lineWidth: 1))
            }.buttonStyle(.plain).accessibilityLabel(label).accessibilityIdentifier(identifier + "-color")
                .help(preview["in_gamut"].bool ? label : "Outside the sRGB preview gamut. The stored color is preserved.")
                .sheet(isPresented: $editing) {
                    ColorEditor(value: value, documentSpace: documentSpace) { change($0); editing = false }
                }
        }
    }
}

/// The shared form retains the original tagged value across readout changes.
/// Only Use Color publishes an edit; invalid drafts and cancellation do not.
struct ColorEditor: View {
    @Environment(\.dismiss) private var dismiss
    @State private var form: JSON
    let use: (JSON) -> Void
    init(value: JSON, documentSpace: String, use: @escaping (JSON) -> Void) {
        _form = State(initialValue: ColorUI.resolve(["type": "form", "request": [
            "color": value.raw, "document_space": documentSpace, "display_space": "Srgb", "model": "document_rgb"]]))
        self.use = use
    }
    private func update(_ draft: JSON) { form = ColorUI.resolve(["type": "form", "request": draft.raw]) }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Edit Color").font(.headline)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Text(form["description"].string)
                    let models = form["models"].array
                    Picker("Color model", selection: Binding(get: { form["draft"]["model"].string }, set: {
                        update(form["draft"].replacing("change_model", with: JSON($0)))
                    })) {
                        ForEach(models.indices, id: \.self) { i in Text(models[i][1].string).tag(models[i][0].string) }
                    }.accessibilityIdentifier("color-input-model")
                    if !form["preview"].isNull {
                        ColorSwatch(rgba: form["preview"]["rgba"]).frame(height: 48).accessibilityHidden(true)
                        if !form["preview"]["in_gamut"].bool {
                            Text("Outside the sRGB preview gamut. The stored color is preserved.").font(.caption)
                        }
                    }
                    ForEach(0..<4, id: \.self) { i in
                        if !form["labels"][i].string.isEmpty {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(form["labels"][i].string)
                                TextField(form["labels"][i].string, text: Binding(get: { form["draft"]["fields"][i].string }, set: { text in
                                    var fields = form["draft"]["fields"].array.map(\.string)
                                    guard fields.indices.contains(i) else { return }
                                    fields[i] = text
                                    update(form["draft"].replacing("fields", with: JSON(fields)))
                                })).textFieldStyle(.roundedBorder).autocorrectionDisabled()
                                    .accessibilityIdentifier("color-input-\(i)")
                            }
                        }
                    }
                    if !form["error"].isNull { Text(form["error"].string).foregroundStyle(.red).accessibilityIdentifier("color-input-error") }
                }
            }
            HStack {
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Use Color") { use(form["value"]) }.disabled(form["value"].isNull || !form["error"].isNull)
                    .keyboardShortcut(.defaultAction).accessibilityIdentifier("color-input-use")
            }
        }.padding(20).frame(minWidth: 320, idealWidth: 380, maxWidth: 460, minHeight: 420, idealHeight: 520)
    }
}
