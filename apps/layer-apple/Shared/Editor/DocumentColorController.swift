import SwiftUI

/// Owns one shared document request. Heavy conversion and immutable previews
/// stay on the file worker; only atomic adoption enters the render owner.
@MainActor final class DocumentColorController: ObservableObject {
    @Published private(set) var busy = false
    @Published private(set) var publishing = false
    @Published private(set) var ready = false
    @Published private(set) var loaded = false
    @Published private(set) var color = JSON()
    @Published private(set) var sourceInfo = JSON()
    @Published private(set) var rows: [JSON] = []
    @Published private(set) var previews: [CGImage] = []
    @Published private(set) var clipped: UInt64 = 0
    @Published private(set) var copy = false
    @Published private(set) var sourceDescription = ""
    @Published private(set) var sourceProfile = ""
    @Published private(set) var sourceOriginal = JSON()
    @Published private(set) var sourceContext = JSON()
    @Published private(set) var rasterizeDetails = ""
    @Published var error: String?
    let preferences: ColorPreferencesStore
    let operation: String
    let history: Bool
    let properties: Bool
    let source: Bool
    private let redo: Bool
    @Published private(set) var catalog = JSON()
    @Published private(set) var bootstrap = JSON()
    @Published private(set) var propertiesView = JSON()
    private weak var store: EditorStore?
    private let requestID: UInt64
    private let expected: (UInt64, UInt64)?
    private let completion: (Bool) -> Void
    private var task: NativeProjectTask?
    private var pending: (JSON?, Bool)?
    private var scheduled: DispatchWorkItem?
    private var closing = false
    private var finished = false
    init(store: EditorStore, request: JSON, requestID: UInt64, expected: (UInt64, UInt64)?, completion: @escaping (Bool) -> Void) {
        self.preferences = store.colorPreferences
        self.store = store; self.requestID = requestID; self.expected = expected; self.completion = completion
        source = ["repair_source_profile", "rasterize_source"].contains(request["type"].string)
        operation = source ? request["type"].string : request["operation"].string
        history = request["type"].string == "color_history"
        properties = request["type"].string == "properties"
        redo = request["redo"].bool
        refreshCopy()
    }
    var text: JSON { catalog["document_color_copy"] }
    var dialog: JSON {
        if properties { return loaded ? propertiesView["dialog"] : bootstrap["progress_dialog"] }
        return text[history ? "progress_dialog" : "dialog"]
    }
    var title: String {
        if properties { return propertiesView["title"].isNull ? bootstrap["preparing_document"].string : propertiesView["title"].string }
        if history { return text[redo ? "redo_title" : "undo_title"].string }
        let titles = ["repair_source_profile": "repair_title", "rasterize_source": "rasterize_title", "assign": "assign_title", "depth": "depth_title"]
        return text[titles[operation] ?? "convert_title"].string
    }
    var propertiesDone: String { propertiesView["done"].isNull ? bootstrap["common"]["done"].string : propertiesView["done"].string }
    var suggestedDepth: String { DialogUI.resolve(["kind": "document_color_suggested_depth", "depth": color["depth"].raw]).string }
    func dither(_ depth: String, active: Bool) -> JSON {
        DialogUI.resolve(["kind": "document_color_dither", "depth": depth, "active": active])
    }
    func refreshCopy() {
        guard let store else { return }
        catalog = store.catalog; bootstrap = store.bootstrap
        guard loaded else { return }
        if operation == "rasterize_source" {
            store.query(["type": "rasterize_details"]) { [weak self] details in self?.rasterizeDetails = details.string }
        } else if operation == "repair_source_profile" && !sourceContext.isNull {
            store.query(["type": "source_profile_copy", "name": sourceContext["name"].raw, "assumed": sourceContext["assumed"].bool]) { [weak self] copy in
                self?.sourceDescription = copy["description"].string; self?.sourceProfile = copy["name"].string
            }
        }
    }
    private func describeSource() {
        guard operation == "repair_source_profile" else { refreshCopy(); return }
        store?.query(["type": "source_dialog_profile", "id": requestID]) { [weak self] profile in self?.sourceOriginal = profile }
        store?.query(["type": "source_dialog_context", "id": requestID]) { [weak self] context in
            self?.sourceContext = context; self?.refreshCopy()
        }
    }
    func load() {
        if properties || source {
            capture(source ? .source : .properties) { [weak self] task in
                NativeProjectTask.io.async {
                    do {
                        let details = try task.details()
                        DispatchQueue.main.async {
                            guard let self else { return }
                            if self.source { self.sourceInfo = details; self.color = details["color"] }
                            else { self.propertiesView = details; self.rows = details["rows"].array }
                            self.loaded = true; self.busy = false; self.task = nil
                            if self.closing { self.finish(false) } else if self.source { self.describeSource() }
                        }
                    } catch { let message = error.localizedDescription
                        DispatchQueue.main.async { self?.failed(message) }
                    }
                }
            }
        } else {
            store?.query(["type": "document_color"]) { [weak self] value in
                guard let self, !finished else { return }
                guard !value["space"].string.isEmpty else { failed(bootstrap["action_failed"].string); return }
                color = value; loaded = true
                if history { prepare(nil) }
            }
        }
    }
    private func capture(_ kind: NativeProjectTask.Kind, work: @escaping (NativeProjectTask) -> Void) {
        guard !busy, !closing, !finished else { return }
        guard let native = store?.native else { failed(catalog["native_copy"]["color"]["canvas_unavailable"].string); return }
        busy = true; ready = false; error = nil
        native.projectTask(kind: kind, expected: expected) { [weak self] task, error in
            DispatchQueue.main.async {
                guard let self else { task?.cancel(); return }
                if self.closing { task?.cancel(); self.busy = false; self.finish(false); return }
                guard let task else { self.failed(error ?? self.bootstrap["action_failed"].string); return }
                self.task = task; work(task)
            }
        }
    }
    func schedule(_ choice: JSON?, copy: Bool = false) {
        guard !history, !properties, loaded, !closing, !finished, !publishing else { return }
        ready = false
        pending = (choice, copy)
        scheduled?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.preparePending() }
        scheduled = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1, execute: work)
    }
    private func preparePending() {
        guard !busy, !closing, !finished, let next = pending else { return }
        pending = nil; scheduled = nil
        prepare(next.0, copy: next.1)
    }
    func showAppearance() {
        guard properties, !busy, !finished else { return }
        let store = self.store
        finish(false)
        store?.invoke("soft_proof_setup")
    }
    func invalidate() {
        guard !busy else { return }
        ready = false; previews = []; clipped = 0; task = nil; error = nil
    }
    func prepare(_ choice: JSON?, copy: Bool = false) {
        guard !busy, !closing, !finished else { return }
        invalidate(); self.copy = copy
        capture(source ? .source : .color) { [weak self] task in
            guard let self, let native = store?.native else { task.cancel(); return }
            let history = self.history
            native.prepareEdit(task, choice: choice, copy: copy) { [weak self] error in
              NativeProjectTask.io.async {
                do {
                    if let error { throw HostFailure(message: error) }
                    let details = try task.details()
                    let images = history ? [] : try [task.comparison(after: false), task.comparison(after: true)]
                    DispatchQueue.main.async {
                        guard let self else { return }
                        self.busy = false
                        if self.closing { self.finish(false); return }
                        if self.pending != nil { self.task = nil; self.preparePending(); return }
                        if self.source { self.sourceInfo = details }
                        self.clipped = details["clipped_channels"].uint; self.previews = images; self.ready = true
                        if self.history { self.apply() }
                    }
                } catch { let message = error.localizedDescription
                    DispatchQueue.main.async { self?.failed(message) }
                }
              }
            }
        }
    }
    func apply() {
        guard !busy, ready, !closing, !finished, let task, let native = store?.native else { return }
        busy = true; publishing = true
        let publish = { [weak self] in
            guard let self else { return }
            native.finishProject(task, opening: true, title: self.title, url: nil) { [weak self] error in
                DispatchQueue.main.async {
                    guard let self else { return }
                    self.publishing = false; self.busy = false
                    if let error { self.failed(error); return }
                    if !self.source { self.store?.layerThumbnails.reset(); self.store?.filterPreviews.reset() }
                    self.finish(true)
                }
            }
        }
        if copy {
            store?.recovery.flush { [weak self] saved in
                guard let self else { return }
                if saved { publish() }
                else { self.publishing = false; self.failed(self.store?.recovery.failure ?? self.bootstrap["action_failed"].string) }
            }
        } else { publish() }
    }
    func cancel() {
        guard !finished, !publishing else { return }
        closing = true; pending = nil; scheduled?.cancel(); scheduled = nil; task?.cancel()
        if !busy { finish(false) }
    }
    private func failed(_ message: String) {
        busy = false; ready = false; task = nil
        if closing { finish(false) } else if pending != nil { preparePending() } else { error = message }
    }
    private func finish(_ success: Bool) {
        guard !finished else { return }
        finished = true; task = nil; completion(success)
    }
}
