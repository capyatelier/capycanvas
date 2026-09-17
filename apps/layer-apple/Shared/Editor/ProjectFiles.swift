import SwiftUI
import UniformTypeIdentifiers
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Native panels and URLs surround the shared document transactions. A save
/// acknowledges its captured revision only after the file operation succeeds.
@MainActor final class ProjectFiles: ObservableObject {
    @Published var busy = false
    @Published var blocksEditor = false
    @Published var confirming = false
    @Published var error: String?
    @Published var picker: Picker?
    @Published var cancelling = false
    @Published var creating = false
    @Published var creationError: String?
    @Published var creationSaving = false
    @Published var colorEditor: DocumentColorController?
    @Published var exportEditor: ExportController?
    @Published var pendingProfile: JSON?
    @Published var profileError: String?
    @Published var interpreting = false
    private var profileCompletion: ((JSON?) -> Void)?
    private var creationCompletion: ((JSON?) -> Void)?
    private var exportDelivery: (() -> Void)?
    private weak var store: EditorStore?
    private var requestID: UInt64?
    private var approved: (UInt64, UInt64)?
    private var destination: URL?
    var closeWindow: (() -> Void)?
    private var handledClose = false
    private var activeTask: NativeProjectTask?
    private var pickerCompletion: (([URL]) -> Void)?
    private var cancelled = false
    private var finishing = false
    private var externalOpen: (url: URL, submitted: Bool)?
    private var droppedPhotos: (items: [PhotoItem], placement: JSON)?
    private var loadingPhoto = false
    private var recovering: RecoveryRecord?
    private var closeCompletion: ((Bool) -> Void)?
    /// Dialog dependency keeps editor/file effects testable without driving
    /// platform panels. The app uses the native implementation by default.
    struct Dialogs {
        var open: (Bool, @escaping ([URL]) -> Void) -> Void
        var save: (String, UTType, @escaping (URL?) -> Void) -> Void
        var create: ((JSON, @escaping (JSON?) -> Void) -> Void)? = nil
        var export: ((URL, @escaping (URL?) -> Void) -> Void)? = nil
        var paste: ((@escaping (Result<[PhotoItem], Error>) -> Void) -> Void)? = nil
        var exportOptions: ((ExportController) -> Void)? = nil
    }
    private let dialogs: Dialogs?
    let preferences: ColorPreferencesStore
    struct Picker: Identifiable {
        let id = UUID()
        let export: URL?
        let types: [UTType]
        var multiple = false
    }
    init(store: EditorStore, dialogs: Dialogs? = nil) { self.store = store; self.dialogs = dialogs; self.preferences = store.colorPreferences }
    var title: String {
        let name = store?.state["document_file"]["location"]["name"].string ?? ""
        return name.isEmpty ? "Untitled" : name
    }
    func receive(_ state: JSON) {
        submitExternalOpen()
        let file = state["document_file"]
        if !busy { blocksEditor = file["close_ready"].bool }
        if !file["close_ready"].bool { handledClose = false }
        if file["close_ready"].bool && !handledClose {
            handledClose = true
            if closeCompletion != nil { finishClose(true) } else { closeWindow?() }
        }
        guard requestID == nil, !finishing,
            let request = state["requests"].array.first(where: { $0["kind"]["type"].string == "document" }) else { return }
        requestID = request["id"].uint; busy = true
        if droppedPhotos == nil { cancelling = false; cancelled = false }
        approved = (file["epoch"].uint, file["revision"].uint)
        let document = request["kind"]["request"]
        let action = document["type"].string
        blocksEditor = action != "save" || closeCompletion != nil
        switch action {
        case "save":
            destination = URL(string: document["location"]["uri"].string)
            save(as: document["location"].isNull) { [weak self] saved in self?.finish(saved) }
        case "new":
            let completed: (JSON?) -> Void = { [weak self] options in
                guard let self else { return }
                if let options { open(nil, options: options) } else { finish() }
            }
            if let create = dialogs?.create { create(newDocumentSpec, completed) }
            else { creationError = nil; creationCompletion = completed; creating = true }
        case "export": beginExport(name: document["name"].string)
        case "open":
            if let url = externalOpen?.url { externalOpen = nil; open(url) }
            else { chooseOpen { [weak self] urls in
                guard let self else { return }
                if let url = urls.first { open(url) } else { finish() }
            } }
        case "place":
            let drop = droppedPhotos; droppedPhotos = nil
            task(opening: true, placing: true, placement: drop?.placement) { [weak self] task in
                if let drop { self?.place(task, inputs: drop.items); return }
                self?.chooseOpen(photosOnly: true) { [weak self] urls in
                    guard let self else { return }
                    if urls.isEmpty { finish() } else { place(task, inputs: urls.map(PhotoItem.init(fileURL:))) }
                }
            }
        case "paste":
            task(opening: true, placing: true) { [weak self] task in
                guard let self else { return }
                let id = requestID
                let completed: (Result<[PhotoItem], Error>) -> Void = { [weak self] result in
                    guard let self, requestID == id else { return }
                    if cancelled { finish(); return }
                    switch result {
                    case .success(let images):
                        if images.isEmpty { fail("The clipboard contains no supported images") }
                        else { place(task, inputs: images) }
                    case .failure(let error): fail(error.localizedDescription)
                    }
                }
                if let paste = dialogs?.paste { paste(completed) } else { PhotoClipboard.read(completed) }
            }
        case "change_color", "color_history", "properties", "repair_source_profile", "rasterize_source":
            guard let store else { fail("The canvas session is unavailable"); return }
            let editor = DocumentColorController(store: store, request: document, expected: approved) { [weak self] result in
                self?.colorEditor = nil; self?.finish(result)
            }
            colorEditor = editor; editor.load()
        case "confirm_close": confirming = true
        default: fail("This document service is not available yet")
        }
    }
    func openURL(_ url: URL) {
        // Native URL delivery can repeat before the owner publishes its request.
        // Keep the first destination reserved during that interval as well.
        guard !busy, externalOpen == nil else { error = "Finish the current document operation first"; return }
        guard let store else { error = "The canvas session is unavailable"; return }
        if store.snapshot["shaders_ready"].bool && store.workspaceLibrary?.ready != false
            && !store.command("open_document")["enabled"].bool {
            error = "Finish the canvas interaction before opening a drawing"; return
        }
        externalOpen = (url, false)
        submitExternalOpen()
    }
    func submitExternalOpen() {
        // File launch can precede the first snapshot, workspace restoration and
        // Metal attachment. First-frame catalog validation can still start after
        // attachment, so wait for full startup before submitting the request.
        // Publications resume this one pending request.
        guard externalOpen?.submitted == false, let store,
            store.workspaceLibrary?.ready != false, store.snapshot["shaders_ready"].bool,
            store.command("open_document")["enabled"].bool else { return }
        externalOpen?.submitted = true
        store.edit(["type": "invoke", "command": "open_document"]) { [weak self] error in
            guard let self, let error else { return }
            externalOpen = nil; recovering = nil; self.error = error
        }
    }
    func recover(_ record: RecoveryRecord) {
        guard !busy, externalOpen == nil, let url = store?.recovery.files.archive(record),
            store?.command("open_document")["enabled"].bool == true else {
            error = "Finish the current canvas operation before recovering a drawing"; return
        }
        recovering = record; openURL(url)
    }
    func confirmClose(_ completion: @escaping (Bool) -> Void) {
        guard !busy, externalOpen == nil, let native = store?.native else { completion(false); return }
        busy = true; blocksEditor = true; closeCompletion = completion
        native.documentRequest(closeDecision: 0) { [weak self] error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let error { self.report(error); self.finishClose(false) }
                else if self.store?.state["document_file"]["close_ready"].bool == true { self.finishClose(true) }
            }
        }
    }
    private func finishClose(_ allowed: Bool) {
        let completion = closeCompletion; closeCompletion = nil
        if requestID == nil { busy = false; blocksEditor = allowed; activeTask = nil; cancelling = false }
        completion?(allowed)
    }
    func choose(_ choice: String) {
        guard let id = requestID, confirming else { return }
        confirming = false; finishing = true
        store?.native?.documentRequest(id: id, closeDecision: choice == "save" ? 1 : choice == "discard" ? 2 : 3) { [weak self] error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let error { self.report(error) }
                if choice == "cancel" { self.externalOpen = nil; self.recovering = nil }
                self.released()
            }
        }
    }
    func dismissAlert() {
        if error != nil { error = nil; return }
        DispatchQueue.main.async { [weak self] in
            if self?.confirming == true { self?.choose("cancel") }
        }
    }
    func cancel() {
        if let colorEditor { colorEditor.cancel(); return }
        if let exportEditor { exportEditor.cancel(); return }
        cancelled = true; cancelling = true; activeTask?.cancel()
        // Provider delivery can take arbitrarily long. Its late callback is
        // rejected by request ID; cancellation need not wait for that callback.
        if loadingPhoto { finish() }
    }
    private func task(opening: Bool, placing: Bool = false, placement: JSON? = nil, _ ready: @escaping (NativeProjectTask) -> Void) {
        guard let native = store?.native else { fail("The canvas session is unavailable"); return }
        native.projectTask(kind: placing ? .place : opening ? .open : .save, expected: opening ? approved : nil,
            placement: placement) { [weak self] task, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if self.cancelled { self.finish(); return }
                guard let task else { self.fail(error ?? "Document is unavailable"); return }
                self.activeTask = task; ready(task)
            }
        }
    }
    var newDocumentSpec: JSON {
        (store?.catalog["new_document"] ?? JSON()).replacing("creation",
            with: store?.snapshot["document_options"]["creation"] ?? JSON())
    }
    func created(_ choice: NewDrawingChoice?) {
        guard !creationSaving, creationCompletion != nil else { return }
        func complete(_ options: JSON?) {
            let completion = creationCompletion; creationCompletion = nil; creating = false
            completion?(options)
        }
        guard let choice else { complete(nil); return }
        guard let store else { creationError = "The canvas session is unavailable"; return }
        creationSaving = true
        store.edit(["type": "new_document_preferences", "action": ["type": "remember",
            "options": choice.options.raw, "name": choice.presetName, "defaults": choice.useAsDefaults]]) { [weak self] error in
            guard let self else { return }
            creationSaving = false; creationError = error
            if error == nil { complete(choice.options) }
        }
    }
    private func save(as copy: Bool, completion: @escaping (Bool) -> Void) {
        if !copy, let destination { write(destination, completion: completion); return }
        task(opening: false) { [weak self] task in
            guard let self else { return }
            deliver(task, name: title, type: .capyProject) { [weak self] url in
                guard let self, let url else { completion(false); return }
                saved(task, at: url, completion: completion)
            }
        }
    }
    private func beginExport(name: String) {
        guard let id = requestID, let native = store?.native else { fail("The canvas is unavailable"); return }
        let editor = ExportController(native: native, request: id, preferences: preferences) { [weak self] task, recipe, destination, color in
            guard let self, self.requestID == id else { task?.cancel(); return }
            self.exportEditor = nil
            guard let task else { self.finish(); return }
            self.activeTask = task
            let type: UTType = recipe["format"].string == "Tiff" ? .tiff : recipe["format"].string == "Jpeg" ? .jpeg : .png
            let filename = URL(fileURLWithPath: name).deletingPathExtension().lastPathComponent + "." + (type.preferredFilenameExtension ?? "png")
            self.exportDelivery = { [weak self] in
                guard let self else { return }
                if self.cancelled { self.finish(); return }
                self.blocksEditor = false
                self.deliver(task, name: filename, type: type) { [weak self] url in
                    guard let self else { return }
                    guard url != nil else { self.finish(); return }
                    guard self.preferences.canSave else { self.finish(true); return }
                    let preferences = self.preferences
                    NativeProjectTask.io.async { [weak self] in
                        let result = Result { try preferences.presets(color: color, request:
                            JSON(["type": "remember", "index": destination < 4 ? destination : 3, "recipe": recipe.raw])) }
                        DispatchQueue.main.async {
                            if case .failure(let error) = result {
                                self?.report("Image exported, but its preferences could not be saved: \(error.localizedDescription)")
                            }
                            self?.finish(true)
                        }
                    }
                }
            }
            if self.dialogs?.exportOptions != nil { self.exportDismissed() }
        }
        exportEditor = editor
        editor.load { [weak self, weak editor] in
            if let editor { self?.dialogs?.exportOptions?(editor) }
        }
    }
    /// Present the native file picker after the export sheet has dismissed.
    func exportDismissed() {
        let delivery = exportDelivery; exportDelivery = nil; delivery?()
    }
    private var usesExportPicker: Bool {
        #if os(macOS)
        return dialogs?.export != nil
        #else
        return true
        #endif
    }
    /// Share destination and staging behavior for projects and profiled images.
    private func deliver(_ task: NativeProjectTask, name: String, type: UTType, completion: @escaping (URL?) -> Void) {
        if !usesExportPicker {
            chooseSave(name: name, type: type) { [weak self] url in
                guard let self, let url else { completion(nil); return }
                NativeProjectTask.io.async {
                    do { try task.write(to: url); DispatchQueue.main.async { completion(url) } }
                    catch { let message = error.localizedDescription
                        DispatchQueue.main.async { self.report(message); completion(nil) }
                    }
                }
            }
        } else {
            NativeProjectTask.io.async { [weak self] in
                do {
                    let staging = try ProjectFileIO.stagingURL(title: name, extension: type == .capyProject ? "capy" : type.preferredFilenameExtension ?? "png")
                    do { try task.write(to: staging) }
                    catch { try? FileManager.default.removeItem(at: staging.deletingLastPathComponent()); throw error }
                    DispatchQueue.main.async {
                        guard let self else { Self.removeStaging(staging); return }
                        if self.cancelled { Self.removeStaging(staging); completion(nil); return }
                        let completed: (URL?) -> Void = { url in Self.removeStaging(staging); completion(url) }
                        if let export = self.dialogs?.export { export(staging, completed) }
                        else { self.pickerCompletion = { completed($0.first) }; self.picker = Picker(export: staging, types: []) }
                    }
                } catch { let message = error.localizedDescription
                    DispatchQueue.main.async { self?.report(message); completion(nil) }
                }
            }
        }
    }
    private func write(_ url: URL, completion: @escaping (Bool) -> Void) {
        task(opening: false) { [weak self] task in
            NativeProjectTask.io.async {
                do {
                    try task.write(to: url)
                    DispatchQueue.main.async { self?.saved(task, at: url, completion: completion) }
                } catch {
                    let message = error.localizedDescription
                    DispatchQueue.main.async { self?.report(message); completion(false) }
                }
            }
        }
    }
    private func saved(_ task: NativeProjectTask, at url: URL, completion: @escaping (Bool) -> Void) {
        guard let native = store?.native else { completion(false); return }
        native.finishProject(task, opening: false, title: url.lastPathComponent, url: url) { [weak self] error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let error { self.report(error); completion(false) }
                else { self.destination = url; completion(true) }
            }
        }
    }
    func drop(_ providers: [NSItemProvider], placement: JSON) -> Bool {
        guard let store, !busy, externalOpen == nil, droppedPhotos == nil,
            store.command("import_image")["enabled"].bool else { return false }
        let items = providers.compactMap(PhotoItem.provider)
        guard !items.isEmpty else { return false }
        droppedPhotos = (items, placement); busy = true; blocksEditor = true
        cancelled = false; cancelling = false
        store.edit(["type": "invoke", "command": "import_image"]) { [weak self] message in
            guard let self, let message else { return }
            droppedPhotos = nil
            if requestID != nil { fail(message) }
            else { busy = false; blocksEditor = false; cancelling = false; error = message }
        }
        return true
    }
    private func place(_ task: NativeProjectTask, inputs: [PhotoItem], index: Int = 0) {
        guard index < inputs.count else { prepare(task, url: nil, recovery: nil) {}; return }
        let id = requestID, item = inputs[index]
        loadingPhoto = true
        item.load { [weak self, weak task] result in
            guard let self, let task, requestID == id, !finishing else { return }
            loadingPhoto = false
            if cancelled { finish(); return }
            prepare(task, url: nil, recovery: nil, next: { [weak self] in
                self?.place(task, inputs: inputs, index: index + 1)
            }) {
                switch try result.get() {
                case .file(let url): try task.read(from: url)
                case .image(let data): try task.read(image: data, name: item.name)
                }
            }
        }
    }
    private func open(_ url: URL?, options: JSON? = nil) {
        let recovery = recovering
        task(opening: true) { [weak self] task in
            self?.prepare(task, url: url, recovery: recovery) { try task.read(from: url, options: options) }
        }
    }
    private func prepare(_ task: NativeProjectTask, url: URL?, recovery: RecoveryRecord?, next: (() -> Void)? = nil, work: @escaping () throws -> Void) {
        NativeProjectTask.io.async { [weak self] in
            do {
                try work()
                let profile = try task.pendingProfile()
                DispatchQueue.main.async {
                    guard let self else { return }
                    self.interpreting = false
                    if self.cancelled { self.finish(); return }
                    if !profile.isNull {
                        self.pendingProfile = profile; self.profileError = nil
                        self.profileCompletion = { [weak self] choice in
                            guard let self else { return }
                            guard let choice else { self.finish(); return }
                            self.interpreting = true
                            self.prepare(task, url: url, recovery: recovery, next: next) { try task.assumeProfile(choice) }
                        }
                        return
                    }
                    self.profileCompletion = nil; self.pendingProfile = nil
                    if let next { next(); return }
                    self.store?.native?.finishProject(task, opening: true, title: url?.lastPathComponent ?? "Untitled", url: url,
                        recovered: recovery != nil) { [weak self] error in
                        DispatchQueue.main.async {
                            guard let self else { return }
                            if let error { self.report(error) }
                            else if let recovery { self.store?.recovery.didRestore(recovery) }
                            self.recovering = nil
                            self.finish(error == nil)
                        }
                    }
                }
            } catch {
                let message = error.localizedDescription
                DispatchQueue.main.async {
                    guard let self else { return }
                    self.interpreting = false
                    if self.cancelled { self.finish() }
                    else if self.pendingProfile != nil { self.profileError = message }
                    else { self.fail(message) }
                }
            }
        }
    }
    func chooseProfile(_ profile: JSON?) {
        guard !interpreting else { return }
        profileCompletion?(profile)
    }
    private func fail(_ message: String) { report(message); finish() }
    private func report(_ message: String) { if !cancelled { error = message } }
    private func finish(_ succeeded: Bool = false) {
        guard !finishing, let id = requestID else { return }
        finishing = true
        // A durable save is already acknowledged by the owner. Other requests
        // complete only after the picker and worker have finished their effect.
        if store?.state["requests"].array.contains(where: { $0["id"].uint == id }) != true {
            released(); return
        }
        store?.native?.documentRequest(id: id, succeeded: succeeded) { [weak self] error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let error { self.report(error) }
                self.released()
            }
        }
    }
    private func released() {
        requestID = nil; approved = nil; busy = false; blocksEditor = false; finishing = false
        profileCompletion = nil; pendingProfile = nil; profileError = nil; interpreting = false
        activeTask = nil; cancelling = false; exportDelivery = nil; exportEditor = nil
        loadingPhoto = false
        if let state = store?.state.json {
            receive(state)
            if requestID == nil && closeCompletion != nil { finishClose(state["document_file"]["close_ready"].bool) }
            if requestID == nil { recovering = nil; externalOpen = nil }
        }
    }
    private func chooseOpen(photosOnly: Bool = false, _ completion: @escaping ([URL]) -> Void) {
        if let dialogs { dialogs.open(photosOnly, completion); return }
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.allowedContentTypes = (photosOnly ? [] : [.capyProject]) + UTType.capyPhotoTypes; panel.allowsMultipleSelection = photosOnly
        panel.canChooseDirectories = false
        panel.begin { response in completion(response == .OK ? panel.urls : []) }
        #else
        pickerCompletion = completion; picker = Picker(export: nil, types: (photosOnly ? [] : [.capyProject]) + UTType.capyPhotoTypes, multiple: photosOnly)
        #endif
    }
    private func chooseSave(name: String, type: UTType, _ completion: @escaping (URL?) -> Void) {
        if let dialogs { dialogs.save(name, type, completion); return }
        #if os(macOS)
        let panel = NSSavePanel()
        panel.allowedContentTypes = [type]; panel.canCreateDirectories = true
        panel.nameFieldStringValue = URL(fileURLWithPath: name).pathExtension.isEmpty
            ? name + "." + (type == .capyProject ? "capy" : type.preferredFilenameExtension ?? "png") : name
        panel.begin { response in completion(response == .OK ? panel.url : nil) }
        #else
        completion(nil) // iPad uses the export picker above.
        #endif
    }
    func picked(_ urls: [URL]) {
        let completion = pickerCompletion; pickerCompletion = nil; picker = nil
        completion?(urls)
    }
    private nonisolated static func removeStaging(_ url: URL) {
        NativeProjectTask.io.async { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
    }
}

