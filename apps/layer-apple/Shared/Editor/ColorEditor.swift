import SwiftUI

struct ManagedColorButton: View {
    let label: String
    let identifier: String
    let value: JSON
    let documentSpace: String
    var viewing = JSON()
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
                .help(preview["in_gamut"].bool ? label : "Outside the Display P3 preview gamut. The stored color is preserved.")
                .sheet(isPresented: $editing) {
                    ColorEditor(value: value, documentSpace: documentSpace, viewing: viewing) { change($0); editing = false }
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
    let hdrUse: ((JSON, Double) -> Void)?
    let viewing: JSON
    @State private var intensityText: String
    init(value: JSON, documentSpace: String, intensity: Double? = nil, viewing: JSON = JSON(), hdrUse: ((JSON, Double) -> Void)? = nil, use: @escaping (JSON) -> Void) {
        _form = State(initialValue: ColorUI.resolve(["type": "form", "request": [
            "color": value.raw, "document_space": documentSpace, "display_space": "DisplayP3", "document_depth": viewing["document_depth"].raw, "model": viewing["hdr"].bool ? "linear_rgb" : "document_rgb", "rendition": viewing["recipe"].raw, "intensity": intensity as Any? ?? NSNull()]]))
        self.use = use; self.hdrUse = hdrUse; self.viewing = viewing
        _intensityText = State(initialValue: intensity.map { String(format: "%.2f", $0) } ?? "")
    }
    private func update(_ draft: JSON) { form = ColorUI.resolve(["type": "form", "request": draft.raw]) }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Edit Color").font(.headline)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Text(form["description"].string)
                    if !form["preview"].isNull {
                        if !form["draft"]["intensity"].isNull {
                            HStack(spacing: 0) {
                                VStack(spacing: 4) { Text("Base").font(.caption); HDRColorSwatch(color: form["base"], viewing: viewing) }
                                VStack(spacing: 4) { Text("Adjusted").font(.caption); HDRColorSwatch(color: form["value"], viewing: viewing) }
                            }.frame(height: 68)
                            HStack {
                                Text("Intensity (EV)")
                                TextField("Intensity (EV)", text: $intensityText).textFieldStyle(.roundedBorder)
                                    .accessibilityIdentifier("color-input-intensity")
                                    .onChange(of: intensityText) { _, value in update(form["draft"].replacing("change_intensity_text", with: JSON(value))) }
                            }
                        } else { ColorSwatch(rgba: form["preview"]["rgba"]).frame(height: 48).accessibilityHidden(true) }
                        if form["draft"]["intensity"].isNull && !form["preview"]["in_gamut"].bool {
                            Text("Outside the Display P3 preview gamut. The stored color is preserved.").font(.caption)
                        }
                    }
                    let models = form["models"].array
                    FormPicker("Color model", selection: Binding(get: { form["draft"]["model"].string }, set: {
                        update(form["draft"].replacing("change_model", with: JSON($0)))
                    })) {
                        ForEach(models.indices, id: \.self) { i in Text(models[i][1].string).tag(models[i][0].string) }
                    }.accessibilityIdentifier("color-input-model")
                    VStack(spacing: 0) {
                    ForEach(0..<4, id: \.self) { i in
                        if !form["labels"][i].string.isEmpty {
                            HStack {
                                Text(form["labels"][i].string).frame(maxWidth: .infinity, alignment: .leading)
                                TextField(form["labels"][i].string, text: Binding(get: { form["draft"]["fields"][i].string }, set: { text in
                                    var fields = form["draft"]["fields"].array.map(\.string)
                                    guard fields.indices.contains(i) else { return }
                                    fields[i] = text
                                    update(form["draft"].replacing("fields", with: JSON(fields)))
                                })).textFieldStyle(.plain).multilineTextAlignment(.trailing).autocorrectionDisabled()
                                    .accessibilityIdentifier("color-input-\(i)")
                            }.padding(10)
                            if i < 3 { Divider() }
                        }
                    }
                    }.background(.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
                    if !form["validation"].isNull { Text(form["validation"].string).font(.caption) }
                    if !form["error"].isNull { Text(form["error"].string).foregroundStyle(.red).accessibilityIdentifier("color-input-error") }
                }
            }
            HStack {
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Use Color") {
                    if let hdrUse, !form["draft"]["intensity"].isNull { hdrUse(form["value"], form["draft"]["intensity"].number) }
                    else { use(form["value"]) }
                }.disabled(form["value"].isNull || !form["error"].isNull)
                    .keyboardShortcut(.defaultAction).accessibilityIdentifier("color-input-use")
            }
        }.onAppear { if intensityText.isEmpty && !form["draft"]["intensity"].isNull { intensityText = String(format: "%.2f", form["draft"]["intensity"].number) } }
        .padding(20).frame(minWidth: 320, idealWidth: 380, maxWidth: 460, minHeight: 420, idealHeight: 520)
    }
}
