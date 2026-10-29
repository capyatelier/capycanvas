import SwiftUI
import UniformTypeIdentifiers
#if os(macOS)
import AppKit
import Combine
#else
import UIKit
#endif

/// Native panels and URLs surround the shared document transactions. A save
/// acknowledges its captured revision only after the file operation succeeds.
@MainActor final class ProjectFiles: ObservableObject {
    @Published var busy = false
    @Published var blocksEditor = false
    @Published var confirming = false
    @Published private(set) var confirmationTitle = ""
    @Published var error: String?
    @Published var picker: Picker?
    @Published var cancelling = false
    @Published private(set) var creating = false
    @Published var colorEditor: DocumentColorController?
    @Published var exportEditor: ExportController?
    @Published var packageSummary: JSON?
    @Published var packageImage: CGImage?
    @Published var packageCopying = false
    @Published var pendingProfile: JSON?
    @Published var profileError: String?
    @Published var interpreting = false
    @Published var copyProgress: String?
    private var copying = false
    private var profileCompletion: ((JSON?) -> Void)?
    private var exportDelivery: (() -> Void)?
    private weak var store: EditorStore?
    private var requestID: UInt64?
    private var approved: (UInt64, UInt64)?
    // Keep the native picker's URL, including its security scope. The shared
    // location string identifies the document but cannot recreate file access.
    private var destinations: [UInt64: URL] = [:]
    private var destination: URL? {
        get { destinations[store?.snapshot["document_tabs"]["selected"].uint ?? 1] }
        set { destinations[store?.snapshot["document_tabs"]["selected"].uint ?? 1] = newValue }
    }
    private var queuedOpens: [PhotoItem] = []
    #if os(macOS)
    weak var presentationWindow: NSWindow?
    private var exportDismissal: AnyCancellable?
    #else
    weak var presentationScreen: UIScreen?
    #endif
    var closeWindow: (() -> Void)?
    private var handledClose = false
    private var activeTask: NativeProjectTask?
    private var packageSource: URL?
    private var pickerCompletion: (([URL]) -> Void)?
    private var cancelled = false
    private var finishing = false
    private var externalOpen: (item: PhotoItem, submitted: Bool)?
    private var droppedPhotos: (items: [PhotoItem], placement: JSON)?
    private var loadingPhoto = false
    private var closeCompletion: ((Bool) -> Void)?
    /// Dialog dependency keeps editor/file effects testable without driving
    /// platform panels. The app uses the native implementation by default.
    struct Dialogs {
        var open: (Bool, @escaping ([URL]) -> Void) -> Void
        var save: (String, UTType, @escaping (URL?) -> Void) -> Void
        var create: ((JSON, @escaping ([[String: Any]]?) -> Void) -> Void)? = nil
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
        store?.state["tabs"][0]["title"].string ?? ""
    }
    var alertTitle: String {
        error == nil ? confirmationTitle : store?.bootstrap["recovery"]["attention"].string ?? ""
    }
    func forgetDocument(_ id: UInt64) { destinations.removeValue(forKey: id) }
    func receive(_ state: JSON) {
        submitExternalOpen()
        let file = state["document_file"]
        if !busy { blocksEditor = file["close_ready"].bool }
        if !file["close_ready"].bool { handledClose = false }
        if file["close_ready"].bool && !handledClose && requestID == nil {
            handledClose = true
            if closeCompletion != nil { finishClose(true) } else { store?.drawingTabs.closeApproved() }
        }
        let pending = state["requests"].array.first(where: { $0["kind"]["type"].string == "document" })
        if let document = pending?["kind"]["request"], document["type"].string == "confirm_close",
            confirmationTitle != document["title"].string { confirmationTitle = document["title"].string }
        guard requestID == nil, !finishing, let request = pending else { return }
        requestID = request["id"].uint; busy = true
        if droppedPhotos == nil { cancelling = false; cancelled = false }
        approved = (file["epoch"].uint, file["revision"].uint)
        let document = request["kind"]["request"]
        let action = document["type"].string
        blocksEditor = !["save", "copy"].contains(action) || closeCompletion != nil
        switch action {
        case "save":
            save(to: document["location"]["uri"].string, name: document["name"].string) { [weak self] saved in self?.finish(saved) }
        case "new": openNewDrawing()
        case "export":
            if document["repeat"].isNull { beginExport(name: document["name"].string, owner: document["owner"]) }
            else { exportAgain(name: document["name"].string, owner: document["owner"], repeating: document["repeat"]) }
        case "open":
            if let item = externalOpen?.item { externalOpen = nil; openItem(item) }
            else { chooseOpen { [weak self] urls in
                guard let self else { return }
                if let url = urls.first { queuedOpens.append(contentsOf: urls.dropFirst().map(PhotoItem.init(fileURL:))); open(url) } else { finish() }
            } }
        case "place":
            let drop = droppedPhotos; droppedPhotos = nil
            task(.place, placement: drop?.placement) { [weak self] task in
                if let drop { self?.place(task, inputs: drop.items); return }
                self?.chooseOpen(photosOnly: true) { [weak self] urls in
                    guard let self else { return }
                    if urls.isEmpty { finish() } else { place(task, inputs: urls.map(PhotoItem.init(fileURL:))) }
                }
            }
        case "copy": copyPixels()
        case "paste":
            guard let nonce = PhotoClipboard.nonce, let native = store?.native, let id = requestID else { pasteImages(); return }
            native.hasClip(nonce: nonce) { [weak self] handled in
                DispatchQueue.main.async {
                    guard let self, self.requestID == id else { return }
                    if !handled { self.pasteImages() }
                    else {
                        guard PhotoClipboard.nonce == nonce else { self.pasteImages(); return }
                        self.task(.place, placement: JSON(["nonce": nonce])) { [weak self] task in
                            self?.prepare(task, url: nil) { try task.finishImages() }
                        }
                    }
                }
            }
        case "change_color", "color_history", "properties", "repair_source_profile", "rasterize_source":
            guard let store else { fail("The canvas session is unavailable"); return }
            let editor = DocumentColorController(store: store, request: document, expected: approved) { [weak self] result in
                self?.colorEditor = nil; self?.finish(result)
            }
            colorEditor = editor; editor.load()
        case "import_lookup": importLookup()
        case "confirm_close": confirming = true
        default: fail("This document service is not available yet")
        }
    }
    private func pasteImages() {
        task(.place) { [weak self] task in
            guard let self else { return }
            let id = requestID
            let completed: (Result<[PhotoItem], Error>) -> Void = { [weak self] result in
                guard let self, requestID == id else { return }
                if cancelled { finish(); return }
                switch result {
                case .success(let images):
                    if images.isEmpty { fail(store?.catalog["document_delivery_copy"]["clipboard_empty"].string ?? "") }
                    else { place(task, inputs: images) }
                case .failure(let error): fail(error.localizedDescription)
                }
            }
            if let paste = dialogs?.paste { paste(completed) }
            else { PhotoClipboard.read(tooLarge: store?.catalog["document_delivery_copy"]["clipboard_too_large"].string ?? "", completed) }
        }
    }
    private func copyPixels() {
        copying = true
        task(.clip) { [weak self] task in
            guard let self, let native = store?.native else { return }
            let id = requestID, nonce = UUID().uuidString
            copyProgress = task.clipProgress
            NativeProjectTask.io.async { [weak self] in
                let result = Result { try task.copyPixels(nonce: nonce) }
                DispatchQueue.main.async { [weak self] in
                    guard let self, requestID == id else { return }
                    if cancelled { finish(); return }
                    switch result {
                    case .success(let png):
                        do { try PhotoClipboard.write(png: png, nonce: nonce, failure: store?.catalog["document_delivery_copy"]["clipboard_unavailable"].string ?? "") }
                        catch { fail(error.localizedDescription); return }
                        capy_project_clip_published(nativeValue(task.handle))
                        native.finishProject(task, opening: true, title: "", url: nil) { [weak self] error in
                            DispatchQueue.main.async { [weak self] in
                                guard let self else { return }
                                if let error { report(error) }
                                finish(error == nil)
                            }
                        }
                    case .failure(let error): fail(error.localizedDescription)
                    }
                }
            }
        }
    }
    func openItems(_ items: [PhotoItem]) {
        guard !items.isEmpty else { return }
        queuedOpens.append(contentsOf: items)
        submitQueuedOpen()
    }
    func openURL(_ url: URL) { openItems([PhotoItem(fileURL: url)]) }
    private func submitQueuedOpen() {
        guard !busy, externalOpen == nil, !queuedOpens.isEmpty else { return }
        externalOpen = (queuedOpens.removeFirst(), false)
        submitExternalOpen()
    }
    func submitExternalOpen() {
        guard externalOpen?.submitted == false, let store,
            store.workspaces?.ready != false, !store.recovery.restoring, store.snapshot["canvas_ready"].bool,
            store.command("open_document")["enabled"].bool else { return }
        externalOpen?.submitted = true
        // Admission and command dispatch share the serial owner. A New/Open
        // queued ahead of this URL must leave it waiting, not lose the URL.
        store.query(["type": "document_tabs", "op": "open"]) { [weak self] accepted in
            guard let self, !accepted.bool else { return }
            externalOpen?.submitted = false
            submitExternalOpen()
        }
    }
    func confirmClose(_ completion: @escaping (Bool) -> Void) {
        guard !busy, externalOpen == nil, closeCompletion == nil, let store else { completion(false); return }
        busy = true; blocksEditor = true; closeCompletion = completion
        store.query(["type": "document_tabs", "op": "session_close"]) { [weak self] result in
            guard let self, closeCompletion != nil else { return }
            finishClose(result.bool)
        }
    }
    func confirmCurrentClose(_ completion: @escaping (Bool) -> Void) {
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
                if choice == "cancel" { self.externalOpen = nil }
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
        queuedOpens.removeAll()
        cancelled = true; cancelling = true; activeTask?.cancel()
        // Provider delivery can take arbitrarily long. Its late callback is
        // rejected by request ID; cancellation need not wait for that callback.
        if loadingPhoto { finish() }
    }
    private func task(_ kind: NativeProjectTask.Kind, placement: JSON? = nil, _ ready: @escaping (NativeProjectTask) -> Void) {
        guard let native = store?.native else { fail("The canvas session is unavailable"); return }
        native.projectTask(kind: kind, expected: [.open, .place].contains(kind) ? approved : nil,
            placement: placement) { [weak self] task, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if self.cancelled { self.finish(); return }
                guard let task else { self.fail(error ?? "Document is unavailable"); return }
                self.activeTask = task; ready(task)
            }
        }
    }
    var discardLabel: String { store?.catalog["new_document"]["discard"].string ?? "" }
    var documentSpaces: [JSON] { store?.catalog["document_spaces"].array ?? [] }
    private func openNewDrawing() {
        guard let store else { fail("The canvas session is unavailable"); return }
        creating = true
        store.edit(["type": "new_drawing", "action": ["op": "open", "screen": newDrawingScreen as Any? ?? NSNull(),
            "untitled": store.snapshot["document_tabs"]["upcoming_id"].raw]]) { [weak self] error in
            guard let self else { return }
            if let error { creating = false; fail(error); return }
            dialogs?.create?(store.state["new_drawing"]) { [weak self] edits in
                guard let self else { return }
                if let edits { newDrawing(steps: edits + [["op": "create"]]) } else { newDrawing(["op": "cancel"]) }
            }
        }
    }
    private func newDrawing(steps: [[String: Any]]) {
        guard let step = steps.first else { return }
        newDrawing(step) { [weak self] error in
            guard let self else { return }
            if let error { report(error); newDrawing(["op": "cancel"]) } else { newDrawing(steps: Array(steps.dropFirst())) }
        }
    }
    func newDrawing(_ action: [String: Any], completion: @escaping @MainActor (String?) -> Void = { _ in }) {
        guard let store else { return }
        store.edit(["type": "new_drawing", "action": action]) { [weak self] error in
            completion(error)
            guard let self, creating, store.state["new_drawing"].isNull else { return }
            creating = false
            if error == nil && action["op"] as? String == "create" { open(nil) } else { finish() }
        }
    }
    private var newDrawingScreen: [String: Any]? {
        #if os(macOS)
        return Self.screenInfo(presentationWindow?.screen)
        #else
        return Self.screenInfo(presentationScreen)
        #endif
    }
    #if os(macOS)
    static func screenInfo(_ screen: NSScreen? = nil) -> [String: Any]? {
        guard let screen = screen ?? NSScreen.main else { return nil }
        let scale = screen.backingScaleFactor
        let extent = [screen.frame.width, screen.frame.height].map { UInt32(max(1, ($0 * scale).rounded())) }
        let display = (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value
        let millimetres = display.map { CGDisplayScreenSize(CGDirectDisplayID($0)).width } ?? 0
        let ppi: Any = millimetres > 0 ? UInt32(max(1, (CGFloat(extent[0]) * 25.4 / millimetres).rounded())) : NSNull()
        return ["extent": extent, "ppi": ppi]
    }
    #else
    static func screenInfo(_ screen: UIScreen? = nil) -> [String: Any]? {
        let screen = screen ?? UIScreen.main
        let extent = [screen.bounds.width, screen.bounds.height].map { UInt32(max(1, ($0 * screen.nativeScale).rounded())) }
        return ["extent": extent, "ppi": NSNull()]
    }
    #endif
    private func save(to uri: String, name: String, completion: @escaping (Bool) -> Void) {
        if let destination, destination.absoluteString == uri { write(destination, completion: completion); return }
        guard let store, let native = store.native, let id = requestID else { completion(false); return }
        let identity = store.drawingIdentity
        native.savedDestination(uri) { [weak self] destination in DispatchQueue.main.async {
            guard let self else { completion(false); return }
            guard self.requestID == id, self.store?.drawingIdentity == identity else { return }
            guard !self.cancelled else { completion(false); return }
            if let destination { self.write(destination, expectedURI: uri, completion: completion) }
            else { self.saveAs(name: name, completion: completion) }
        } }
    }
    private func saveAs(name: String, completion: @escaping (Bool) -> Void) {
        task(.save) { [weak self] task in
            guard let self else { return }
            deliver(task, name: name, type: .capyProject) { [weak self] url in
                guard let self, let url else { completion(false); return }
                saved(task, at: url, completion: completion)
            }
        }
    }
    private func exportFilename(_ name: String, recipe: JSON) -> String {
        URL(fileURLWithPath: name).deletingPathExtension().lastPathComponent + "." + Self.exportExtension(recipe["format"].string)
    }
    private func exportDestination(_ url: URL, owner: JSON, recipe: JSON, completion: @escaping (Bool) -> Void) {
        guard let id = requestID, let store else { completion(false); return }
        store.edit(["type": "prepare_export", "id": id, "owner": owner.raw, "recipe": recipe.raw,
            "location": ["uri": url.absoluteString, "name": url.lastPathComponent]]) { [weak self] error in
            if let error { self?.report(error) }
            completion(error == nil)
        }
    }
    private func exportAgain(name: String, owner: JSON, repeating: JSON) {
        guard let id = requestID, let native = store?.native else { fail("The canvas is unavailable"); return }
        let recipe = repeating["recipe"]
        let outsideRange = store?.catalog["export_copy"]["outside_range"].string ?? ""
        native.exportTask(id: id) { [weak self] task, error in
            DispatchQueue.main.async {
                guard let self, self.requestID == id else { task?.cancel(); return }
                guard let task else { self.fail(error ?? "The drawing is unavailable"); return }
                self.activeTask = task
                NativeProjectTask.io.async { [weak self] in
                    do {
                        try task.configureExport(recipe)
                        if recipe["format"].string.contains("Hdr") {
                            try task.compare()
                            if try task.details()["range_blocked"].bool {
                                throw HostFailure(message: outsideRange)
                            }
                        }
                        DispatchQueue.main.async { self?.deliverAgain(task, name: name, owner: owner, recipe: recipe,
                            remembered: URL(string: repeating["location"]["uri"].string)) }
                    } catch { let message = error.localizedDescription
                        DispatchQueue.main.async { self?.fail(message) }
                    }
                }
            }
        }
    }
    private func deliverAgain(_ task: NativeProjectTask, name: String, owner: JSON, recipe: JSON, remembered: URL?) {
        let filename = remembered?.lastPathComponent ?? exportFilename(name, recipe: recipe)
        let choose = { [weak self] in
            guard let self else { return }
            self.deliver(task, name: filename, type: Self.exportType(recipe["format"].string), prepare: { [weak self] url, ready in
                self?.exportDestination(url, owner: owner, recipe: recipe, completion: ready) ?? ready(false)
            }) { [weak self] url in self?.finish(url != nil) }
        }
        guard !usesExportPicker, let remembered, remembered.isFileURL,
            FileManager.default.isWritableFile(atPath: remembered.path) else { choose(); return }
        exportDestination(remembered, owner: owner, recipe: recipe) { [weak self] ready in
            guard let self else { return }
            guard ready else { finish(); return }
            NativeProjectTask.io.async { [weak self] in
                do {
                    try task.write(to: remembered)
                    DispatchQueue.main.async { self?.finish(true) }
                } catch { DispatchQueue.main.async { choose() } }
            }
        }
    }
    private func beginExport(name: String, owner: JSON) {
        guard let id = requestID, let native = store?.native else { fail("The canvas is unavailable"); return }
        let editor = ExportController(native: native, request: id, preferences: preferences,
            copy: { [weak store = self.store] in store?.catalog["export_copy"] ?? JSON() }) { [weak self] task, recipe, destination, color in
            guard let self, self.requestID == id else { task?.cancel(); return }
            guard let task else { self.finish(); return }
            self.activeTask = task
            let type = Self.exportType(recipe["format"].string)
            let filename = self.exportFilename(name, recipe: recipe)
            self.exportDelivery = { [weak self] in
                guard let self else { return }
                if self.cancelled { self.finish(); return }
                self.blocksEditor = false
                self.deliver(task, name: filename, type: type, prepare: { [weak self] url, ready in
                    self?.exportDestination(url, owner: owner, recipe: recipe, completion: ready) ?? ready(false)
                }) { [weak self] url in
                    guard let self else { return }
                    guard url != nil else { self.finish(); return }
                    guard self.preferences.canSave else { self.finish(true); return }
                    let preferences = self.preferences
                    NativeProjectTask.io.async { [weak self] in
                        let result = Result { try preferences.presets(color: color, request:
                            JSON(["type": "remember", "index": destination < 4 ? destination : 3, "recipe": recipe.raw])) }
                        DispatchQueue.main.async {
                            if case .failure(let error) = result {
                                let detail = error.localizedDescription
                                if let native = self?.store?.native {
                                    native.submit(2, JSON(["type": "document_delivery_message", "message":
                                        ["type": "export_preferences", "detail": detail]])) { [weak self] copy in
                                        DispatchQueue.main.async {
                                            guard let self, self.requestID == id else { return }
                                            self.report(copy?.string ?? detail); self.finish(true)
                                        }
                                    }
                                    return
                                }
                                self?.report(detail)
                            }
                            self?.finish(true)
                        }
                    }
                }
            }
            self.exportEditor = nil
            if self.dialogs?.exportOptions != nil { self.exportDismissed() }
        }
        exportEditor = editor
        editor.load { [weak self, weak editor] in
            if let editor { self?.dialogs?.exportOptions?(editor) }
        }
    }
    /// Present the native file picker after the export sheet has dismissed.
    func exportDismissed() {
        guard exportDelivery != nil else { return }
        #if os(macOS)
        if let window = presentationWindow, window.attachedSheet != nil {
            if exportDismissal == nil {
                exportDismissal = NotificationCenter.default.publisher(for: NSWindow.didEndSheetNotification, object: window)
                    .first().receive(on: DispatchQueue.main).sink { [weak self] _ in
                        self?.exportDismissal = nil
                        self?.exportDismissed()
                    }
            }
            return
        }
        exportDismissal = nil
        #endif
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
    private func deliver(_ task: NativeProjectTask, name: String, type: UTType, previewOnly: Bool = false,
        prepare: ((URL, @escaping (Bool) -> Void) -> Void)? = nil, completion: @escaping (URL?) -> Void) {
        let original = previewOnly ? packageSource : nil
        let destinationError = packageSummary?["destination_error"].string ?? ""
        if !usesExportPicker {
            chooseSave(name: name, type: type) { [weak self] url in
                guard let self, let url else { completion(nil); return }
                (prepare ?? { $1(true) })(url) { ready in
                    guard ready else { completion(nil); return }
                    NativeProjectTask.io.async {
                        do { try task.write(to: url, previewOnly: previewOnly, original: original, destinationError: destinationError); DispatchQueue.main.async { completion(url) } }
                        catch { let message = error.localizedDescription
                            DispatchQueue.main.async { self.report(message); completion(nil) }
                        }
                    }
                }
            }
        } else {
            NativeProjectTask.io.async { [weak self] in
                do {
                    let suffix = URL(fileURLWithPath: name).pathExtension
                    let staging = try ProjectFileIO.stagingURL(title: name, extension: type == .capyProject ? "capy" : suffix.isEmpty ? type.preferredFilenameExtension ?? "png" : suffix)
                    do { try task.write(to: staging, previewOnly: previewOnly, original: original, destinationError: destinationError) }
                    catch { try? FileManager.default.removeItem(at: staging.deletingLastPathComponent()); throw error }
                    DispatchQueue.main.async {
                        guard let self else { Self.removeStaging(staging); return }
                        if self.cancelled { Self.removeStaging(staging); completion(nil); return }
                        let completed: (URL?) -> Void = { url in
                            Self.removeStaging(staging)
                            guard let url, let prepare else { completion(url); return }
                            prepare(url) { completion($0 ? url : nil) }
                        }
                        if let export = self.dialogs?.export { export(staging, completed) }
                        else { self.pickerCompletion = { completed($0.first) }; self.picker = Picker(export: staging, types: []) }
                    }
                } catch { let message = error.localizedDescription
                    DispatchQueue.main.async { self?.report(message); completion(nil) }
                }
            }
        }
    }
    private func write(_ url: URL, expectedURI: String? = nil, completion: @escaping (Bool) -> Void) {
        task(.save) { [weak self] task in
            NativeProjectTask.io.async {
                do {
                    try task.write(to: url, expectedURI: expectedURI)
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
    func dropAvailable(_ providers: [NSItemProvider], layer: Bool) -> Bool {
        let command = fileDrop(providers.map(PhotoItem.declaresDrawing), layer: layer)["command"]
        return command.isNull || store?.command(command.string)["enabled"].bool == true
    }
    func drop(_ providers: [NSItemProvider], placement: JSON) -> Bool {
        guard store != nil, !busy, externalOpen == nil, droppedPhotos == nil, !providers.isEmpty else { return false }
        busy = true; blocksEditor = true; cancelled = false; cancelling = false
        PhotoItem.drawings(providers[...]) { [weak self] drawings in self?.receiveDrop(providers, drawings: drawings, placement: placement) }
        return true
    }
    private func receiveDrop(_ providers: [NSItemProvider], drawings: [Bool], placement: JSON) {
        let decision = fileDrop(drawings, layer: !placement["layer"].isNull)
        let command = decision["command"].string, opening = command == "open_document"
        let items = providers.compactMap(opening ? PhotoItem.drawingProvider : PhotoItem.provider)
        guard let store, !cancelled, !items.isEmpty, opening || store.command(command)["enabled"].bool else {
            busy = false; blocksEditor = false; cancelling = false
            if !cancelled, !decision["error"].isNull { error = decision["error"].string }
            return
        }
        if opening { busy = false; blocksEditor = false; openItems(items); return }
        droppedPhotos = (items, placement)
        store.edit(["type": "invoke", "command": command]) { [weak self] message in
            guard let self, let message else { return }
            droppedPhotos = nil
            if requestID != nil { fail(message) }
            else { busy = false; blocksEditor = false; cancelling = false; error = message }
        }
    }
    private func fileDrop(_ drawings: [Bool], layer: Bool) -> JSON {
        do {
            let text = try JSON(["language": store?.interfaceLanguage ?? "en", "request": ["drawings": drawings, "layer": layer]]).encoded()
            guard let response = text.withCString({ takeNativeText(capy_apple_file_drop($0)) }) else { return JSON() }

            return try JSON.decode(response)
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    private func place(_ task: NativeProjectTask, inputs: [PhotoItem], index: Int = 0, representation: Int = 0) {
        guard index < inputs.count else { prepare(task, url: nil) { try task.finishImages() }; return }
        let id = requestID, item = inputs[index]
        loadingPhoto = true
        item.representations[representation]({ [weak self, weak task] result in
            guard let self, let task, requestID == id, !finishing else { return }
            loadingPhoto = false
            if cancelled { finish(); return }
            prepare(task, url: nil, next: { [weak self] in
                self?.place(task, inputs: inputs, index: index + 1)
            }, retry: representation + 1 < item.representations.count ? { [weak self] in
                self?.place(task, inputs: inputs, index: index, representation: representation + 1)
            } : nil) {
                switch try result.get() {
                case .file(let url): try task.read(from: url)
                case .image(let data): try task.read(image: data, name: item.name)
                }
            }
        })
    }
    private func openItem(_ item: PhotoItem) {
        store?.recovery.flush { [weak self] saved in
            guard let self else { return }
            guard saved else { fail(store?.recovery.failure ?? "Could not preserve the current drawing for recovery"); return }
            task(.open) { [weak self] task in
                guard let self else { return }
                let id = requestID; loadingPhoto = true
                item.load { [weak self, weak task] result in
                    guard let self, let task, requestID == id, !finishing else { return }
                    loadingPhoto = false
                    if cancelled { finish(); return }
                    do {
                        switch try result.get() {
                        case .file(let url): prepare(task, url: url) { try task.read(from: url) }
                        case .image(let data): prepare(task, url: nil) { try task.read(image: data, name: item.name) }
                        }
                    } catch { fail(error.localizedDescription) }
                }
            }
        }
    }
    private func open(_ url: URL?) {
        // Preserve the outgoing drawing's checkpoint before it becomes inactive.
        store?.recovery.flush { [weak self] saved in
            guard let self else { return }
            guard saved else { fail(store?.recovery.failure ?? "Could not preserve the current drawing for recovery"); return }
            beginOpen(url)
        }
    }
    private func beginOpen(_ url: URL?) {
        task(.open) { [weak self] task in
            self?.prepare(task, url: url) { try task.read(from: url) }
        }
    }
    private func prepare(_ task: NativeProjectTask, url: URL?, next: (() -> Void)? = nil, retry: (() -> Void)? = nil, work: @escaping () throws -> Void) {
        NativeProjectTask.io.async { [weak self] in
            do {
                try work()
                let package = try task.packageSummary()
                if !package.isNull {
                    let image = try task.packagePreview()
                    DispatchQueue.main.async {
                        guard let self else { return }
                        if self.cancelled { self.finish(); return }
                        self.interpreting = false
                        self.packageSummary = package; self.packageImage = image; self.packageSource = url
                    }
                    return
                }
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
                            self.prepare(task, url: url, next: next) { try task.assumeProfile(choice) }
                        }
                        return
                    }
                    self.profileCompletion = nil; self.pendingProfile = nil
                    if let next { next(); return }
                    self.store?.native?.finishProject(task, opening: true, title: url?.lastPathComponent ?? "Untitled", url: url) { [weak self] error in
                        DispatchQueue.main.async {
                            guard let self else { return }
                            if let error { self.report(error) }
                            else {
                                self.destination = url
                            }
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
                    else if let retry { retry() }
                    else { self.fail(message) }
                }
            }
        }
    }
    func closePackage() {
        guard !packageCopying else { return }
        packageSummary = nil; packageImage = nil; packageSource = nil
        finish()
    }
    func copyPackage() {
        guard !packageCopying, let task = activeTask else { return }
        packageCopying = true
        deliver(task, name: "Copy.capy", type: .capyProject) { [weak self] _ in
            guard let self else { return }
            self.packageCopying = false
            self.closePackage()
        }
    }
    func exportPackagePreview() {
        guard !packageCopying, let task = activeTask else { return }
        packageCopying = true
        deliver(task, name: "Preview.png", type: .png, previewOnly: true) { [weak self] _ in
            guard let self else { return }
            self.packageCopying = false
            self.closePackage()
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
        #if os(macOS)
        exportDismissal = nil
        #endif
        requestID = nil; approved = nil; busy = false; blocksEditor = false; finishing = false
        profileCompletion = nil; pendingProfile = nil; profileError = nil; interpreting = false
        packageSummary = nil; packageImage = nil; packageSource = nil; packageCopying = false
        activeTask = nil; cancelling = false; exportDelivery = nil; exportEditor = nil
        loadingPhoto = false; copying = false; copyProgress = nil
        if let state = store?.state.json {
            receive(state)
            if requestID == nil && closeCompletion != nil { finishClose(state["document_file"]["close_ready"].bool) }
            if requestID == nil {
                if !cancelled { submitQueuedOpen() }
            }
        }
    }
    private func importLookup() {
        let types = [UTType(filenameExtension: "cube", conformingTo: .data) ?? .data]
        let chosen: ([URL]) -> Void = { [weak self] urls in
            guard let self else { return }
            guard let url = urls.first else { finish(); return }
            task(.lookup) { [weak self] task in
                NativeProjectTask.io.async { [weak self] in
                    do {
                        try task.read(from: url)
                        DispatchQueue.main.async {
                            self?.store?.native?.finishProject(task, opening: true, title: url.lastPathComponent, url: nil) { [weak self] error in
                                DispatchQueue.main.async {
                                    if let error { self?.report(error) }
                                    self?.finish(error == nil)
                                }
                            }
                        }
                    } catch { DispatchQueue.main.async { self?.fail(error.localizedDescription) } }
                }
            }
        }
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.allowedContentTypes = types; panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        present(panel) { response in chosen(response == .OK ? panel.urls : []) }
        #else
        pickerCompletion = chosen; picker = Picker(export: nil, types: types)
        #endif
    }
    func choosePaper(_ completion: @escaping (URL?) -> Void) {
        chooseOpen(photosOnly: true, multiple: false) { completion($0.first) }
    }
    private func chooseOpen(photosOnly: Bool = false, multiple: Bool = true, _ completion: @escaping ([URL]) -> Void) {
        if let dialogs { dialogs.open(photosOnly, completion); return }
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.allowedContentTypes = (photosOnly ? [] : [.capyProject]) + UTType.capyPhotoTypes; panel.allowsMultipleSelection = multiple
        panel.canChooseDirectories = false
        present(panel) { response in completion(response == .OK ? panel.urls : []) }
        #else
        pickerCompletion = completion; picker = Picker(export: nil, types: (photosOnly ? [] : [.capyProject]) + UTType.capyPhotoTypes, multiple: multiple)
        #endif
    }
    static func exportType(_ format: String) -> UTType {
        if format.hasPrefix("Jpeg") { return .jpeg }
        if format.hasPrefix("Avif") { return UTType(filenameExtension: "avif") ?? UTType("public.avif") ?? .data }
        if format == "Exr" { return UTType(filenameExtension: "exr") ?? UTType(exportedAs: "art.capycanvas.openexr", conformingTo: .image) }
        if format == "Webp" { return .webP }
        return format == "Tiff" ? .tiff : .png
    }
    static func exportExtension(_ format: String) -> String {
        if format.hasPrefix("Jpeg") { return "jpg" }
        if format.hasPrefix("Avif") { return "avif" }
        if format == "Webp" { return "webp" }
        return format == "Exr" ? "exr" : format == "Tiff" ? "tiff" : "png"
    }
    private func chooseSave(name: String, type: UTType, _ completion: @escaping (URL?) -> Void) {
        if let dialogs { dialogs.save(name, type, completion); return }
        #if os(macOS)
        let panel = NSSavePanel()
        panel.allowedContentTypes = [type]; panel.canCreateDirectories = true
        panel.nameFieldStringValue = URL(fileURLWithPath: name).pathExtension.isEmpty
            ? name + "." + (type == .capyProject ? "capy" : type.preferredFilenameExtension ?? "png") : name
        present(panel) { response in completion(response == .OK ? panel.url : nil) }
        #else
        completion(nil) // iPad uses the export picker above.
        #endif
    }
    #if os(macOS)
    private func present(_ panel: NSSavePanel, completion: @escaping (NSApplication.ModalResponse) -> Void) {
        guard let presentationWindow else {
            completion(.cancel)
            error = store?.bootstrap["recovery"]["drawing_window_unavailable"].string
            return
        }
        var presenter = presentationWindow
        while let sheet = presenter.attachedSheet { presenter = sheet }
        panel.beginSheetModal(for: presenter, completionHandler: completion)
    }
    #endif
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
    @Environment(\.capyCommonCopy) private var common
    func body(content: Content) -> some View {
        content.allowsHitTesting(!files.blocksEditor)
            .accessibilityHidden(files.blocksEditor)
            .alert(files.alertTitle,
                isPresented: Binding(get: { files.confirming || files.error != nil }, set: { if !$0 { files.dismissAlert() } })) {
                if files.error != nil { Button(common["ok"].string, role: .cancel) { files.error = nil } }
                else {
                    Button(common["save"].string) { files.choose("save") }
                    Button(files.discardLabel, role: .destructive) { files.choose("discard") }
                    Button(common["cancel"].string, role: .cancel) { files.choose("cancel") }
                }
            } message: { if let error = files.error { Text(error) } }
            .sheet(isPresented: Binding(get: { files.packageSummary != nil }, set: { if !$0 { files.closePackage() } })) {
                if let summary = files.packageSummary {
                    VStack(alignment: .leading, spacing: 16) {
                        Text(summary["status"].string).font(.headline)
                        if let image = files.packageImage {
                            Image(decorative: image, scale: 1).resizable().aspectRatio(contentMode: .fit).frame(maxHeight: 420)
                        }
                        Text(summary["reason"].string)
                        HStack {
                            Button(summary["close"].string) { files.closePackage() }.disabled(files.packageCopying)
                            Spacer()
                            if summary["capabilities"]["export"].bool {
                                Button(summary["export_preview"].string) { files.exportPackagePreview() }.disabled(files.packageCopying)
                                    .accessibilityIdentifier("export-package-preview")
                            }
                            Button(summary["copy_original"].string) { files.copyPackage() }.disabled(files.packageCopying)
                                .accessibilityIdentifier("copy-original-package")
                        }
                    }.padding(24).frame(minWidth: 320, idealWidth: 560)
                        .accessibilityElement(children: .contain)
                        .accessibilityIdentifier("preserved-package-preview")
                        .interactiveDismissDisabled(files.packageCopying).modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation())
                }
            }
            .sheet(isPresented: Binding(get: { files.pendingProfile != nil }, set: { if !$0 { files.chooseProfile(nil) } })) {
                PhotoProfileForm(preferences: files.preferences, interpretation: files.pendingProfile ?? JSON(),
                    spaces: files.documentSpaces, error: files.profileError, busy: files.interpreting) {
                    files.chooseProfile($0)
                }.interactiveDismissDisabled(files.interpreting).modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation())
            }
            .sheet(isPresented: Binding(get: { files.colorEditor != nil }, set: { if !$0 { files.colorEditor?.cancel() } })) {
                if let editor = files.colorEditor {
                    DocumentColorForm(editor: editor, spaces: files.documentSpaces)
                        .interactiveDismissDisabled(editor.publishing).modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation())
                }
            }
            .sheet(isPresented: Binding(get: { files.exportEditor != nil }, set: { if !$0 { files.exportEditor?.cancel() } }),
                onDismiss: files.exportDismissed) {
                if let editor = files.exportEditor { ExportForm(editor: editor).modifier(EditorFittedPresentation()).modifier(EditorPopupPresentation()) }
            }
            #if os(iOS)
            .background {
                NativeDocumentPicker(picker: files.picker) { files.picked($0) }
                    .frame(width: 0, height: 0).allowsHitTesting(false).accessibilityHidden(true)
            }
            #endif
    }
}
struct ProjectProgressNotice: View {
    @ObservedObject var files: ProjectFiles
    @Environment(\.capyCommonCopy) private var common
    let copy: JSON
    var body: some View {
        if files.busy && files.activeOperationVisible {
            EditorNotice(message: files.copyProgress ?? copy["loading"].string, working: true) {
                Button(common["cancel"].string) { files.cancel() }.disabled(files.cancelling)
            }
        }
    }
}

extension ProjectFiles {
    fileprivate var activeOperationVisible: Bool { (!copying || copyProgress != nil) && !confirming && picker == nil && !creating && pendingProfile == nil && packageSummary == nil && colorEditor == nil && exportEditor == nil }
}

#if os(iOS)
struct NativeDocumentPicker: UIViewControllerRepresentable {
    let picker: ProjectFiles.Picker?
    let completion: ([URL]) -> Void
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeUIViewController(context: Context) -> Anchor {
        let anchor = Anchor()
        anchor.ready = { [weak coordinator = context.coordinator] in coordinator?.present() }
        context.coordinator.anchor = anchor
        return anchor
    }
    func updateUIViewController(_ controller: Anchor, context: Context) {
        context.coordinator.picker = picker
        context.coordinator.completion = completion
        context.coordinator.present()
    }
    static func dismantleUIViewController(_ controller: Anchor, coordinator: Coordinator) {
        controller.ready = nil
        coordinator.stopped = true
        coordinator.picker = nil
        coordinator.completion = nil
        coordinator.dismiss()
    }
    final class Anchor: UIViewController {
        var ready: (() -> Void)?
        override func loadView() {
            view = UIView()
            view.backgroundColor = .clear
            view.isUserInteractionEnabled = false
        }
        override func viewDidAppear(_ animated: Bool) { super.viewDidAppear(animated); ready?() }
        override func viewDidLayoutSubviews() { super.viewDidLayoutSubviews(); ready?() }
    }
    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        weak var anchor: Anchor?
        var picker: ProjectFiles.Picker?
        var completion: (([URL]) -> Void)?
        var stopped = false
        private var controller: UIDocumentPickerViewController?
        private var identity: UUID?
        private var transitioning = false
        func present() {
            guard !transitioning else { return }
            if stopped { dismiss(); return }
            if controller != nil {
                if picker == nil || identity != picker?.id { dismiss() }
                return
            }
            guard let picker, let anchor, let window = anchor.viewIfLoaded?.window else { return }
            var presenter: UIViewController = anchor
            while let parent = presenter.parent { presenter = parent }
            while let presented = presenter.presentedViewController { presenter = presented }
            if let transition = presenter.transitionCoordinator ?? presenter.presentingViewController?.transitionCoordinator {
                transitioning = true
                if transition.animate(alongsideTransition: nil, completion: { [weak self] _ in
                    self?.transitioning = false
                    self?.present()
                }) { return }
                transitioning = false
            }
            guard !presenter.isBeingPresented, !presenter.isBeingDismissed,
                presenter.viewIfLoaded?.window === window else { return }
            let controller = picker.export.map { UIDocumentPickerViewController(forExporting: [$0], asCopy: false) }
                ?? UIDocumentPickerViewController(forOpeningContentTypes: picker.types, asCopy: false)
            controller.allowsMultipleSelection = picker.multiple
            controller.delegate = self
            self.controller = controller
            identity = picker.id
            transitioning = true
            presenter.present(controller, animated: true) {
                self.transitioning = false
                self.present()
            }
        }
        func dismiss() {
            guard !transitioning, let controller else { return }
            transitioning = true
            controller.dismiss(animated: true) {
                self.controller = nil
                self.identity = nil
                self.transitioning = false
                self.present()
            }
        }
        private func complete(_ controller: UIDocumentPickerViewController, _ urls: [URL]) {
            guard self.controller === controller, identity != nil else { return }
            identity = nil
            picker = nil
            completion?(urls)
            dismiss()
        }
        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            complete(controller, urls)
        }
        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { complete(controller, []) }
    }
}
#endif