struct ProjectFilesModifier: ViewModifier {
    @ObservedObject var files: ProjectFiles
    func body(content: Content) -> some View {
        content.allowsHitTesting(!files.blocksEditor)
            .overlay(alignment: .bottom) {
                if files.busy && files.activeOperationVisible {
                    HStack {
                        ProgressView().controlSize(.small)
                        Text(files.cancelling ? "Cancelling…" : "Working with document…")
                        Button("Cancel") { files.cancel() }.disabled(files.cancelling)
                    }.padding(10).modifier(EditorPopupSurface(shape: Capsule())).padding(12)
                }
            }
            .alert(files.error == nil ? "Save changes to “\(files.title)” before continuing?" : "Document",
                isPresented: Binding(get: { files.confirming || files.error != nil }, set: { if !$0 { files.dismissAlert() } })) {
                if files.error != nil { Button("OK", role: .cancel) { files.error = nil } }
                else {
                    Button("Save") { files.choose("save") }
                    Button("Discard Changes", role: .destructive) { files.choose("discard") }
                    Button("Cancel", role: .cancel) { files.choose("cancel") }
                }
            } message: { if let error = files.error { Text(error) } }
            .sheet(isPresented: $files.creating, onDismiss: { files.created(nil) }) {
                NewDrawingForm(spec: files.newDocumentSpec, error: files.creationError, busy: files.creationSaving) {
                    files.created($0)
                }.interactiveDismissDisabled(files.creationSaving).modifier(EditorPopupPresentation())
            }
            .sheet(isPresented: Binding(get: { files.pendingProfile != nil }, set: { if !$0 { files.chooseProfile(nil) } })) {
                PhotoProfileForm(preferences: files.preferences, interpretation: files.pendingProfile ?? JSON(),
                    spaces: files.newDocumentSpec["creation"]["spaces"].array, error: files.profileError, busy: files.interpreting) {
                    files.chooseProfile($0)
                }.interactiveDismissDisabled(files.interpreting).modifier(EditorPopupPresentation())
            }
            .sheet(isPresented: Binding(get: { files.colorEditor != nil }, set: { if !$0 { files.colorEditor?.cancel() } })) {
                if let editor = files.colorEditor {
                    DocumentColorForm(editor: editor, spaces: files.newDocumentSpec["creation"]["spaces"].array)
                        .interactiveDismissDisabled(editor.publishing).modifier(EditorPopupPresentation())
                }
            }
            .sheet(isPresented: Binding(get: { files.exportEditor != nil }, set: { if !$0 { files.exportEditor?.cancel() } }),
                onDismiss: files.exportDismissed) {
                if let editor = files.exportEditor { ExportForm(editor: editor).modifier(EditorPopupPresentation()) }
            }
            #if os(iOS)
            // Files may deliver its URL after SwiftUI dismisses this sheet.
            // Only the document-picker delegate completes selection or cancellation.
            .sheet(item: $files.picker) { picker in
                NativeDocumentPicker(export: picker.export, contentTypes: picker.types, multiple: picker.multiple) { files.picked($0) }.ignoresSafeArea()
            }
            #endif
    }
}
private extension ProjectFiles {
    var activeOperationVisible: Bool { !confirming && picker == nil && !creating && pendingProfile == nil && colorEditor == nil && exportEditor == nil }
}

#if os(iOS)
struct NativeDocumentPicker: UIViewControllerRepresentable {
    let export: URL?
    let contentTypes: [UTType]
    let multiple: Bool
    let completion: ([URL]) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(completion) }
    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let controller = export.map { UIDocumentPickerViewController(forExporting: [$0], asCopy: false) }
            ?? UIDocumentPickerViewController(forOpeningContentTypes: contentTypes, asCopy: false)
        controller.allowsMultipleSelection = multiple; controller.delegate = context.coordinator
        return controller
    }
    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let completion: ([URL]) -> Void
        init(_ completion: @escaping ([URL]) -> Void) { self.completion = completion }
        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) { completion(urls) }
        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { completion([]) }
    }
}
#endif
