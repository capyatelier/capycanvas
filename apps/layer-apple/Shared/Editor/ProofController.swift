import SwiftUI

/// One cancellable CPU preparation per window. Shared Rust owns recipe identity,
/// stale-result checks and viewing state; the file lane owns durable ICC copies.
@MainActor final class ProofController: ObservableObject {
    @Published private(set) var setupID: UInt64?
    @Published private(set) var form = JSON()
    @Published private(set) var formRevision: UInt64 = 0
    @Published private(set) var status = ""
    @Published private(set) var busy = false
    @Published private(set) var committing = false
    @Published var error: String?
    private weak var store: EditorStore?
    private let worker = DispatchQueue(label: "art.capycanvas.proof", qos: .userInitiated)
    private var task: NativeProjectTask?
    private var serial: UInt64 = 0
    private var pendingRecipe: JSON?
    private var documentKey = ""
    private var lastSDRRequest: UInt64 = 0
    private var generation: UInt64?
    private var gpuReady = false
    private var paused = false
    private var observing = false
    private var observeAgain = false
    init(store: EditorStore) { self.store = store }
    var preferences: ColorPreferencesStore { store?.colorPreferences ?? ColorPreferencesStore(root: nil) }

    func receive(_ state: JSON, gpuReady: Bool) {
        self.gpuReady = gpuReady
        let key = String(state["document_file"]["epoch"].uint)
        if key != documentKey {
            pendingRecipe = nil; cancelWork(); generation = nil
            documentKey = key
            if !form.isNull { loadForm() }
        }
        if let request = state["requests"].array.first(where: { $0["kind"]["type"].string == "sdr_rendition" }), request["id"].uint != lastSDRRequest {
            lastSDRRequest = request["id"].uint
            action(["type": "mode", "mode": "sdr"]); reveal()
            store?.dispatch(["type": "complete_request", "id": lastSDRRequest, "error": NSNull()])
        }
        let next = state["requests"].array.first { $0["kind"]["type"].string == "soft_proof_setup" }?["id"].uint
        if next != setupID {
            cancelWork(); setupID = next; error = nil
            if let next {
                busy = pendingRecipe != nil
                reveal()
                if store?.snapshot["proof_panel"]["mode"].string == "off" { action(["type": "mode", "mode": "print"]) }
                store?.query(["type": "proof_form"]) { [weak self] value in
                    guard let self, setupID == next else { return }
                    acceptForm(value)
                    if let recipe = pendingRecipe { pendingRecipe = nil; busy = false; apply(recipe) }
                    else { store?.dispatch(["type": "complete_request", "id": next, "error": NSNull()]) }
                }
            }
        }
        if setupID != nil || state["soft_proof"].bool || state["gamut_warning"].bool || !status.isEmpty || busy { sync() }
    }
    func action(_ action: [String: Any]) {
        store?.query(["type": "proof_panel", "action": action]) { [weak self] _ in self?.store?.wake?() }
    }
    func reveal() { action(["type": "reveal"]) }
    func loadForm() {
        let key = documentKey
        store?.query(["type": "proof_form"]) { [weak self] value in
            guard let self, documentKey == key else { return }
            acceptForm(value)
        }
    }
    private func acceptForm(_ value: JSON) { form = value; formRevision &+= 1 }
    func applyLive(_ recipe: JSON) {
        guard !committing else { return }
        if setupID != nil { apply(recipe) }
        else { pendingRecipe = recipe; busy = true; store?.invoke("soft_proof_setup") }
    }
    func setPaused(_ value: Bool) {
        paused = value
        if value { cancelWork() } else { sync() }
    }
    private func cancelWork() {
        guard !committing else { return }
        serial &+= 1; task?.cancel(); task = nil; busy = false
    }
    func close() {
        pendingRecipe = nil
        guard !committing, let id = setupID else { return }
        cancelWork(); setupID = nil; error = nil
        store?.dispatch(["type": "complete_request", "id": id, "error": NSNull()])
        store?.focusCanvas?()
    }
    private func sync() {
        guard !paused, let store else { return }
        if observing { observeAgain = true; return }
        observing = true
        store.query(["type": "proof_status"]) { [weak self] value in
            guard let self else { return }
            observing = false
            guard !paused, !value.isNull else { return }
            status = value["text"].string
            if setupID == nil && !committing {
                if generation != value["generation"].uint && !form.isNull { loadForm() }
                if generation != value["generation"].uint || !value["needed"].bool || !gpuReady { cancelWork() }
                generation = value["generation"].uint
                if value["needed"].bool && gpuReady && !busy { start(id: 0, recipe: nil) }
            }
            if observeAgain { observeAgain = false; sync() }
        }
    }
    func apply(_ recipe: JSON) {
        if setupID == nil { applyLive(recipe); return }
        guard !busy, !committing, let id = setupID else { return }
        cancelWork(); start(id: id, recipe: recipe)
    }
    private func start(id: UInt64, recipe: JSON?) {
        guard !paused, let native = store?.native else { return }
        let token = serial
        busy = true; error = nil
        native.proofTask(id: id, recipe: recipe) { [weak self] task, failure in
            DispatchQueue.main.async {
                guard let self, self.serial == token, !self.paused else { task?.cancel(); return }
                guard let task else { self.finished(token, error: failure ?? "Could not prepare proof"); return }
                self.task = task
                self.worker.async { [weak self] in
                    let result = Result { try task.buildProof() }
                    DispatchQueue.main.async {
                        guard let self, self.serial == token, !self.paused else { return }
                        switch result {
                        case .success: self.validate(task, native: native, token: token, id: id)
                        case .failure(let failure):
                            let message = failure.localizedDescription
                            if id == 0 {
                                native.finishProof(task, failure: message) { [weak self] _ in
                                    DispatchQueue.main.async { self?.finished(token, error: message) }
                                }
                            } else { self.finished(token, error: message) }
                        }
                    }
                }
            }
        }
    }
    private func validate(_ task: NativeProjectTask, native: NativeOwner, token: UInt64, id: UInt64) {
        native.checkProof(task) { [weak self] failure in
            DispatchQueue.main.async {
                guard let self, self.serial == token, !self.paused else { return }
                if let failure { self.finished(token, error: failure); return }
                // Dismissal cannot promise cancellation after durable preservation
                // begins. Adoption still repeats shared document/recipe validation.
                self.committing = true
                let preferences = self.preferences
                NativeProjectTask.io.async { [weak self] in
                    do {
                        let previous = try task.proofPreservation()
                        if let previous { _ = try preferences.importProfile(previous, requireSaved: true) }
                        native.finishProof(task, preserved: previous != nil) { [weak self] failure in
                            DispatchQueue.main.async {
                                guard let self, self.serial == token else { return }
                                if failure == nil && id != 0 { self.setupID = nil }
                                self.finished(token, error: failure)
                                if failure == nil { self.loadForm() }
                                self.store?.wake?()
                            }
                        }
                    } catch { let message = error.localizedDescription
                        DispatchQueue.main.async { self?.finished(token, error: message) }
                    }
                }
            }
        }
    }
    private func finished(_ token: UInt64, error: String?) {
        guard serial == token else { return }
        busy = false; committing = false; task = nil; self.error = error
        sync()
    }
}
