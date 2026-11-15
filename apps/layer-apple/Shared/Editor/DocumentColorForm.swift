import SwiftUI

struct DocumentColorForm: View {
    @ObservedObject var editor: DocumentColorController
    @Environment(\.capyInterfaceLanguage) private var language
    @Environment(\.editorDialogPage) private var page
    let spaces: [JSON]
    @State private var space = "Srgb"
    @State private var depth = "U16"
    @State private var intent = "RelativeColorimetric"
    @State private var dither = false
    @State private var copy = false
    @State private var profile = JSON()
    @State private var readingProfile = false
    private var text: JSON { editor.text }
    private var exportCopy: JSON { editor.catalog["export_copy"] }
    private var delivery: JSON { editor.catalog["document_delivery_copy"] }
    private var dithering: JSON { editor.dither(depth, active: dither) }
    private var help: String {
        if editor.operation == "rasterize_source" { return editor.rasterizeDetails }
        let keys = ["repair_source_profile": "repair_help", "assign": "assign_help", "depth": "depth_help"]
        return text[keys[editor.operation] ?? "convert_help"].string
    }
    private var progress: String {
        if editor.publishing { return text["applying"].string }
        if !editor.loaded { return editor.bootstrap["preparing_document"].string }
        if editor.ready { return delivery["writing_converted_copy"].string }
        if editor.source { return text[editor.operation == "rasterize_source" ? "rasterize_comparison" : "preparing_comparison"].string }
        return editor.copy ? delivery["preparing_converted_copy"].string : text["preparing"].string
    }
    private var applyLabel: String {
        if editor.operation == "rasterize_source" { return text["rasterize"].string }
        if editor.sourceInfo["adds_layer"].bool { return text["add_source"].string }
        return editor.source ? text["apply_profile"].string : text["common"]["apply"].string
    }
    private var choice: JSON? {
        switch editor.operation {
        case "repair_source_profile": return profile.isNull ? nil : profile["profile"]
        case "rasterize_source": return nil
        case "assign": return JSON(["Assign": space])
        case "depth": return JSON(["Depth": ["depth": depth, "dither": dithering["value"].raw]])
        default: return JSON(["Convert": ["space": space, "options": ["intent": intent, "black_point_compensation": false]]])
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if page {
                EditorPageHeader(intent: editor.dialog, title: editor.title, identifier: "document-color",
                    primaryLabel: applyLabel, primaryEnabled: editor.properties ? !editor.busy : editor.ready && !editor.busy && !readingProfile,
                    cancel: { if !editor.publishing { editor.cancel() } }, primary: { editor.apply() })
            } else { Text(editor.title).font(.headline) }
            EditorScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    if editor.properties {
                        ForEach(Array(editor.rows.enumerated()), id: \.offset) { _, row in
                            Text(row[0].string).font(.headline)
                            Text(row[1].string).textSelection(.enabled)
                        }
                        if !editor.propertiesView["sources"].array.isEmpty {
                            Text(editor.propertiesView["source_images"].string).font(.headline)
                            ForEach(Array(editor.propertiesView["sources"].array.enumerated()), id: \.offset) { _, row in
                                Text(row[0].string).font(.headline)
                                Text(row[1].string).textSelection(.enabled)
                            }
                        }
                    } else if !editor.history {
                        Text(help)
                        if editor.operation == "repair_source_profile" {
                            Text(text["current_source"].string).font(.headline)
                            Text(editor.sourceDescription).textSelection(.enabled)
                        }
                        choices.disabled(editor.publishing || !editor.loaded)
                    }
                    if editor.busy { ProgressView(progress) }
                    if editor.clipped > 0 { Text(text[editor.source ? "source_clipped" : "outside_gamut"].string) }
                    if editor.sourceInfo["adds_layer"].bool { Text(text["adds_layer"].string) }
                    ComparisonPanes {
                        ForEach(Array(editor.previews.enumerated()), id: \.offset) { index, image in
                            VStack(alignment: .leading, spacing: 8) {
                                Text(text[index == 0 ? "before" : "after"].string).font(.headline)
                                Image(decorative: image, scale: 1).resizable().aspectRatio(contentMode: .fit)
                                    .frame(maxWidth: .infinity, maxHeight: 180)
                                    .accessibilityLabel(text[index == 0 ? "original_composition" : "prepared_composition"].string)
                            }
                        }
                    }
                    if let error = editor.error { Text(error).foregroundStyle(.red) }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack {
                if !page { Button(editor.properties ? editor.propertiesDone : text["common"]["cancel"].string) { editor.cancel() }
                    .keyboardShortcut(.cancelAction).disabled(editor.publishing) }
                Spacer()
                if editor.properties {
                    Button(editor.propertiesView["proof"].string) { editor.showAppearance() }
                        .disabled(editor.busy).accessibilityIdentifier("document-properties-appearance")
                }
                if !page && !editor.properties && !editor.history {
                    Button(applyLabel) {
                        editor.apply()
                    }.keyboardShortcut(.defaultAction).disabled(!editor.ready || editor.busy || readingProfile)
                        .accessibilityIdentifier("document-color-apply")
                }
            }
        }.padding(24).modifier(EditorDialogFrame(minWidth: 340, idealWidth: 520, maxWidth: 620, minHeight: 260, idealHeight: 640, maxHeight: 720))
            .onChange(of: language, initial: true) { _, _ in editor.refreshCopy() }
            .onChange(of: editor.color.stableKey, initial: true) { _, _ in
                if editor.loaded {
                    space = editor.color["space"].string
                    depth = editor.suggestedDepth
                }
            }
            .onChange(of: editor.loaded) { _, loaded in if loaded { editor.schedule(choice, copy: copy) } }
            .onChange(of: editor.source ? nil : choice?.stableKey) { _, _ in editor.schedule(choice, copy: copy) }
            .onChange(of: copy) { _, _ in editor.schedule(choice, copy: copy) }
    }
    @ViewBuilder private var choices: some View {
        if editor.operation == "repair_source_profile" {
            if !editor.sourceContext.isNull {
                SourceProfilePicker(label: text["correct_profile"].string, preferences: editor.preferences, purpose: editor.sourceContext["purpose"],
                    original: JSON(["name": editor.sourceProfile, "profile": editor.sourceOriginal.raw]),
                    selection: Binding(get: { profile }, set: { profile = $0; editor.schedule($0["profile"], copy: copy) }), busy: $readingProfile)
            }
        } else if editor.operation != "depth" && editor.operation != "rasterize_source" {
            FormPicker(text["space"].string, selection: $space) {
                ForEach(spaces, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
            }.accessibilityIdentifier("document-color-space")
        }
        if editor.operation == "depth" {
            FormPicker(text["depth"].string, selection: $depth) {
                Text(text["depth_8"].string).tag("U8"); Text(text["depth_16"].string).tag("U16")
                Text(text["depth_float16"].string).tag("F16"); Text(text["depth_float32"].string).tag("F32")
            }
                .accessibilityIdentifier("document-color-depth")
            Toggle(exportCopy["reduce_banding"].string, isOn: $dither).disabled(!dithering["available"].bool)
                .accessibilityIdentifier("document-color-dither")
        }
        if editor.operation == "convert" {
            FormPicker(text["result"].string, selection: $copy) {
                Text(text["editable_layers"].string).tag(false); Text(text["flattened_copy"].string).tag(true)
            }
            FormPicker(exportCopy["intent"].string, selection: $intent) {
                Text(exportCopy["relative"].string).tag("RelativeColorimetric"); Text(exportCopy["perceptual"].string).tag("Perceptual")
                Text(exportCopy["saturation"].string).tag("Saturation"); Text(exportCopy["absolute"].string).tag("AbsoluteColorimetric")
            }

        }
    }
}

private struct ComparisonPanes: Layout {
    private func frames(_ width: CGFloat, _ subviews: Subviews) -> [CGRect] {
        let fit = DialogUI.resolve(["kind": "comparison_layout", "width": width]), gap = fit["gap"].number
        let split = fit["split"].bool && subviews.count == 2, column = split ? (width - gap) / 2 : width
        var frames: [CGRect] = [], y: CGFloat = 0
        for (index, view) in subviews.enumerated() {
            let height = view.sizeThatFits(ProposedViewSize(width: column, height: nil)).height
            frames.append(CGRect(x: split ? CGFloat(index) * (column + gap) : 0, y: split ? 0 : y, width: column, height: height))
            y += height + gap
        }
        return frames
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? 0
        return CGSize(width: width, height: frames(width, subviews).map(\.maxY).max() ?? 0)
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (view, frame) in zip(subviews, frames(bounds.width, subviews)) {
            view.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), proposal: ProposedViewSize(frame.size))
        }
    }
}
