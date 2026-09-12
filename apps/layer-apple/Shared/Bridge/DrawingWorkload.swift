import Foundation

/// Opt-in benchmark in an isolated editor. Uses the ordinary input owner,
/// display link, renderer, panels, history and recovery writer on both hosts.
@MainActor final class DrawingWorkload {
    private weak var store: EditorStore?
    private let plan: DrawingWorkloadPlan
    private var timer: Timer?
    private var preparing = false
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
        record(0, d: UInt64(plan.extent), e: UInt64(plan.extent), f: UInt64(plan.paintLayers),
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
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            store.edit(value) { error in
                if let error { continuation.resume(throwing: HostFailure(message: error)) }
                else { continuation.resume() }
            }
        }
    }
    private func prepare() async throws {
        guard let native = store?.native else { throw CancellationError() }
        let job: NativeProjectTask = try await withCheckedThrowingContinuation { continuation in
            native.projectTask(opening: true) { task, error in
                if let task { continuation.resume(returning: task) }
                else { continuation.resume(throwing: HostFailure(message: error ?? "Workload document preparation failed")) }
            }
        }
        let extent = plan.extent
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            NativeProjectTask.io.async {
                do { try job.read(from: nil, extent: [extent, extent]); continuation.resume() }
                catch { continuation.resume(throwing: error) }
            }
        }
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            native.finishProject(job, opening: true, title: "Performance workload", url: nil) { error in
                if let error { continuation.resume(throwing: HostFailure(message: error)) }
                else { continuation.resume() }
            }
        }
        // Seven translucent underpaint layers plus the active paint layer in
        // the 4K cases. These remain visible and participate in composition.
        for layer in 0..<(plan.paintLayers - 1) {
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
        guard let store, store.state["layers"].array.count == plan.paintLayers + 1 else {
            throw HostFailure(message: "The workload layer count does not match its specification")
        }
        started = FrameTrace.now()
        record(1)
    }
    private func tick() {
        guard !finished else { return }
        guard let store, store.failure == nil else { finish(failed: true); return }
        let now = FrameTrace.now()
        if started == 0 {
            if now - created > 120_000_000_000 { finish(failed: true); return }
            if !preparing && store.canvasSubmitted && store.snapshot["shaders_ready"].bool
                && (store.workspaceLibrary == nil || store.workspaceLibrary?.ready == true) {
                preparing = true
                Task { [weak self] in
                    do { try await self?.prepare() }
                    catch { self?.store?.failure = error.localizedDescription; self?.finish(failed: true) }
                }
            }
            return
        }
        let elapsed = Double(now - started) / 1e9
        if measurementStarted == 0 && elapsed >= DrawingWorkloadPlan.warmupSeconds {
            measurementStarted = now; record(2, d: deliveredSamples, e: deliveredBatches)
        }
        if measurementStarted != 0 && Double(now - measurementStarted) >= plan.seconds * 1e9 {
            finish(failed: false); return
        }
        let due = Int(floor(elapsed * Double(DrawingWorkloadPlan.samplesPerSecond)))
        // Bound a delayed producer's catch-up allocation. Abort and report an
        // invalid run instead of dropping a second of input to make it faster.
        guard due - nextSample < DrawingWorkloadPlan.samplesPerSecond else { finish(failed: true); return }
        guard due >= nextSample else { return }
        let camera = store.state["camera"], revision = store.cameraRevision
        let zoom = camera["zoom"].number
        let translation = camera["translation"].array.map(\.number)
        guard translation.count == 2, zoom > 0 else { finish(failed: true); return }
        let previousBatches = deliveredBatches
        var batch: [Double] = [], contact: UInt64?
        func deliver() {
            guard let contact, !batch.isEmpty else { return }
            store.native?.pointer(id: contact, tool: 0, button: 0, records: batch, predicted: false, revision: revision)
            deliveredBatches += 1; deliveredSamples += UInt64(batch.count / 9)
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
            batch += [translation[0] + sample.x * zoom, translation[1] + sample.y * zoom,
                sample.pressure, 0.2, 0.1, 0.3, 0, Double(timestamp), sample.phase]
        }
        deliver()
        if plan.predicts, let contact = lastContact {
            var predicted: [Double] = []
            for index in nextSample..<(nextSample + 2) {
                guard let sample = plan.sample(index), sample.contact == contact, sample.phase == 2 else { break }
                let time = started + UInt64(Double(index) / Double(DrawingWorkloadPlan.samplesPerSecond) * 1e9)
                predicted += [translation[0] + sample.x * zoom, translation[1] + sample.y * zoom,
                    sample.pressure, 0.2, 0.1, 0.3, 0, Double(time), 2]
            }
            if !predicted.isEmpty {
                store.native?.pointer(id: contact, tool: 0, button: 0, records: predicted, predicted: true, revision: revision)
            }
        }
        if deliveredBatches != previousBatches { store.wake?() }
        if now - lastReport >= 1_000_000_000 {
            record(6, d: deliveredSamples, e: deliveredBatches, f: maximumLateness)
            lastReport = now; maximumLateness = 0
        }
    }
    private func finish(failed: Bool) {
        guard !finished else { return }
        finished = true; timer?.invalidate(); timer = nil
        if let contact = lastContact, lastRecord.count == 9, let store {
            lastRecord[7] = Double(FrameTrace.now()); lastRecord[8] = 3
            store.native?.pointer(id: contact, tool: 0, button: 0, records: lastRecord, predicted: false, revision: store.cameraRevision)
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
