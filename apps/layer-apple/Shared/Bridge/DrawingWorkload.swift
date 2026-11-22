import Foundation
import Combine

extension EditorStore {
    var workloadReady: Bool {
        canvasSubmitted && snapshot["shaders_ready"].bool && (workspaces == nil || workspaces?.ready == true)
    }
    func workloadEdit(_ value: [String: Any]) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            edit(value) { error in
                if let error { continuation.resume(throwing: HostFailure(message: error)) }
                else { continuation.resume() }
            }
        }
    }
    func workloadCheckpoint() async throws {
        let saved = await withCheckedContinuation { continuation in
            flushPersistence { continuation.resume(returning: $0) }
        }
        guard saved else { throw HostFailure(message: "The workload checkpoint did not complete") }
    }
    func applyWorkloadAppearance(theme: String?) async throws {
        if let theme { try await workloadEdit(["type": "set_theme", "theme": theme]) }
        if let level = ProcessInfo.processInfo.environment["CAPY_WORKLOAD_TRANSPARENCY"] {
            guard let index = ["off", "low", "medium", "high"].firstIndex(of: level) else {
                throw HostFailure(message: "CAPY_WORKLOAD_TRANSPARENCY must be off, low, medium or high")
            }
            try await workloadEdit(["type": "preferences", "action": ["type": "edit", "id": "transparency", "value": index]])
        }
        if let theme, state["theme"].string != theme { throw HostFailure(message: "The workload theme was not applied") }
    }
    func workloadProject(_ kind: NativeProjectTask.Kind, url: URL, title: String) async throws {
        guard let native else { throw CancellationError() }
        let job: NativeProjectTask = try await withCheckedThrowingContinuation { continuation in
            native.projectTask(kind: kind) { task, error in
                if let task { continuation.resume(returning: task) }
                else { continuation.resume(throwing: HostFailure(message: error ?? "The document task is unavailable")) }
            }
        }
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            NativeProjectTask.io.async {
                do { if kind == .open { try job.read(from: url) } else { try job.write(to: url) }; continuation.resume() }
                catch { continuation.resume(throwing: error) }
            }
        }
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            native.finishProject(job, opening: kind == .open, title: title, url: kind == .open ? nil : url) { error in
                if let error { continuation.resume(throwing: HostFailure(message: error)) }
                else { continuation.resume() }
            }
        }
    }
    static func workloadSample(x: Double, y: Double, pressure: Double, time: UInt64, phase: Double) -> [Double] {
        [x, y, pressure, 0.2, 0.1, 0.3, 0, Double(time), phase]
    }
    func workloadPointer(_ contact: UInt64, tool: UInt32 = 0, records: [Double], predicted: Bool = false) {
        native?.pointer(id: contact, tool: tool, button: 0, records: records, predicted: predicted, revision: cameraRevision)
    }
}

/// Opt-in benchmark in an isolated editor. Uses the ordinary input owner,
/// display link, renderer, panels, history and recovery writer on both hosts.
@MainActor final class DrawingWorkload {
    private weak var store: EditorStore?
    private let plan: DrawingWorkloadPlan
    private var timer: Timer?
    private var preparing = false
    private var restoringPrime = false
    private var baselineLayers = ""
    private var started: UInt64 = 0
    private var measurementStarted: UInt64 = 0
    private var finished = false
    private var nextSample = 0
    private var lastContact: UInt64?
    private var lastRecord: [Double] = []
    private var deliveredSamples: UInt64 = 0
    private var deliveredBatches: UInt64 = 0
    private var lastReport: UInt64 = 0
    private var maximumLateness: UInt64 = 0
    private let created = FrameTrace.now()

