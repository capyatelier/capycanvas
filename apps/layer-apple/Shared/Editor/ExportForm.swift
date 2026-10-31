import SwiftUI

struct ExportForm: View {
    @ObservedObject var editor: ExportController
    @Environment(\.capyInterfaceLanguage) private var language
    private var copy: JSON { editor.copy }
    @State private var fit = false
    @State private var enlarge = false
    @State private var width = "2048"
    @State private var height = "2048"
    @State private var resolution = "Master"
    @State private var ppi = "300"
    @State private var quality = "90"
    @State private var presetName = ""
    @State private var showSDRBase = false
    private var format: String { editor.draft["format"].string }
    private var gainmap: Bool { format.hasPrefix("JpegHdr") || format.hasPrefix("AvifHdr") }
    private var floating: Bool { ["F16", "F32"].contains(editor.details["color"]["depth"].string) }
    @State private var readingProfile = false
    private var hdr: Bool { editor.draft["hdr"].bool }
    private var draft: [String] { [String(fit), String(enlarge), width, height, resolution, ppi, quality] }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(copy["title"].string).font(.headline)
            Text(copy["help"].string)
            EditorScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    if editor.loaded {
                        choices.disabled(editor.busy || readingProfile)
                        Button(copy["preview"].string) { withRecipe(editor.preview) }
                            .disabled(editor.busy || readingProfile).accessibilityIdentifier("export-preview")
                    }
                    if editor.busy { ProgressView(copy["preparing"].string) }
                    if editor.previews.count == 3 {
                        FormPicker(copy["preview_rendition"].string, selection: $showSDRBase) {
                            Text(copy["hdr_preview"].string).tag(false)
                            Text(copy["sdr_base"].string).tag(true)
                        }.accessibilityIdentifier("export-preview-rendition")
                    }
                    ForEach(0..<min(2, editor.previews.count), id: \.self) { index in
                        Text(index == 0 ? copy["artwork"].string :
                            gainmap ? (showSDRBase ? copy["sdr_base"].string : copy["hdr_preview"].string) :
                            copy["output"].string).font(.headline)
                        Image(decorative: editor.previews[index == 1 && showSDRBase && editor.previews.count == 3 ? 2 : index], scale: 1)
                            .resizable().aspectRatio(contentMode: .fit).frame(maxWidth: .infinity, maxHeight: 180)
                            .accessibilityLabel(index == 0 ? copy["artwork_preview"].string : copy["output_preview"].string)
                    }
                    if !editor.previews.isEmpty {
                        Text(copy[gainmap ? "preview_gainmap" : format == "Exr" ? "preview_exr" : hdr ? "preview_pq" : "preview_sdr"].string).font(.caption)
                        Text(editor.outputSize)
                            .accessibilityIdentifier("export-output-size")
                    }
                    if editor.clipped > 0 { Text(copy[editor.details["range_blocked"].bool ? "outside_range" : "outside_gamut"].string) }
                    if let error = editor.error { Text(error).foregroundStyle(.red).accessibilityIdentifier("export-error") }
                    if !editor.loaded && !editor.busy {
                        Button(copy["retry"].string) { editor.load() }
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack {
                Button(copy["common"]["cancel"].string) { editor.cancel() }.keyboardShortcut(.cancelAction).disabled(readingProfile)
                    .accessibilityIdentifier("export-cancel")
                Spacer()
                Button(copy["choose_file"].string) { withRecipe(editor.choose) }.keyboardShortcut(.defaultAction)
                    .disabled(!editor.loaded || editor.busy || readingProfile).accessibilityIdentifier("export-choose-file")
            }
        }.padding(24).frame(minWidth: 340, idealWidth: 520, maxWidth: 620, minHeight: 320, idealHeight: 660, maxHeight: 760)
            .onChange(of: language, initial: true) { _, _ in editor.refreshCopy() }
            .onChange(of: editor.choiceRevision, initial: true) { _, _ in restoreFields() }
            .onChange(of: draft) { _, _ in editor.invalidate() }
            .interactiveDismissDisabled(readingProfile)
    }
    private var choices: some View {
        VStack(alignment: .leading, spacing: 12) {
            FormPicker(copy["destination"].string, selection: Binding(get: { editor.destination }, set: {
                editor.preference(JSON(["type": "get", "index": $0]))
            })) {
                ForEach(editor.names.indices, id: \.self) { Text(editor.names[$0]).tag($0) }
            }.accessibilityIdentifier("export-destination")
            if floating {
                FormPicker(copy["range"].string, selection: Binding(get: { hdr ? format : "sdr" }, set: {
                    editor.change("format", JSON($0 == "sdr" ? "Png" : $0))
                })) {
                    Text(copy["sdr_rendition"].string).tag("sdr")
                    Text(copy["jpeg_gainmap"].string).tag("JpegHdr")
                    Text(copy["avif_gainmap"].string).tag("AvifHdr")
                    Text(copy["format_pq"].string).tag("PngHdr")
                    Text(copy["format_exr"].string).tag("Exr")
                }.accessibilityIdentifier("export-range")
            }
            if !hdr {
                FormPicker(copy["format"].string, selection: Binding(get: { format }, set: { editor.change("format", JSON($0)) })) {
                    options("formats", labels: ["Png": copy["format_png"].string, "Tiff": copy["format_tiff"].string, "Jpeg": copy["format_jpeg"].string, "Webp": copy["format_webp"].string])
                }.accessibilityIdentifier("export-format")
            }
            if hdr {
                Text(copy[format == "Exr" ? "format_exr" : gainmap ? (format.hasPrefix("Avif") ? "avif_gainmap" : "jpeg_gainmap") : "format_pq"].string).font(.caption)
                if format == "JpegHdr" {
                    FormPicker(copy["transparency"].string, selection: choice("background")) {
                        options("backgrounds", labels: ["Preserve": copy["keep_transparency"].string, "White": copy["white_background"].string, "Black": copy["black_background"].string])
                    }.accessibilityIdentifier("export-background")
                }
            }
            else {
            FormPicker(copy["profile"].string, selection: Binding(get: { editor.profileIndex }, set: editor.selectProfile)) {
                ForEach(editor.profiles.indices, id: \.self) { Text(editor.profileNames.indices.contains($0) ? editor.profileNames[$0] : "").tag($0) }
            }.accessibilityIdentifier("export-profile")
            ProfileChooserButtons(preferences: editor.preferences, busy: $readingProfile, onProfile: editor.imported)
            FormPicker(copy["depth"].string, selection: choice("depth")) {
                options("depths", labels: ["U8": copy["depth_8"].string, "U16": copy["depth_16"].string])
            }.disabled(editor.draft["depths"].array.count < 2).accessibilityIdentifier("export-depth")
            FormPicker(copy["transparency"].string, selection: choice("background")) {
                options("backgrounds", labels: ["Preserve": copy["preserve"].string, "White": copy["white_background"].string, "Black": copy["black_background"].string])
            }.accessibilityIdentifier("export-background")
            }
            if format == "Jpeg" || gainmap { number(copy["quality"].string, $quality, id: "export-quality") }
            Toggle(copy["fit_bounds"].string, isOn: $fit).accessibilityIdentifier("export-fit")
            if fit {
                number(copy["maximum_width"].string, $width, id: "export-width")
                number(copy["maximum_height"].string, $height, id: "export-height")
                Toggle(copy["enlarge"].string, isOn: $enlarge)
            }
            DisclosureGroup(copy["common"]["more"].string) {
                VStack(alignment: .leading, spacing: 12) {
                    if hdr && format != "Exr" {
                        Toggle(copy["clip_hdr"].string, isOn: Binding(get: { editor.draft["clip_hdr_range"].bool }, set: { editor.change("clip_hdr_range", JSON($0)) }))
                            .accessibilityIdentifier("export-hdr-clip")
                    }
                    if !hdr {
                    FormPicker(copy["intent"].string, selection: Binding(get: { editor.recipe["encoding"]["conversion"]["intent"].string }, set: { intent in
                        let conversion = editor.recipe["encoding"]["conversion"].replacing("intent", with: JSON(intent))
                        editor.change("encoding", editor.recipe["encoding"].replacing("conversion", with: conversion))
                    })) {
                        Text(copy["relative"].string).tag("RelativeColorimetric"); Text(copy["perceptual"].string).tag("Perceptual")
                        Text(copy["saturation"].string).tag("Saturation"); Text(copy["absolute"].string).tag("AbsoluteColorimetric")
                    }
                    FormPicker(copy["dither"].string, selection: Binding(get: { editor.recipe["encoding"]["dither"].string }, set: { encoding("dither", $0) })) {
                        options("dithers", labels: ["None": copy["dither_none"].string, "Stochastic8": copy["dither_stochastic"].string])
                    }.disabled(editor.draft["dithers"].array.count < 2)
                    }
                    FormPicker(copy["resolution"].string, selection: $resolution) {
                        Text(copy["keep_resolution"].string).tag("Master"); Text(copy["ppi"].string).tag("Ppi"); Text(copy["omit"].string).tag("Omit")
                    }
                    if resolution == "Ppi" { number(copy["ppi"].string, $ppi, id: "export-ppi") }
                    if editor.details["form"]["metadata"].bool { metadata }
                }.padding(.top, 8)
            }
            DisclosureGroup(copy["presets"].string) {
                VStack(alignment: .leading, spacing: 8) {
                    TextField(copy["preset_name"].string, text: $presetName).textFieldStyle(.roundedBorder).accessibilityIdentifier("export-preset-name")
                    HStack {
                        Button(copy["save_preset"].string) { withRecipe { editor.preference(JSON(["type": "save", "name": presetName, "recipe": $0.raw])) } }
                            .accessibilityIdentifier("export-preset-save")
                        Button(copy["update_preset"].string) { withRecipe { editor.preference(JSON(["type": "update", "index": editor.destination, "recipe": $0.raw])) } }
                            .disabled(editor.destination < 4)
                    }
                    HStack {
                        Button(copy["delete_preset"].string, role: .destructive) { editor.preference(JSON(["type": "remove", "index": editor.destination])) }
                            .disabled(editor.destination < 4)
                        Button(copy["reset_destination"].string) { editor.preference(JSON(["type": "reset", "index": editor.destination])) }
                            .disabled(editor.destination >= 4)
                    }
                }.padding(.top, 8).disabled(!editor.preferences.canSave)
            }
        }
    }
    @ViewBuilder private var metadata: some View {
        let view = editor.metadataCopy, kept = editor.recipe["metadata"]
        if view["available"].bool {
            FormPicker(view["label"].string, selection: Binding(get: { kept["keep"].string }, set: {
                editor.change("metadata", kept.replacing("keep", with: JSON($0)))
            })) {
                ForEach(view["choices"].array, id: \.stableKey) { Text($0["label"].string).tag($0["value"].string) }
            }.accessibilityIdentifier("export-metadata")
        }
        if view["location"].bool {
            Toggle(view["remove_location"].string, isOn: Binding(get: { kept["remove_location"].bool }, set: {
                editor.change("metadata", kept.replacing("remove_location", with: JSON($0)))
            })).accessibilityIdentifier("export-remove-location")
        }
        if !view["note"].isNull { Text(view["note"].string).font(.caption).accessibilityIdentifier("export-metadata-note") }
    }
    private func choice(_ key: String) -> Binding<String> {
        Binding(get: { editor.recipe[key].string }, set: { editor.change(key, JSON($0)) })
    }
    private func options(_ key: String, labels: [String: String]) -> some View {
        ForEach(editor.draft[key].array.map(\.string), id: \.self) { value in
            Text(labels[value] ?? value).tag(value)
        }
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
        func integer(_ text: String, _ control: String) throws -> UInt64 {
            try NativeTextContext.number(editor.details["form"]["numeric"][control], value: 0,
                operation: ["type": "expression", "text": text], language: language)["value"].uint
        }
        return try editor.recipe.replacing("jpeg_quality", with: (format == "Jpeg" || gainmap)
                ? JSON(integer(quality, "quality")) : editor.recipe["jpeg_quality"])
            .replacing("size", with: fit ? JSON(["Fit": ["bounds": [integer(width, "dimension"), integer(height, "dimension")], "enlarge": enlarge]]) : JSON("Original"))
            .replacing("resolution", with: resolution == "Ppi" ? JSON(["Ppi": integer(ppi, "ppi")]) : JSON(resolution))
    }
    private func withRecipe(_ action: (JSON) -> Void) {
        do { action(try selectedRecipe()) }
        catch { editor.error = error.localizedDescription }
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
