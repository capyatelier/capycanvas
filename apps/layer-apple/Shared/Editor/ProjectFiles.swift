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
    private var closeCompletion: ((Bool) -> Void)?
    /// Dialog dependency keeps editor/file effects testable without driving
    /// platform panels. The app uses the native implementation by default.
    struct Dialogs {
        var open: (@escaping (URL?) -> Void) -> Void
        var save: (String, @escaping (URL?) -> Void) -> Void
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
        blocksEditor = action == "open" || action == "new" || action == "confirm_close" || closeCompletion != nil
        switch action {
        case "save":
            destination = URL(string: document["location"]["uri"].string)
            save(as: document["location"].isNull) { [weak self] saved in self?.finish(saved) }
        case "new": open(nil)
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
                if choice == "cancel" { self.externalURL = nil }
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
    private func save(as copy: Bool, completion: @escaping (Bool) -> Void) {
        if !copy, let destination { write(destination, completion: completion); return }
        #if os(macOS)
        let exportPicker = dialogs?.export != nil
        #else
        let exportPicker = true
        #endif
        if !exportPicker {
            chooseSave { [weak self] url in
                guard let self else { return }
                guard let url else { completion(false); return }
                write(url, completion: completion)
            }
        } else {
            task(opening: false) { [weak self] task in
                guard let self else { return }
                let title = self.title
                NativeProjectTask.io.async { [weak self] in
                    do {
                        let staging = try ProjectFileIO.stagingURL(title: title)
                        do { try task.write(to: staging) }
                        catch { try? FileManager.default.removeItem(at: staging.deletingLastPathComponent()); throw error }
                        DispatchQueue.main.async {
                            guard let self else { Self.removeStaging(staging); return }
                            if self.cancelled { Self.removeStaging(staging); completion(false); return }
                            let completed: (URL?) -> Void = { [weak self] url in
                                Self.removeStaging(staging)
                                guard let self, let url else { completion(false); return }
                                self.saved(task, at: url, completion: completion)
                            }
                            if let export = self.dialogs?.export { export(staging, completed) }
                            else { self.pickerCompletion = completed; self.picker = Picker(export: staging) }
                        }
                    } catch {
                        let message = error.localizedDescription
                        DispatchQueue.main.async { self?.report(message); completion(false) }
                    }
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
    private func open(_ url: URL?) {
        task(opening: true) { [weak self] task in
            NativeProjectTask.io.async {
                do {
                    try task.read(from: url)
                    DispatchQueue.main.async {
                        guard let self else { return }
                        if self.cancelled { self.finish(); return }
                        self.store?.native?.finishProject(task, opening: true, title: url?.lastPathComponent ?? "Untitled", url: url) { [weak self] error in
                            DispatchQueue.main.async {
                                guard let self else { return }
                                if let error { self.report(error) } else { self.destination = url; self.store?.layerThumbnails.reset() }
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
        finishing = true; externalURL = nil
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
        activeTask = nil; cancelling = false
        if let state = store?.state {
            receive(state)
            if requestID == nil && closeCompletion != nil { finishClose(state["document_file"]["close_ready"].bool) }
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
    private func chooseSave(_ completion: @escaping (URL?) -> Void) {
        if let dialogs { dialogs.save(title, completion); return }
        #if os(macOS)
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.capyProject]; panel.canCreateDirectories = true
        panel.nameFieldStringValue = title == "Untitled" ? "Untitled.capy" : title
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
            #if os(iOS)
            .sheet(item: $files.picker, onDismiss: { files.picked(nil) }) { picker in
                ProjectPicker(picker: picker) { files.picked($0) }.ignoresSafeArea()
            }
            #endif
    }
}
private extension ProjectFiles {
    var activeOperationVisible: Bool { !confirming && picker == nil }
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