    init(store: EditorStore, plan: DrawingWorkloadPlan) {
        self.store = store; self.plan = plan
        record(0, d: UInt64(plan.documentExtent[0]), e: UInt64(plan.documentExtent[1]), f: UInt64(plan.paintLayers),
            g: UInt64(plan.brush), h: UInt64(plan.diameter * 1000), i: plan.predicts ? 1 : 0)
        let timer = Timer(timeInterval: 1.0 / 120, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        self.timer = timer
        RunLoop.main.add(timer, forMode: .common)
    }
    deinit { timer?.invalidate() }
    private func record(_ phase: UInt64, d: UInt64 = 0, e: UInt64 = 0, f: UInt64 = 0,
        g: UInt64 = 0, h: UInt64 = 0, i: UInt64 = 0) {
        if phase != 6 { NSLog("Capy workload %@ phase %llu", plan.name, phase) }
        store?.native?.observeWorkload(FrameTraceEvent(kind: .workload, a: FrameTrace.now(), b: phase,
            c: plan.identifier, d: d, e: e, f: f, g: g, h: h, i: i))
    }
    private func action(_ value: [String: Any]) async throws {
        guard let store else { throw CancellationError() }
        try await store.workloadEdit(value)
    }
    private func checkpoint() async throws {
        guard let store else { throw CancellationError() }
        try await store.workloadCheckpoint()
    }
    private var layerRevisions: String {
        JSON(store?.state["layers"].array.map {
            [$0["id"].raw, $0["paint_revision"].raw, $0["mask_revision"].raw]
        } ?? []).stableKey
    }
    private func restorePrime() async throws {
        try await checkpoint()
        guard layerRevisions != baselineLayers else {
            throw HostFailure(message: "The priming stroke did not change the paint layer")
        }
        try await action(["type": "invoke", "command": "undo"])
        try await checkpoint()
        guard layerRevisions == baselineLayers else {
            throw HostFailure(message: "Undo did not restore the workload baseline")
        }
        nextSample = 0; maximumLateness = 0; lastReport = 0
        started = FrameTrace.now(); measurementStarted = started
        record(8, d: 1)
        record(2, d: deliveredSamples, e: deliveredBatches)
        restoringPrime = false
    }
    private func prepare() async throws {
        guard let store else { throw CancellationError() }
        let extent = plan.documentExtent
        let source = plan.document.map { URL(fileURLWithPath: $0) }
        if source == nil {
            let files = store.projectFiles
            let epoch = store.state["document_file"]["epoch"].uint
            try await action(["type": "invoke", "command": "new_document"])
            let steps: [[String: Any]] = [
                ["op": "open", "screen": NSNull(), "untitled": 1],
                ["op": "unit", "unit": "px"], ["op": "linked", "linked": false],
                ["op": "width", "value": extent[0]], ["op": "height", "value": extent[1]],
                ["op": "space", "space": plan.color["space"] ?? "Srgb"],
                ["op": "depth", "depth": plan.color["depth"] ?? "U8"],
                ["op": "paper"], ["op": "pick_paper", "id": "drawing_paper"], ["op": "paper_relief", "value": 0], ["op": "use_paper"],
                ["op": "paper_color"], ["op": "paper_color_value", "color": ["space": "Srgb", "rgba": [1, 1, 1, 1]]],
                ["op": "transparent", "transparent": false], ["op": "use_paper_color"], ["op": "create"]]
            for step in steps {
                try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                    files.newDrawing(step) { error in
                        if let error { continuation.resume(throwing: HostFailure(message: error)) }
                        else { continuation.resume() }
                    }
                }
            }
            for await busy in files.$busy.values { if !busy { break } }
            guard files.error == nil, !files.creating, !files.blocksEditor,
                store.state["document_file"]["epoch"].uint != epoch else {
                throw HostFailure(message: files.error ?? "Workload document preparation failed")
            }
        } else if let source {
            try await store.workloadProject(.open, url: source, title: "Performance workload")
        }
        // Seven translucent underpaint layers plus the active paint layer in
        // the 4K cases. These remain visible and participate in composition.
        for layer in 0..<(source == nil ? plan.paintLayers - 1 : 0) {
            try await action(["type": "set_color", "rgba": [Double(layer % 3) * 0.3 + 0.1, 0.25, 0.55, 0.2]])
            try await action(["type": "invoke", "command": "select_all"])
            try await action(["type": "invoke", "command": "fill_selection"])
            try await action(["type": "invoke", "command": "deselect"])
            try await action(["type": "invoke", "command": "add_layer"])
        }
        try await action(["type": "select_brush", "id": plan.brush])
        try await action(["type": "set_brush_size", "value": plan.diameter])
        try await action(["type": "set_color", "rgba": [0.08, 0.2, 0.55, 1.0]])
        try await action(["type": "invoke", "command": "fit_canvas"])
        try await store.applyWorkloadAppearance(theme: plan.theme)
        guard store.state["layers"].array.count == plan.paintLayers + 1 else {
            throw HostFailure(message: "The workload layer count does not match its specification")
        }
        if source != nil {
            let tab = store.state["tabs"].array.first { $0["active"].bool }
            let paint = store.state["layers"].array.first
            guard tab?["width"].uint == UInt64(extent[0]), tab?["height"].uint == UInt64(extent[1]),
                paint?["selected"].bool == true, paint?["editing"].bool == true,
                paint?["drawing"].bool == true, paint?["object"].bool == false,
                store.state["brush"]["preset"].uint == UInt64(plan.brush),
                Float(store.state["brush"]["diameter"].number) == Float(plan.diameter) else {
                throw HostFailure(message: "The reference photo, empty paint target or brush does not match the workload")
            }
            try await checkpoint()
            baselineLayers = layerRevisions
        }
        started = FrameTrace.now()
        record(1)
        if source != nil { record(7) }
    }
    private func tick() {
        guard !finished, !restoringPrime else { return }
        guard let store, store.failure == nil else { finish(failed: true); return }
        let now = FrameTrace.now()
        if started == 0 {
            if now - created > 120_000_000_000 { finish(failed: true); return }
            if !preparing && store.workloadReady {
                preparing = true
                Task { [weak self] in
                    do { try await self?.prepare() }
                    catch { self?.store?.failure = error.localizedDescription; self?.finish(failed: true) }
                }
            }
            return
        }
        let elapsed = Double(now - started) / 1e9
        if plan.document == nil && measurementStarted == 0 && elapsed >= DrawingWorkloadPlan.warmupSeconds {
            measurementStarted = now; record(2, d: deliveredSamples, e: deliveredBatches)
        }
        if plan.document == nil && measurementStarted != 0 && Double(now - measurementStarted) >= plan.seconds * 1e9 {
            finish(failed: false); return
        }
        var due = Int(floor(elapsed * Double(DrawingWorkloadPlan.samplesPerSecond)))
        // Bound a delayed producer's catch-up allocation. Abort and report an
        // invalid run instead of dropping a second of input to make it faster.
        guard due - nextSample < DrawingWorkloadPlan.samplesPerSecond else { finish(failed: true); return }
        if plan.document != nil {
            if measurementStarted != 0 && elapsed >= plan.seconds && nextSample == Int(plan.seconds) * DrawingWorkloadPlan.samplesPerSecond {
                finish(failed: false); return
            }
            due = min(due, measurementStarted == 0 ? plan.contactSamples - 1 : Int(plan.seconds) * DrawingWorkloadPlan.samplesPerSecond - 1)
        }
        guard due >= nextSample else { return }
        let camera = store.state["camera"]
        let zoom = camera["zoom"].number
        let translation = camera["translation"].array.map(\.number)
        guard translation.count == 2, zoom > 0 else { finish(failed: true); return }
        let previousBatches = deliveredBatches
        var batch: [Double] = [], contact: UInt64?
        func deliver() {
            guard let contact, !batch.isEmpty else { return }
            store.workloadPointer(contact, records: batch)
            deliveredBatches += 1; deliveredSamples += UInt64(batch.count / 9)
            if plan.document != nil && measurementStarted != 0 {
                if batch[8] == 1 { record(9, d: contact, e: FrameTrace.timestamp(batch[7])) }
                if batch.last == 3 { record(10, d: contact, e: FrameTrace.timestamp(batch[batch.count - 2])) }
            }
            lastRecord = Array(batch.suffix(9))
            lastContact = lastRecord[8] == 3 ? nil : contact
            batch.removeAll(keepingCapacity: true)
        }
        while nextSample <= due {
            let index = nextSample; nextSample += 1
            guard let sample = plan.sample(index) else { deliver(); contact = nil; continue }
            if contact != sample.contact { deliver(); contact = sample.contact }
            let timestamp = started + UInt64(Double(index) / Double(DrawingWorkloadPlan.samplesPerSecond) * 1e9)
            maximumLateness = max(maximumLateness, now - min(now, timestamp))
            batch += EditorStore.workloadSample(x: translation[0] + sample.x * zoom, y: translation[1] + sample.y * zoom,
                pressure: sample.pressure, time: timestamp, phase: sample.phase)
        }
        deliver()
        if plan.predicts, let contact = lastContact {
            var predicted: [Double] = []
            for index in nextSample..<(nextSample + 2) {
                guard let sample = plan.sample(index), sample.contact == contact, sample.phase == 2 else { break }
                let time = started + UInt64(Double(index) / Double(DrawingWorkloadPlan.samplesPerSecond) * 1e9)
                predicted += EditorStore.workloadSample(x: translation[0] + sample.x * zoom, y: translation[1] + sample.y * zoom,
                    pressure: sample.pressure, time: time, phase: 2)
            }
            if !predicted.isEmpty { store.workloadPointer(contact, records: predicted, predicted: true) }
        }
        if deliveredBatches != previousBatches { store.wake?() }
        if now - lastReport >= 1_000_000_000 {
            record(6, d: deliveredSamples, e: deliveredBatches, f: maximumLateness)
            lastReport = now; maximumLateness = 0
        }
        if plan.document != nil {
            if measurementStarted == 0 && nextSample == plan.contactSamples {
                restoringPrime = true
                Task { [weak self] in
                    do { try await self?.restorePrime() }
                    catch { self?.store?.failure = error.localizedDescription; self?.finish(failed: true) }
                }
            } else if measurementStarted != 0 && elapsed >= plan.seconds {
                finish(failed: false)
            }
        }
    }
    private func finish(failed: Bool) {
        guard !finished else { return }
        finished = true; timer?.invalidate(); timer = nil
        if let contact = lastContact, lastRecord.count == 9, let store {
            lastRecord[7] = Double(FrameTrace.now()); lastRecord[8] = 3
            store.workloadPointer(contact, records: lastRecord)
            deliveredSamples += 1; deliveredBatches += 1
            store.wake?()
        }
        record(failed ? 5 : 3, d: deliveredSamples, e: deliveredBatches, f: maximumLateness)
        if failed && store?.failure == nil { store?.failure = "The performance workload did not complete; its trace is not a valid workload result" }
        // Observe a fixed postlude for pen-up, GPU work, idle and recovery. Its
        // end marker is not proof that the renderer has drained all its work.
        DispatchQueue.main.asyncAfter(deadline: .now() + 10) { [weak self] in
            guard let self else { return }
            if self.store?.failure != nil || self.store?.storageFailure != nil || self.store?.recovery.error != nil {
                self.record(5)
            }
            self.record(4); self.store?.native?.finishTrace()
        }
    }
}
