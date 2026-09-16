import SwiftUI
import UniformTypeIdentifiers
#if os(macOS)
import AppKit
#endif

struct DocumentColorForm: View {
    @ObservedObject var editor: DocumentColorController
    let spaces: [JSON]
    @State private var space = "Srgb"
    @State private var depth = "U16"
    @State private var intent = "RelativeColorimetric"
    @State private var dither = "None"
    @State private var copy = false
    @State private var filename = "Converted copy.capy"
    @State private var choosingFolder = false
    private var choice: JSON {
        switch editor.operation {
        case "assign": return JSON(["Assign": space])
        case "depth": return JSON(["Depth": ["depth": depth, "dither": depth == "U8" ? dither : "None"]])
        default: return JSON(["Convert": ["space": space, "options": ["intent": intent, "black_point_compensation": false]]])
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(editor.title).font(.headline)
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    if editor.properties {
                        ForEach(Array(editor.rows.enumerated()), id: \.offset) { _, row in
                            Text(row[0].string).font(.headline)
                            Text(row[1].string).textSelection(.enabled)
                        }
                    } else if !editor.history {
                        Text(editor.operation == "assign"
                            ? "Keep document RGB numbers and reinterpret their color. Retained original photos keep their source profile."
                            : editor.operation == "depth" ? "Change stored precision. Effects and blending remain the same."
                            : "Converting editable layers can change blending and effects. Compare the complete composition before applying; original photo samples stay retained.")
                        choices.disabled(editor.busy || !editor.loaded)
                        Button("Preview Complete Result") { editor.prepare(choice, copy: copy) }
                            .disabled(editor.busy || !editor.loaded).accessibilityIdentifier("document-color-preview")
                    }
                    if editor.busy { ProgressView(editor.publishing ? "Applying color result…" : "Preparing document…") }
                    if editor.clipped > 0 { Text("Some colors exceed the destination gamut. Compare the result before applying.") }
                    ForEach(Array(editor.previews.enumerated()), id: \.offset) { index, image in
                        Text(index == 0 ? "Before" : "After").font(.headline)
                        Image(decorative: image, scale: 1).resizable().aspectRatio(contentMode: .fit)
                            .frame(maxWidth: .infinity, maxHeight: 180)
                            .accessibilityLabel(index == 0 ? "Original composition" : "Prepared composition")
                    }
                    if let error = editor.error { Text(error).foregroundStyle(.red) }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack {
                Button(editor.properties ? "Done" : "Cancel") { editor.cancel() }
                    .keyboardShortcut(.cancelAction).disabled(editor.publishing)
                Spacer()
                if !editor.properties && !editor.history {
                    Button(editor.copy ? "Save Copy…" : "Apply") {
                        if editor.copy { saveCopy() } else { editor.apply() }
                    }.keyboardShortcut(.defaultAction).disabled(!editor.ready || editor.busy)
                        .accessibilityIdentifier("document-color-apply")
                }
            }
        }.padding(24).frame(minWidth: 340, idealWidth: 520, maxWidth: 620, minHeight: 260, idealHeight: 640, maxHeight: 720)
            .onChange(of: editor.color.stableKey, initial: true) { _, _ in
                if editor.loaded { space = editor.color["space"].string; depth = editor.color["depth"].string }
            }
            .onChange(of: choice.stableKey) { _, _ in editor.invalidate() }
            .onChange(of: copy) { _, _ in editor.invalidate() }
            #if os(iOS)
            .fileImporter(isPresented: $choosingFolder, allowedContentTypes: [.folder]) { result in
                switch result {
                case .success(let folder): editor.saveCopy(to: folder.appendingPathComponent(filename), access: folder)
                case .failure(let error):
                    let native = error as NSError
                    if native.domain != NSCocoaErrorDomain || native.code != NSUserCancelledError {
                        editor.error = error.localizedDescription
                    }
                }
            }
            #endif
    }
    @ViewBuilder private var choices: some View {
        if editor.operation != "depth" {
            Picker("Color space", selection: $space) {
                ForEach(spaces, id: \.stableKey) { Text($0[1].string).tag($0[0].string) }
            }.accessibilityIdentifier("document-color-space")
        }
        if editor.operation == "depth" {
            Picker("Bit depth", selection: $depth) { Text("8-bit SDR").tag("U8"); Text("16-bit SDR").tag("U16") }
                .accessibilityIdentifier("document-color-depth")
            if depth == "U8" {
                Picker("Dither", selection: $dither) { Text("None").tag("None"); Text("Stochastic").tag("Stochastic8") }
            }
        }
        if editor.operation == "convert" {
            Picker("Result", selection: $copy) { Text("Editable layers").tag(false); Text("Save flattened copy").tag(true) }
            Picker("Rendering intent", selection: $intent) {
                Text("Relative colorimetric").tag("RelativeColorimetric"); Text("Perceptual").tag("Perceptual")
                Text("Saturation").tag("Saturation"); Text("Absolute colorimetric").tag("AbsoluteColorimetric")
            }
            #if os(iOS)
            if copy { TextField("Copy filename", text: $filename).textFieldStyle(.roundedBorder) }
            #endif
        }
    }
    private func saveCopy() {
        #if os(macOS)
        let panel = NSSavePanel(); panel.allowedContentTypes = [.capyProject]
        panel.nameFieldStringValue = filename; panel.canCreateDirectories = true
        panel.begin { response in if response == .OK, let url = panel.url { editor.saveCopy(to: url) } }
        #else
        filename = filename.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !filename.isEmpty, !filename.contains("/"), filename != ".", filename != ".." else {
            editor.error = "Choose a filename without folders."; return
        }
        if !filename.lowercased().hasSuffix(".capy") { filename += ".capy" }
        choosingFolder = true
        #endif
    }
}
