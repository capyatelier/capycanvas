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
    @Environment(\.capyInterfaceLanguage) private var interfaceLanguage
    let spec: JSON
    let error: String?
    let busy: Bool
    let completion: (NewDrawingChoice?) -> Void
    @State private var appearance: JSON
    @State private var options: JSON
    @State private var width: String
    @State private var height: String
    @State private var presetName = ""
    @State private var useAsDefaults = false
    init(spec: JSON, error: String? = nil, busy: Bool = false, completion: @escaping (NewDrawingChoice?) -> Void) {
        self.spec = spec; self.error = error; self.busy = busy; self.completion = completion
        let options = spec["creation"]["options"]
        _options = State(initialValue: options)
        _appearance = State(initialValue: JSON())
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
    private var text: JSON { spec["creation"]["text"] }
    private var preset: Binding<String> {
        Binding(get: { presets.first { $0["options"].stableKey == selectedOptions?.stableKey }?["id"].stableKey ?? "custom" }, set: { id in
            guard let chosen = presets.first(where: { $0["id"].stableKey == id }) else { return }
            options = chosen["options"]
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
                    FormPicker(text["preset"].string, selection: preset) {
                        Text(text["custom"].string).tag("custom")
                        ForEach(presets.indices, id: \.self) { Text(presets[$0]["name"].string).tag(presets[$0]["id"].stableKey) }
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
                    FormPicker(text["background"].string, selection: Binding(get: { options["background"].string }, set: {
                        options = options.replacing("background", with: JSON($0))
                    })) {
                        ForEach(spec["creation"]["backgrounds"].array, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
                    }.accessibilityIdentifier("new-document-background")
                    FormPicker(text["space"].string, selection: color("space")) {
                        ForEach(spec["creation"]["spaces"].array, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
                    }.accessibilityIdentifier("new-document-space")
                    FormPicker(text["depth"].string, selection: color("depth")) {
                        ForEach(spec["creation"]["depths"].array, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
                    }.accessibilityIdentifier("new-document-depth")
                    let blending = spec["creation"]["blending"]
                    FormPicker(blending["label"].string, selection: Binding(get: { appearance["blending"].string }, set: {
                        options = options.replacing("blend_space", with: JSON($0))
                    })) {
                        ForEach(blending["choices"].array, id: \.stableKey) { Text($0["label"].string).tag($0["id"].string) }
                    }.disabled(!appearance["blending_editable"].bool).accessibilityIdentifier("new-document-blending")
                    Text(appearance["blending_help"].string).font(.caption).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier("new-document-blending-note")
                    if !appearance["note"].isNull {
                        Text(appearance["note"].string).font(.caption)
                    }
                    Divider()
                    TextField(text["save_preset"].string, text: $presetName).textFieldStyle(.roundedBorder)
                        .accessibilityIdentifier("new-document-preset-name")
                    Toggle(text["use_defaults"].string, isOn: $useAsDefaults).accessibilityIdentifier("new-document-defaults")
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
        }.onAppear { appearance = NativeTextContext.appearance(options, language: interfaceLanguage) }
            .onChange(of: interfaceLanguage) { _, _ in appearance = NativeTextContext.appearance(options, language: interfaceLanguage) }
            .onChange(of: options.stableKey) { _, _ in appearance = NativeTextContext.appearance(options, language: interfaceLanguage) }.disabled(busy).padding(24).frame(minWidth: 320, idealWidth: 400, maxWidth: 500,
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
