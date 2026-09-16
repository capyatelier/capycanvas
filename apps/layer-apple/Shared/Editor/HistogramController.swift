import SwiftUI

/// Inspection never mutates the drawing. Revision changes cancel immutable work;
/// a short debounce keeps automatic updates out of the continuous stroke loop.
@MainActor final class HistogramController: ObservableObject {
    @Published private(set) var isOpen = false
    @Published private(set) var busy = false
    @Published private(set) var result = JSON()
    @Published private(set) var status = "Preparing inspection…"
    @Published var automatic = true { didSet { schedule() } }
    private weak var store: EditorStore?
    private var epoch: UInt64 = 0
    private var revision: UInt64 = 0
    private var gpuReady = false
    private var lastRequest: UInt64 = 0
    private var generation: UInt64 = 0
    private var attempted: String?
    private var pending: Task<Void, Never>?
    private var task: NativeProjectTask?
    private var key: String { "\(epoch):\(revision)" }
    var stale: Bool { !result.isNull && (result["epoch"].uint != epoch || result["revision"].uint != revision) }
    init(store: EditorStore) { self.store = store }

    func receive(_ state: JSON, gpuReady: Bool) {
        let file = state["document_file"]
        if epoch != file["epoch"].uint || revision != file["revision"].uint || self.gpuReady != gpuReady {
            cancelWork()
            if epoch != file["epoch"].uint { result = JSON() }
            epoch = file["epoch"].uint; revision = file["revision"].uint; self.gpuReady = gpuReady
            attempted = nil
            if isOpen { status = gpuReady ? "Refresh to inspect" : "Canvas unavailable" }
            schedule()
        }
        if let request = state["requests"].array.first(where: { $0["kind"]["type"].string == "histogram" }),
            request["id"].uint > lastRequest {
            lastRequest = request["id"].uint
            isOpen = true
            store?.dispatch(["type": "complete_request", "id": lastRequest, "error": NSNull()])
            refresh()
        }
    }
    func close() {
        isOpen = false; cancelWork(); result = JSON(); attempted = nil
        store?.focusCanvas?()
    }
    private func cancelWork() {
        generation &+= 1
        pending?.cancel(); pending = nil
        task?.cancel(); task = nil; busy = false
    }
    private func schedule() {
        pending?.cancel(); pending = nil
        guard isOpen, automatic, gpuReady, !busy, attempted != key else { return }
        pending = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(300)) } catch { return }
            self?.refresh()
        }
    }
    func refresh() {
        guard isOpen, !busy, gpuReady, let native = store?.native else { return }
        pending?.cancel(); pending = nil
        generation &+= 1
        let token = generation
        attempted = key; busy = true; status = "Updating · complete composite at full resolution"
        native.projectTask(kind: .histogram, expected: (epoch, revision)) { [weak self] task, error in
            DispatchQueue.main.async {
                guard let self, self.isOpen, self.generation == token else { task?.cancel(); return }
                guard let task else { self.finish(token, result: nil, error: error ?? "Could not inspect the drawing"); return }
                self.task = task
                NativeProjectTask.io.async { [weak self] in
                    do {
                        let result = try task.details()
                        DispatchQueue.main.async { self?.finish(token, result: result, error: nil) }
                    } catch { let message = error.localizedDescription
                        DispatchQueue.main.async { self?.finish(token, result: nil, error: message) }
                    }
                }
            }
        }
    }
    private func finish(_ token: UInt64, result: JSON?, error: String?) {
        guard isOpen, generation == token else { return }
        busy = false; task = nil
        if let result {
            self.result = result
            status = result["sampled_time"].isNull ? "Current committed drawing"
                : String(format: "Animated effects · snapshot at %.2f s", result["sampled_time"].number)
        } else { status = error ?? "Could not inspect the drawing" }
    }
}
