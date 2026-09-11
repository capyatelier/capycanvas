import Foundation
import QuartzCore

/// ARC lease crossing the queue boundary. UIKit/AppKit owns view geometry;
/// only the render owner uses the layer's Metal surface and drawable APIs.
private final class MetalLayerLease: @unchecked Sendable {
    let value: CAMetalLayer
    init(_ value: CAMetalLayer) { self.value = value }
}

private final class PersistenceLoad: @unchecked Sendable {
    private let lock = NSLock()
    private var loaded = EditorPersistence.Loaded()
    func set(_ value: EditorPersistence.Loaded) { lock.lock(); loaded = value; lock.unlock() }
    func value() -> EditorPersistence.Loaded { lock.lock(); defer { lock.unlock() }; return loaded }
}

/// The only owner of Rust and GPU state. UI callbacks submit owned input batches.
final class NativeOwner: @unchecked Sendable {
    private let queue: DispatchQueue
    private let handle: OpaquePointer
    private var layer: CAMetalLayer?
    private var lastSnapshotTime: UInt64 = 0
    private var bundledFiltersLoaded = false
    private var canvasReady = false
    private var shadersReady = false
    private let trace: FrameTrace?
    private var latestTracedInput: UInt64 = 0
    private var gpuTimingEnabled = false
    private var gpuPollScheduled = false
    private var gpuSamples = [CapyGpuFrameSample](repeating: CapyGpuFrameSample(), count: 8)
    private var lastTraceState: UInt64?
    private let persistence: EditorPersistence
    private let scene: String
    private let observerID = UUID()
    private var settingsRequests = Set<UInt64>()
    private var settingsWrites = 0
    private var workspaceWrites = 0
    private var lastSettingsData: Data?
    private var lastWorkspaceData: Data?
    private var lastWorkspaceUpdatesDefault = true
    private var failedSettingsWrite = false
    private var failedWorkspaceWrite = false
    private var firstWorkspace = true
    private var initialWorkspaceNeedsSave = false
    private var currentSettings = JSON()
    private var latestSettings: EditorPersistence.SettingsChange?
    private var appliedSettingsRevision: UInt64 = 0
    private var storageErrors: [String: String] = [:]
    private var lastStorageStatus: Data?
    let receive: @Sendable (JSON?, String?) -> Void

