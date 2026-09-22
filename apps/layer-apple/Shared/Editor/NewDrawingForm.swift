import SwiftUI
#if os(iOS)
import UIKit
#endif

struct NewDrawingChoice {
    let options: JSON
    let presetName: String
    let useAsDefaults: Bool
}

/// Native controls present shared presets and independent space/depth choices.
/// Rust validates the candidate document and any saved creation settings.
struct NewDrawingForm: View {
    let spec: JSON
    let error: String?
    let busy: Bool
    let completion: (NewDrawingChoice?) -> Void
    @State private var options: JSON
    @State private var width: String
    @State private var height: String
    @State private var presetName = ""
    @State private var useAsDefaults = false
    init(spec: JSON, error: String? = nil, busy: Bool = false, completion: @escaping (NewDrawingChoice?) -> Void) {
        self.spec = spec; self.error = error; self.busy = busy; self.completion = completion
        let options = spec["creation"]["options"]
        _options = State(initialValue: options)
        _width = State(initialValue: String(options["extent"][0].uint))
        _height = State(initialValue: String(options["extent"][1].uint))
    }
    private var selectedOptions: JSON? {
        guard let width = UInt32(width.trimmingCharacters(in: .whitespacesAndNewlines)),
            let height = UInt32(height.trimmingCharacters(in: .whitespacesAndNewlines)),
            [width, height].allSatisfy({ $0 >= spec["minimum"].uint && $0 <= spec["maximum"].uint }) else { return nil }
        return options.replacing("extent", with: JSON([width, height]))
    }
    private var presets: [JSON] { spec["creation"]["presets"].array }
    private var preset: Binding<Int> {
        Binding(get: { presets.firstIndex { $0["options"].stableKey == selectedOptions?.stableKey } ?? -1 }, set: { index in
            guard presets.indices.contains(index) else { return }
            options = presets[index]["options"]
            width = String(options["extent"][0].uint); height = String(options["extent"][1].uint)
        })
    }
    private func color(_ field: String) -> Binding<String> {
        Binding(get: { options["color"][field].string }, set: {
            options = options.replacing("color", with: options["color"].replacing(field, with: JSON($0)))
        })
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text(spec["title"].string).font(.headline)
            EditorScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    FormPicker("Preset", selection: preset) {
                        Text("Custom").tag(-1)
                        ForEach(presets.indices, id: \.self) { Text(presets[$0]["name"].string).tag($0) }
                    }.accessibilityIdentifier("new-document-preset")
                    Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 12) {
                        GridRow {
                            Text(spec["labels"][0].string)
                            dimension($width, label: spec["labels"][0].string, id: "new-document-width")
                        }
                        GridRow {
                            Text(spec["labels"][1].string)
                            dimension($height, label: spec["labels"][1].string, id: "new-document-height")
                        }
                    }
                    FormPicker("Background", selection: Binding(get: { options["background"].string }, set: {
                        options = options.replacing("background", with: JSON($0))
                    })) {
                        Text("White").tag("White"); Text("Transparent").tag("Transparent")
                    }.accessibilityIdentifier("new-document-background")
                    FormPicker("Color space", selection: color("space")) {
                        ForEach(spec["creation"]["spaces"].array, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
                    }.accessibilityIdentifier("new-document-space")
                    FormPicker("Bit depth", selection: color("depth")) {
                        Text("8-bit SDR").tag("U8"); Text("16-bit SDR").tag("U16"); Text("16-bit float HDR").tag("F16"); Text("32-bit float HDR").tag("F32")
                    }.accessibilityIdentifier("new-document-depth")
                    if options["color"]["space"].string == "ProPhoto" && options["color"]["depth"].string == "U8" {
                        Text("16-bit is recommended for ProPhoto's wider color range.").font(.caption)
                    }
                    Divider()
                    TextField("Save as preset (optional)", text: $presetName).textFieldStyle(.roundedBorder)
                        .accessibilityIdentifier("new-document-preset-name")
                    Toggle("Use as defaults", isOn: $useAsDefaults).accessibilityIdentifier("new-document-defaults")
                    if let error { Text(error).foregroundStyle(.red).accessibilityIdentifier("new-document-error") }
                }
            }
            HStack {
                Spacer()
                Button(spec["cancel"].string, role: .cancel) { completion(nil) }
                    .keyboardShortcut(.cancelAction).accessibilityIdentifier("new-document-cancel")
                Button(spec["accept"].string, action: create)
                    .keyboardShortcut(.defaultAction).disabled(selectedOptions == nil)
                    .accessibilityIdentifier("new-document-create")
            }
        }.disabled(busy).padding(24).frame(minWidth: 320, idealWidth: 400, maxWidth: 500,
            minHeight: 440, idealHeight: 540)
    }
    private func create() {
        #if os(iOS)
        // UIKit can still hold marked text when a button closes the form.
        // Commit it before reading SwiftUI's bound preset name and dimensions.
        UIApplication.shared.sendAction(#selector(UITextInput.unmarkText), to: nil, from: nil, for: nil)
        UIApplication.shared.sendAction(#selector(UIResponder.resignFirstResponder), to: nil, from: nil, for: nil)
        DispatchQueue.main.async { completeCreation() }
        #else
        completeCreation()
        #endif
    }
    private func completeCreation() {
        if let options = selectedOptions {
            completion(NewDrawingChoice(options: options, presetName: presetName, useAsDefaults: useAsDefaults))
        }
    }
    private func dimension(_ value: Binding<String>, label: String, id: String) -> some View {
        TextField("", text: value).textFieldStyle(.roundedBorder)
            .accessibilityLabel(label).accessibilityIdentifier(id)
            #if os(iOS)
            .keyboardType(.numberPad)
            #endif
    }
}
