import SwiftUI
import UniformTypeIdentifiers

struct PaletteFiles: ViewModifier {
    @ObservedObject var controller: PaletteController
    private static let types: [UTType] = {
        let types = ColorPreferencesStore.paletteExtensions().compactMap { UTType(filenameExtension: $0) }
        return types.isEmpty ? [.data] : types
    }()
    func body(content: Content) -> some View {
        content
            .fileImporter(isPresented: $controller.importing, allowedContentTypes: Self.types) { controller.importPalette($0) }
            .fileExporter(isPresented: Binding(get: { controller.export != nil }, set: { if !$0 { controller.export = nil } }),
                document: controller.export?.document, contentType: controller.export?.type ?? .data,
                defaultFilename: controller.export?.name) { controller.exported($0) }
            .sheet(item: Binding(get: { controller.dialog.flatMap { $0.kind == .remove ? nil : $0 } },
                set: { if $0 == nil, controller.dialog?.kind != .remove { controller.dialog = nil } })) { dialog in
                PaletteNameForm(controller: controller, dialog: dialog).modifier(EditorPopupPresentation())
            }
            .alert("Remove Palette?", isPresented: Binding(get: { controller.dialog?.kind == .remove },
                set: { if !$0, controller.dialog?.kind == .remove { controller.dialog = nil } }), presenting: controller.dialog) { dialog in
                Button("Cancel", role: .cancel) { controller.dialog = nil }
                Button("Remove", role: .destructive) {
                    controller.dialog = nil
                    if let id = dialog.palette { controller.apply(["op": "remove_palette", "id": id]) }
                }.accessibilityIdentifier("palette-remove-confirm")
            } message: { dialog in Text("Remove “\(dialog.name)” and its saved colors?") }
    }
}

private struct PaletteNameForm: View {
    @ObservedObject var controller: PaletteController
    let dialog: PaletteDialog
    @State private var name: String
    @State private var error: String?
    @State private var saving = false
    @FocusState private var focused: Bool
    init(controller: PaletteController, dialog: PaletteDialog) {
        self.controller = controller; self.dialog = dialog
        _name = State(initialValue: dialog.name)
    }
    private var action: [String: Any] {
        if dialog.kind == .rename, let id = dialog.palette { return ["op": "rename_palette", "id": id, "name": name] }
        return ["op": "create_palette", "name": name]
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(dialog.kind == .rename ? "Rename Palette" : "New Palette").font(.headline)
            TextField("Name", text: Binding(get: { name }, set: { name = String($0.prefix(64)) }))
                .textFieldStyle(.roundedBorder).focused($focused).onSubmit(save)
                .accessibilityIdentifier("palette-library-name")
            if let error { Text(error).foregroundStyle(.red).font(.callout).accessibilityIdentifier("palette-name-error") }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { controller.dialog = nil }.keyboardShortcut(.cancelAction)
                Button("Save", action: save).keyboardShortcut(.defaultAction).disabled(error != nil || saving)
                    .accessibilityIdentifier("palette-name-save")
            }
        }.padding(20).frame(minWidth: 320)
            .accessibilityElement(children: .contain).accessibilityIdentifier("palette-name-dialog")
            .onAppear { focused = true }
            .task(id: name) { controller.dryRun(action) { error = $0 } }
    }
    private func save() {
        guard error == nil, !saving else { return }
        saving = true
        controller.apply(action) { failure in
            saving = false
            if let failure { error = failure } else { controller.dialog = nil; controller.chooser = false }
        }
    }
}