    init(platform: UInt32, scene: String, persistence: EditorPersistence = .shared,
        receive: @escaping @Sendable (JSON?, String?) -> Void) throws {
        let queue = DispatchQueue(label: "art.capycanvas.render", qos: .userInteractive)
        guard let handle = queue.sync(execute: { capy_apple_create(platform) }) else {
            throw HostFailure(message: "Could not create the native canvas session")
        }
        self.queue = queue; self.handle = handle; self.receive = receive
        self.persistence = persistence; self.scene = scene
        trace = FrameTrace.configured(platform: platform)
        // Reserve the first owner operation before exposing this instance.
        // Disk reads run on the I/O queue; no input or surface task can overtake
        // restoration, and the UI thread never waits for the filesystem.
        let loaded = PersistenceLoad()
        queue.suspend()
        queue.async { [self] in restore(loaded.value()) }
        persistence.load(scene: scene, observer: observerID, changed: { [weak self] change in
            self?.perform { [weak self] in
                guard let self else { return }
                if change.revision > appliedSettingsRevision { latestSettings = change }
                try applySharedSettings(); try publish()
            }
        }) { value in loaded.set(value); queue.resume() }
    }
    deinit {
        persistence.unsubscribe(observerID)
        trace?.finish()
        let handle = handle, retainedLayer = layer
        queue.async {
            capy_apple_detach(handle)
            capy_apple_destroy(handle)
            withExtendedLifetime(retainedLayer) {}
        }
    }
    private func check(_ result: Int32) throws {
        if result < 0 { throw HostFailure(message: capy_apple_error(handle).map(String.init(cString:)) ?? "Native operation failed") }
    }
    private func request(_ kind: UInt32, _ value: JSON = JSON()) throws -> JSON? {
        let source = try value.encoded()
        let result = source.withCString { capy_apple_request(handle, kind, $0) }
        guard let result else {
            if let error = capy_apple_error(handle) { throw HostFailure(message: String(cString: error)) }
            return nil
        }
        defer { capy_apple_string_free(result) }
        return try JSON.decode(String(cString: result))
    }
    private func publish() throws {
        if let snapshot = try request(3) {
            if !snapshot["canvas_ready"].isNull { canvasReady = snapshot["canvas_ready"].bool }
            if !snapshot["shaders_ready"].isNull { shadersReady = snapshot["shaders_ready"].bool }
            try persist(snapshot)
            receive(snapshot, nil)
        }
    }
    private func restore(_ loaded: EditorPersistence.Loaded) {
        for (key, data, action) in [("settings", loaded.settings, "restore_settings"),
            ("workspace", loaded.workspace, "restore_workspace")] {
            guard let data else { continue }
            do {
                let value = try JSONSerialization.jsonObject(with: data)
                _ = try request(0, JSON(["type": action, key: value]))
            } catch { storageErrors[key] = "Could not restore \(key): \(error.localizedDescription)" }
        }
        storageErrors.merge(loaded.errors) { _, new in new }
        initialWorkspaceNeedsSave = loaded.workspaceNeedsSnapshot && storageErrors["workspace"] == nil
        do { try publish() } catch { receive(nil, error.localizedDescription) }
        reportStorage()
    }
    private func persist(_ snapshot: JSON) throws {
        guard !snapshot["state"].isNull else { return }
        currentSettings = snapshot["state"]["settings"]
        if !snapshot["workspace_persistence"].isNull {
            if !firstWorkspace || initialWorkspaceNeedsSave {
                let data = try JSONSerialization.data(withJSONObject: snapshot["workspace_persistence"].raw, options: [.sortedKeys])
                saveWorkspace(data, updateDefault: !firstWorkspace)
            }
            firstWorkspace = false
        }
        for request in snapshot["state"]["requests"].array where request["kind"]["type"].string == "save_settings" {
            let id = request["id"].uint
            guard !settingsRequests.contains(id) else { continue }
            let data = try JSONSerialization.data(withJSONObject: request["kind"]["settings"].raw, options: [.sortedKeys])
            settingsRequests.insert(id)
            saveSettings(data, request: id)
        }
        reportStorage()
    }
    private func saveWorkspace(_ data: Data, updateDefault: Bool = true) {
        lastWorkspaceData = data; lastWorkspaceUpdatesDefault = updateDefault; workspaceWrites += 1
        persistence.saveWorkspace(data, scene: scene, updateDefault: updateDefault) { [self] error in
            perform { [self] in
                workspaceWrites -= 1; storageErrors["workspace"] = error; failedWorkspaceWrite = error != nil; reportStorage()
            }
        }
    }
    private func saveSettings(_ data: Data, request id: UInt64?) {
        lastSettingsData = data; settingsWrites += 1
        persistence.saveSettings(data) { [self] error in
            perform { [self] in
                if let id {
                    _ = try self.request(0, JSON(["type": "complete_request", "id": id, "error": error as Any? ?? NSNull()]))
                    settingsRequests.remove(id)
                }
                settingsWrites -= 1; storageErrors["settings"] = error; failedSettingsWrite = error != nil
                try applySharedSettings(); try publish(); reportStorage()
            }
        }
    }
    func retryPersistence() {
        perform { [self] in
            if failedSettingsWrite, settingsWrites == 0, let data = lastSettingsData { saveSettings(data, request: nil) }
            if failedWorkspaceWrite, workspaceWrites == 0, let data = lastWorkspaceData {
                saveWorkspace(data, updateDefault: lastWorkspaceUpdatesDefault)
            }
            reportStorage()
        }
    }
    func documentRequest(id: UInt64 = 0, succeeded: Bool? = nil, closeDecision: UInt32? = nil,
        completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do {
                if let decision = closeDecision { try check(capy_apple_document_close(handle, UInt32(id), decision)) }
                else { try check(capy_apple_document_complete(handle, UInt32(id), succeeded == true ? 1 : 0)) }
                try publish(); completion(nil)
            } catch { completion(error.localizedDescription) }
        }
    }
    func projectTask(opening: Bool, expected: (UInt64, UInt64)? = nil,
        completion: @escaping @Sendable (NativeProjectTask?, String?) -> Void) {
        queue.async { [self] in
            guard let pointer = capy_apple_project_task(handle, opening ? 1 : 0) else {
                completion(nil, capy_apple_error(handle).map(String.init(cString:)) ?? "Document is unavailable")
                return
            }
            let task = NativeProjectTask(pointer)
            if let expected, capy_project_matches(task.handle, expected.0, expected.1) == 0 {
                completion(nil, "The document changed; review those changes before opening another drawing")
            } else { completion(task, nil) }
        }
    }
    func finishProject(_ task: NativeProjectTask, opening: Bool, title: String, url: URL?,
        completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do {
                try title.withCString { name in
                    try (url?.absoluteString ?? "").withCString { uri in
                        try check(opening ? capy_apple_project_adopt(handle, task.handle, name, uri)
                            : capy_apple_project_saved(handle, task.handle, name, uri))
                    }
                }
                try publish(); completion(nil)
            } catch { completion(error.localizedDescription) }
        }
    }
    private func applySharedSettings() throws {
        // A global notification must not roll back a newer local edit whose
        // write is still pending. Apply the newest committed settings once all
        // of this owner's writes have been acknowledged, including our own.
        guard settingsWrites == 0, !failedSettingsWrite, let change = latestSettings else { return }
        latestSettings = nil; appliedSettingsRevision = change.revision
        let settings = try JSONSerialization.jsonObject(with: change.data)
        storageErrors["settings"] = nil
        guard !NSDictionary(dictionary: currentSettings.object).isEqual(settings) else { return }
        _ = try request(0, JSON(["type": "restore_settings", "settings": settings]))
    }
    private func reportStorage() {
        let error = storageErrors.values.sorted().joined(separator: "\n")
        let status = JSON(["pending": settingsWrites + workspaceWrites, "error": error.isEmpty ? NSNull() : error as Any,
            "can_retry": failedSettingsWrite || failedWorkspaceWrite])
        guard let data = try? JSONSerialization.data(withJSONObject: status.raw, options: [.sortedKeys]), data != lastStorageStatus else { return }
        lastStorageStatus = data
        receive(JSON(["persistence": status.raw]), nil)
    }
    /// A barrier across both queues includes accepted edits, their writes and
    /// acknowledgments. Lifecycle adapters can hold a background/termination
    /// allowance without synchronously blocking the UI or render owner.
    func flushPersistence(_ completion: @escaping @Sendable (Bool) -> Void) {
        queue.async { [self] in
            persistence.flush { [self] in queue.async { [self] in completion(storageErrors.isEmpty) } }
        }
    }
    private func perform(_ work: @escaping @Sendable () throws -> Void) {
        queue.async { [self] in
            do { try work() } catch { receive(nil, error.localizedDescription) }
        }
    }
    func submit(_ kind: UInt32, _ value: JSON, completion: (@Sendable (JSON?) -> Void)? = nil) {
        queue.async { [self] in
            do {
                let result = try request(kind, value)
                try publish()
                completion?(result)
            } catch {
                receive(nil, error.localizedDescription)
                completion?(nil)
            }
        }
    }
    /// Editable controls own validation feedback. Publish the resulting state
    /// before acknowledging the edit, including when semantic validation fails.
    func edit(_ action: JSON, completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            var failure: String?
            do { _ = try request(0, action) }
            catch { failure = error.localizedDescription }
            do { try publish() }
            catch { receive(nil, error.localizedDescription) }
            completion(failure)
        }
    }
    func attach(_ layer: CAMetalLayer, width: UInt32, height: UInt32, scale: Float) {
        let lease = MetalLayerLease(layer)
        perform { [self] in
            let layer = lease.value
            let cache = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("art.capycanvas.apple.shader-pipelines", isDirectory: true)
            try cache.path.withCString {
                try check(capy_apple_attach(handle, Unmanaged.passUnretained(layer).toOpaque(), width, height, scale, $0))
            }
            self.layer = layer
            try publish()
        }
    }
    private func loadBundledFilters() throws {
        // Submit the optional catalog after paper/document readiness, allowing
        // priority document and brush shaders to enter the compiler queue first.
        if let url = Bundle.main.url(forResource: "manifest", withExtension: "json", subdirectory: "filters") {
            let manifest = try String(contentsOf: url, encoding: .utf8)
            let names = try request(2, JSON(["type": "filter_package_modules", "manifest": manifest]))?.array ?? []
            var modules: [String: String] = [:]
            for name in names {
                modules[name.string] = try String(contentsOf: url.deletingLastPathComponent().appendingPathComponent(name.string), encoding: .utf8)
            }
            _ = try request(2, JSON(["type": "load_filter_package", "manifest": manifest, "modules": modules, "mode": "merge"]))
        }
        try check(capy_apple_finish_startup_cache(handle))
        bundledFiltersLoaded = true
        try publish()
    }
    func resize(width: UInt32, height: UInt32, scale: Float) {
        perform { [self] in
            try check(capy_apple_resize(handle, width, height, scale)); try publish()
        }
    }
    func importLayer(_ url: URL) {
        // File I/O and decode must not stall the UI or the render/input owner.
        DispatchQueue.global(qos: .userInitiated).async { [self] in
            do {
                let image = try LayerImagePixels.decode(url)
                perform { [self] in
                    try image.name.withCString { name in
                        try image.rgba.withUnsafeBytes { bytes in
                            try check(capy_apple_import_layer(handle, name, image.width, image.height,
                                bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count))
                        }
                    }
                    try publish()
                }
            } catch { receive(nil, error.localizedDescription) }
        }
    }
    func detach() {
        perform { [self] in try check(capy_apple_detach(handle)); layer = nil }
    }
    func pointer(id: UInt64, tool: UInt32, button: UInt32, records: [Double], predicted: Bool, revision: UInt64) {
        let observation = trace.flatMap { $0.isRecording ? $0 : nil }
        let queued = observation == nil ? 0 : FrameTrace.now()
        perform { [self] in
            let start = observation == nil ? 0 : FrameTrace.now()
            var succeeded = false
            defer {
                if let observation {
                    let timestamps = stride(from: 7, to: records.count, by: 9).map { FrameTrace.timestamp(records[$0]) }
                    observation.record(FrameTraceEvent(kind: .input, a: queued, b: start, c: FrameTrace.now(),
                        d: timestamps.min() ?? 0, e: timestamps.max() ?? 0, f: UInt64(records.count / 9),
                        g: predicted ? 1 : 0, h: FrameTrace.timestamp(records.last ?? 0), i: UInt64(tool), j: succeeded ? 1 : 0))
                    if succeeded && !predicted { latestTracedInput = queued }
                }
            }
            try records.withUnsafeBufferPointer {
                try check(capy_apple_pointer(handle, id, tool, button, $0.baseAddress, $0.count, predicted ? 1 : 0, revision))
            }
            succeeded = true
        }
    }
    func observeTick(now: UInt64, target: UInt64, admitted: Bool) {
        if let trace, trace.isRecording { trace.record(FrameTraceEvent(kind: .tick, a: now, b: target, c: admitted ? 1 : 0)) }
    }
    func observeActivity(active: Bool) {
        if let trace, trace.isRecording { trace.record(FrameTraceEvent(kind: .activity, a: FrameTrace.now(), b: active ? 1 : 0)) }
    }
    func observeDisplay(width: UInt32, height: UInt32, scale: Float, maximumRefreshRate: Int) {
        if let trace, trace.isRecording {
            trace.record(FrameTraceEvent(kind: .display, a: FrameTrace.now(), b: UInt64(width), c: UInt64(height),
                d: FrameTrace.timestamp(Double(scale) * 1000), e: UInt64(max(0, maximumRefreshRate))))
        }
    }
    func scroll(x: Float, y: Float, dx: Float, dy: Float, scale: Float, zoom: Bool, horizontal: Bool) {
        perform { [self] in
            try check(capy_apple_scroll(handle, x, y, dx, dy, scale, zoom ? 1 : 0, horizontal ? 1 : 0))
            try publish()
        }
    }
    func gesture(x: Float, y: Float, scale: Float, rotation: Float) {
        perform { [self] in
            try check(capy_apple_gesture(handle, x, y, scale, rotation))
            try publish()
        }
    }
    /// Runs on the serial owner. At most one trailing poll can be scheduled,
    /// allowing the last GPU readback to complete after the display link sleeps.
    private func collectGpuTiming(_ observation: FrameTrace) {
        guard observation.acceptsCompletions else { return }
        var status = CapyGpuFrameTimingStats()
        let capacity = gpuSamples.count
        let count = capy_apple_take_gpu_timing(handle, &gpuSamples, capacity, &status)
        if count >= 0 {
            for sample in gpuSamples.prefix(Int(count)) {
                observation.record(FrameTraceEvent(kind: .gpu, a: sample.frame, b: sample.elapsed_ns, c: sample.status))
            }
        }
        observation.record(FrameTraceEvent(kind: .gpuStatus, a: FrameTrace.now(), b: status.support,
            c: status.requested, d: status.skipped, e: status.invalid, f: status.pending, g: count < 0 ? 1 : 0))
        guard status.pending > 0, count >= 0, !gpuPollScheduled else { return }
        gpuPollScheduled = true
        queue.asyncAfter(deadline: .now() + .milliseconds(20)) { [self, observation] in
            gpuPollScheduled = false
            collectGpuTiming(observation)
        }
    }
    /// One frame may be outstanding. Completion never means drawable presentation.
    func frame(now: UInt64, target: UInt64, completion: @escaping @Sendable (Bool, UInt64, [UInt64]) -> Void) {
        let observation = trace.flatMap { $0.isRecording ? $0 : nil }
        queue.async { [self] in
            var costs = [UInt64](repeating: 0, count: 5)
            let start = observation == nil ? 0 : FrameTrace.now()
            (layer as? ObservedMetalLayer)?.observation = observation.map { ($0, now) }
            defer {
                (layer as? ObservedMetalLayer)?.observation = nil
                observation?.record(FrameTraceEvent(kind: .frame, a: now, b: target, c: start, d: FrameTrace.now(),
                    e: costs[0], f: costs[1], g: costs[2], h: costs[3], i: costs[4], j: latestTracedInput))
                if let trace { collectGpuTiming(trace) }
            }
            do {
                if gpuTimingEnabled != (observation != nil) {
                    try check(capy_apple_gpu_timing(handle, observation == nil ? 0 : 1))
                    gpuTimingEnabled = observation != nil
                }
                let result = capy_apple_frame(handle, now, max(now, target), &costs)
                try check(result)
                // Always flush the final state before the display link sleeps.
                // Throttling the pen-up frame can otherwise leave Undo/layers
                // stale indefinitely, until an unrelated action wakes the UI.
                if result == 0 || now >= lastSnapshotTime + 33_000_000 {
                    try publish(); lastSnapshotTime = now
                }
                if canvasReady && !bundledFiltersLoaded { try loadBundledFilters() }
                if let observation {
                    let state: UInt64 = (canvasReady ? 1 : 0) | (bundledFiltersLoaded ? 2 : 0)
                        | (result == 1 ? 4 : 0) | (shadersReady ? 8 : 0)
                    if state != lastTraceState {
                        observation.record(FrameTraceEvent(kind: .state, a: FrameTrace.now(), b: now, c: state))
                        lastTraceState = state
                    }
                }
                completion(result == 1, capy_apple_camera_revision(handle), costs)
            } catch {
                observation?.record(FrameTraceEvent(kind: .state, a: FrameTrace.now(), b: now, d: 1))
                receive(nil, error.localizedDescription)
                completion(false, capy_apple_camera_revision(handle), costs)
            }
        }
    }
}
