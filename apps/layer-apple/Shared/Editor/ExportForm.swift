import SwiftUI

struct ExportForm: View {
    @ObservedObject var editor: ExportController
    @State private var fit = false
    @State private var enlarge = false
    @State private var width = "2048"
    @State private var height = "2048"
    @State private var resolution = "Master"
    @State private var ppi = "300"
    @State private var quality = "90"
    @State private var presetName = ""
    @State private var readingProfile = false
    private var draft: [String] { [String(fit), String(enlarge), width, height, resolution, ppi, quality] }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Export image").font(.headline)
            Text("Export a profiled copy. The editable drawing stays unchanged.")
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    if editor.loaded {
                        choices.disabled(editor.busy || readingProfile)
                        Button("Preview Output") { withRecipe(editor.preview) }
                            .disabled(editor.busy || readingProfile).accessibilityIdentifier("export-preview")
                    }
                    if editor.busy { ProgressView("Preparing export…") }
                    ForEach(Array(editor.previews.enumerated()), id: \.offset) { index, image in
                        Text(index == 0 ? "Artwork" : "Output").font(.headline)
                        Image(decorative: image, scale: 1).resizable().aspectRatio(contentMode: .fit)
                            .frame(maxWidth: .infinity, maxHeight: 180)
                            .accessibilityLabel(index == 0 ? "Artwork preview" : "Output preview")
                    }
                    if !editor.previews.isEmpty {
                        Text("sRGB preview of output size, profile, precision and transparency. JPEG compression artifacts are not previewed.").font(.caption)
                        Text("Output: \(editor.details["output_extent"][0].uint) × \(editor.details["output_extent"][1].uint) pixels")
                            .accessibilityIdentifier("export-output-size")
                    }
                    if editor.clipped > 0 { Text("Some colors exceed the output gamut and will be clipped.") }
                    if let error = editor.error { Text(error).foregroundStyle(.red).accessibilityIdentifier("export-error") }
                    if !editor.loaded && !editor.busy {
                        Button("Retry") { editor.load() }
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack {
                Button("Cancel") { editor.cancel() }.keyboardShortcut(.cancelAction).disabled(readingProfile)
                    .accessibilityIdentifier("export-cancel")
                Spacer()
                Button("Choose File…") { withRecipe(editor.choose) }.keyboardShortcut(.defaultAction)
                    .disabled(!editor.loaded || editor.busy || readingProfile).accessibilityIdentifier("export-choose-file")
            }
        }.padding(24).frame(minWidth: 340, idealWidth: 520, maxWidth: 620, minHeight: 320, idealHeight: 660, maxHeight: 760)
            .onChange(of: editor.choiceRevision, initial: true) { _, _ in restoreFields() }
            .onChange(of: draft) { _, _ in editor.invalidate() }
            .interactiveDismissDisabled(readingProfile)
    }
    private var choices: some View {
        VStack(alignment: .leading, spacing: 12) {
            Picker("Destination", selection: Binding(get: { editor.destination }, set: {
                editor.preference(JSON(["type": "get", "index": $0]))
            })) {
                ForEach(editor.names.indices, id: \.self) { Text(editor.names[$0]).tag($0) }
            }.accessibilityIdentifier("export-destination")
            Picker("Format", selection: Binding(get: { editor.recipe["format"].string }, set: { format in
                editor.change("format", JSON(format))
                if format == "Jpeg" {
                    editor.change("depth", JSON("U8"))
                    if editor.recipe["background"].string == "Preserve" { editor.change("background", JSON("White")) }
                }
            })) { Text("PNG").tag("Png"); Text("TIFF").tag("Tiff"); Text("JPEG").tag("Jpeg") }
                .accessibilityIdentifier("export-format")
            Picker("Output profile", selection: Binding(get: { editor.profileIndex }, set: editor.selectProfile)) {
                ForEach(editor.profiles.indices, id: \.self) { Text(editor.profiles[$0]["name"].string).tag($0) }
            }.accessibilityIdentifier("export-profile")
            ProfileChooserButtons(preferences: editor.preferences, busy: $readingProfile, onProfile: editor.imported)
            Picker("Bit depth", selection: Binding(get: { editor.recipe["depth"].string }, set: { depth in
                editor.change("depth", JSON(depth))
                if depth == "U16" { encoding("dither", "None") }
            })) { Text("8-bit").tag("U8"); Text("16-bit").tag("U16") }
                .disabled(editor.recipe["format"].string == "Jpeg").accessibilityIdentifier("export-depth")
            Picker("Transparency", selection: choice("background")) {
                if editor.recipe["format"].string != "Jpeg" { Text("Preserve").tag("Preserve") }
                Text("White background").tag("White"); Text("Black background").tag("Black")
            }.accessibilityIdentifier("export-background")
            if editor.recipe["format"].string == "Jpeg" { number("JPEG quality (1–100)", $quality, id: "export-quality") }
            Toggle("Fit within pixel size", isOn: $fit).accessibilityIdentifier("export-fit")
            if fit {
                number("Maximum width", $width, id: "export-width")
                number("Maximum height", $height, id: "export-height")
                Toggle("Allow enlargement", isOn: $enlarge)
            }
            DisclosureGroup("Advanced") {
                VStack(alignment: .leading, spacing: 12) {
                    Picker("Rendering intent", selection: Binding(get: { editor.recipe["encoding"]["conversion"]["intent"].string }, set: { intent in
                        let conversion = editor.recipe["encoding"]["conversion"].replacing("intent", with: JSON(intent))
                        editor.change("encoding", editor.recipe["encoding"].replacing("conversion", with: conversion))
                    })) {
                        Text("Relative colorimetric").tag("RelativeColorimetric"); Text("Perceptual").tag("Perceptual")
                        Text("Saturation").tag("Saturation"); Text("Absolute colorimetric").tag("AbsoluteColorimetric")
                    }
                    Picker("Dither", selection: Binding(get: { editor.recipe["encoding"]["dither"].string }, set: { encoding("dither", $0) })) {
                        Text("None").tag("None"); Text("Stochastic (8-bit output)").tag("Stochastic8")
                    }.disabled(editor.recipe["depth"].string != "U8")
                    Picker("Resolution metadata", selection: $resolution) {
                        Text("Keep original").tag("Master"); Text("Pixels per inch").tag("Ppi"); Text("Omit").tag("Omit")
                    }
                    if resolution == "Ppi" { number("Pixels per inch", $ppi, id: "export-ppi") }
                }.padding(.top, 8)
            }
            DisclosureGroup("Saved presets") {
                VStack(alignment: .leading, spacing: 8) {
                    TextField("Preset name", text: $presetName).textFieldStyle(.roundedBorder).accessibilityIdentifier("export-preset-name")
                    HStack {
                        Button("Save Preset") { withRecipe { editor.preference(JSON(["type": "save", "name": presetName, "recipe": $0.raw])) } }
                            .accessibilityIdentifier("export-preset-save")
                        Button("Update Preset") { withRecipe { editor.preference(JSON(["type": "update", "index": editor.destination, "recipe": $0.raw])) } }
                            .disabled(editor.destination < 4)
                    }
                    HStack {
                        Button("Delete Preset", role: .destructive) { editor.preference(JSON(["type": "remove", "index": editor.destination])) }
                            .disabled(editor.destination < 4)
                        Button("Reset Destination") { editor.preference(JSON(["type": "reset", "index": editor.destination])) }
                            .disabled(editor.destination >= 4)
                    }
                }.padding(.top, 8).disabled(!editor.preferences.canSave)
            }
        }
    }
    private func choice(_ key: String) -> Binding<String> {
        Binding(get: { editor.recipe[key].string }, set: { editor.change(key, JSON($0)) })
    }
    private func encoding(_ key: String, _ value: String) {
        editor.change("encoding", editor.recipe["encoding"].replacing(key, with: JSON(value)))
    }
    private func number(_ label: String, _ binding: Binding<String>, id: String) -> some View {
        HStack {
            Text(label); Spacer()
            TextField(label, text: binding).textFieldStyle(.roundedBorder).frame(width: 120)
                .accessibilityLabel(label).accessibilityIdentifier(id)
        }
    }
    private func selectedRecipe() throws -> JSON {
        func integer(_ text: String, _ label: String) throws -> Int {
            guard let value = Int(text.trimmingCharacters(in: .whitespacesAndNewlines)) else {
                throw HostFailure(message: "Enter a whole number for \(label).")
            }
            return value
        }
        return try editor.recipe.replacing("jpeg_quality", with: JSON(integer(quality, "JPEG quality")))
            .replacing("size", with: fit ? JSON(["Fit": ["bounds": [integer(width, "width"), integer(height, "height")], "enlarge": enlarge]]) : JSON("Original"))
            .replacing("resolution", with: resolution == "Ppi" ? JSON(["Ppi": integer(ppi, "resolution")]) : JSON(resolution))
    }
    private func withRecipe(_ action: (JSON) -> Void) {
        do { action(try selectedRecipe()) } catch { editor.error = error.localizedDescription }
    }
    private func restoreFields() {
        guard editor.loaded else { return }
        let value = editor.recipe
        quality = String(value["jpeg_quality"].uint)
        fit = !value["size"]["Fit"].isNull; enlarge = value["size"]["Fit"]["enlarge"].bool
        if fit { width = String(value["size"]["Fit"]["bounds"][0].uint); height = String(value["size"]["Fit"]["bounds"][1].uint) }
        resolution = value["resolution"].string.isEmpty ? "Ppi" : value["resolution"].string
        if resolution == "Ppi" { ppi = String(value["resolution"]["Ppi"].uint) }
        presetName = editor.destination >= 4 ? editor.names[editor.destination] : ""
    }
}
