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
    private var creationCompletion: (([UInt32]?) -> Void)?
    private var exportPreparing = false
    private weak var store: EditorStore?
    private var requestID: UInt64?
    private var destination: URL?
    var closeWindow: (() -> Void)?
    private var handledClose = false
    private var activeTask: NativeProjectTask?
    private var pickerCompletion: ((URL?) -> Void)?
    private var cancelled = false
    private var finishing = false
    private var externalURL: URL?
    private var recovering: RecoveryRecord?
    private var closeCompletion: ((Bool) -> Void)?
    /// Dialog dependency keeps editor/file effects testable without driving
    /// platform panels. The app uses the native implementation by default.
    struct Dialogs {
        var open: (@escaping (URL?) -> Void) -> Void
        var save: (String, UTType, @escaping (URL?) -> Void) -> Void
        var create: ((JSON, @escaping ([UInt32]?) -> Void) -> Void)? = nil
        var export: ((URL, @escaping (URL?) -> Void) -> Void)? = nil
    }
    private let dialogs: Dialogs?
    struct Picker: Identifiable {
        let id = UUID()
        let export: URL?
    }
    init(store: EditorStore, dialogs: Dialogs? = nil) { self.store = store; self.dialogs = dialogs }
    var title: String {
        let name = store?.state["document_file"]["location"]["name"].string ?? ""
        return name.isEmpty ? "Untitled" : name
    }
    func receive(_ state: JSON) {
        let file = state["document_file"]
        if !busy { blocksEditor = file["close_ready"].bool }
        if !file["close_ready"].bool { handledClose = false }
        if file["close_ready"].bool && !handledClose {
            handledClose = true
            if closeCompletion != nil { finishClose(true) } else { closeWindow?() }
        }
        guard requestID == nil, !finishing,
            let request = state["requests"].array.first(where: { $0["kind"]["type"].string == "document" }) else { return }
        requestID = request["id"].uint; busy = true; cancelling = false; cancelled = false
        let document = request["kind"]["request"]
        let action = document["type"].string
        blocksEditor = action == "open" || action == "new" || action == "confirm_close" || action == "export" || closeCompletion != nil
        switch action {
        case "save":
            destination = URL(string: document["location"]["uri"].string)
            save(as: document["location"].isNull) { [weak self] saved in self?.finish(saved) }
        case "new":
            let completed: ([UInt32]?) -> Void = { [weak self] extent in
                guard let self else { return }
                if let extent { open(nil, extent: extent) } else { finish() }
            }
            if let create = dialogs?.create { create(newDocumentSpec, completed) }
            else { creationCompletion = completed; creating = true }
        case "export": exportPNG(name: document["name"].string)
        case "open":
            if let url = externalURL { externalURL = nil; open(url) }
            else { chooseOpen { [weak self] url in
                guard let self else { return }
                if let url { open(url) } else { finish() }
            } }
        case "confirm_close": confirming = true
        default: fail("This document service is not available yet")
        }
    }
    func openURL(_ url: URL) {
        guard !busy else { error = "Finish the current document operation first"; return }
        guard store?.command("open_document")["enabled"].bool == true else {
            error = "Finish the canvas interaction before opening a drawing"; return
        }
        externalURL = url; store?.invoke("open_document")
    }
    func recover(_ record: RecoveryRecord) {
        guard !busy, let url = store?.recovery.files.archive(record),
            store?.command("open_document")["enabled"].bool == true else {
            error = "Finish the current canvas operation before recovering a drawing"; return
        }
        recovering = record; openURL(url)
    }
    func confirmClose(_ completion: @escaping (Bool) -> Void) {
        guard !busy, let native = store?.native else { completion(false); return }
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
                if choice == "cancel" { self.externalURL = nil; self.recovering = nil }
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
        cancelled = true; cancelling = true; activeTask?.cancel()
        if exportPreparing && activeTask == nil { finish() }
    }
    private func task(opening: Bool, _ ready: @escaping (NativeProjectTask) -> Void) {
        guard let native = store?.native else { fail("The canvas session is unavailable"); return }
        let file = store?.state["document_file"] ?? JSON()
        native.projectTask(opening: opening, expected: opening ? (file["epoch"].uint, file["revision"].uint) : nil) { [weak self] task, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if self.cancelled { self.finish(); return }
                guard let task else { self.fail(error ?? "Document is unavailable"); return }
                self.activeTask = task; ready(task)
            }
        }
    }
    var newDocumentSpec: JSON { store?.catalog["new_document"] ?? JSON() }
    func created(_ extent: [UInt32]?) {
        let completion = creationCompletion; creationCompletion = nil; creating = false
        completion?(extent)
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
    private func exportPNG(name: String) {
        guard let id = requestID, let native = store?.native else { fail("The canvas is unavailable"); return }
        exportPreparing = true
        native.exportTask(id: id) { [weak self] task, error in
            DispatchQueue.main.async {
                guard let self, self.requestID == id else { return }
                self.exportPreparing = false
                if self.cancelled { self.finish(); return }
                guard let task else { self.fail(error ?? "Export failed"); return }
                self.activeTask = task; self.blocksEditor = false
                self.deliver(task, name: name, type: .png) { [weak self] url in self?.finish(url != nil) }
            }
        }
    }
    private var usesExportPicker: Bool {
        #if os(macOS)
        return dialogs?.export != nil
        #else
        return true
        #endif
    }
    /// Share destination and staging behavior for editable projects and PNGs.
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
                    let staging = try ProjectFileIO.stagingURL(title: name, extension: type == .png ? "png" : "capy")
                    do { try task.write(to: staging) }
                    catch { try? FileManager.default.removeItem(at: staging.deletingLastPathComponent()); throw error }
                    DispatchQueue.main.async {
                        guard let self else { Self.removeStaging(staging); return }
                        if self.cancelled { Self.removeStaging(staging); completion(nil); return }
                        let completed: (URL?) -> Void = { url in Self.removeStaging(staging); completion(url) }
                        if let export = self.dialogs?.export { export(staging, completed) }
                        else { self.pickerCompletion = completed; self.picker = Picker(export: staging) }
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
    private func open(_ url: URL?, extent: [UInt32]? = nil) {
        let recovery = recovering
        task(opening: true) { [weak self] task in
            NativeProjectTask.io.async {
                do {
                    try task.read(from: url, extent: extent)
                    DispatchQueue.main.async {
                        guard let self else { return }
                        if self.cancelled { self.finish(); return }
                        self.store?.native?.finishProject(task, opening: true, title: url?.lastPathComponent ?? "Untitled", url: url,
                            recovered: recovery != nil) { [weak self] error in
                            DispatchQueue.main.async {
                                guard let self else { return }
                                if let error { self.report(error) } else {
                                    self.destination = recovery == nil ? url : nil; self.store?.layerThumbnails.reset()
                                    if let recovery { self.store?.recovery.didRestore(recovery) }
                                }
                                self.recovering = nil
                                self.finish(error == nil)
                            }
                        }
                    }
                } catch {
                    let message = error.localizedDescription
                    DispatchQueue.main.async { self?.report(message); self?.finish() }
                }
            }
        }
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
        requestID = nil; busy = false; blocksEditor = false; finishing = false
        activeTask = nil; cancelling = false; exportPreparing = false
        if let state = store?.state.json {
            receive(state)
            if requestID == nil && closeCompletion != nil { finishClose(state["document_file"]["close_ready"].bool) }
            if requestID == nil { recovering = nil; externalURL = nil }
        }
    }
    private func chooseOpen(_ completion: @escaping (URL?) -> Void) {
        if let dialogs { dialogs.open(completion); return }
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.capyProject]; panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.begin { response in completion(response == .OK ? panel.url : nil) }
        #else
        pickerCompletion = completion; picker = Picker(export: nil)
        #endif
    }
    private func chooseSave(name: String, type: UTType, _ completion: @escaping (URL?) -> Void) {
        if let dialogs { dialogs.save(name, type, completion); return }
        #if os(macOS)
        let panel = NSSavePanel()
        panel.allowedContentTypes = [type]; panel.canCreateDirectories = true
        panel.nameFieldStringValue = name.contains(".") ? name : name + (type == .png ? ".png" : ".capy")
        panel.begin { response in completion(response == .OK ? panel.url : nil) }
        #else
        completion(nil) // iPad uses the export picker above.
        #endif
    }
    func picked(_ url: URL?) {
        let completion = pickerCompletion; pickerCompletion = nil; picker = nil
        completion?(url)
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
                    }.padding(10).background(.regularMaterial, in: Capsule()).padding(12)
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
                NewDrawingForm(spec: files.newDocumentSpec) { files.created($0) }
            }
            #if os(iOS)
            .sheet(item: $files.picker, onDismiss: { files.picked(nil) }) { picker in
                ProjectPicker(picker: picker) { files.picked($0) }.ignoresSafeArea()
            }
            #endif
    }
}
private extension ProjectFiles {
    var activeOperationVisible: Bool { !confirming && picker == nil && !creating }
}

#if os(iOS)
private struct ProjectPicker: UIViewControllerRepresentable {
    let picker: ProjectFiles.Picker
    let completion: (URL?) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(completion) }
    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let controller = picker.export.map { UIDocumentPickerViewController(forExporting: [$0], asCopy: false) }
            ?? UIDocumentPickerViewController(forOpeningContentTypes: [.capyProject], asCopy: false)
        controller.allowsMultipleSelection = false; controller.delegate = context.coordinator
        return controller
    }
    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let completion: (URL?) -> Void
        init(_ completion: @escaping (URL?) -> Void) { self.completion = completion }
        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) { completion(urls.first) }
        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { completion(nil) }
    }
}
#endif
