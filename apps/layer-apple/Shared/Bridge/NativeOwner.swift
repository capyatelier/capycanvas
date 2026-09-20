import Foundation
import QuartzCore
import Metal

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
    private var surfaceSize: (width: UInt32, height: UInt32, scale: Float)?
    private var gpuHealth: DispatchSourceTimer?
    private var hdrDocument = false
    private var selectedDocument: UInt64 = 1
    /// Returning from occlusion/suspension needs a fresh frame even when the
    /// document has no further edits.
    func displayHeadroom(_ value: Double) {
        perform { [self] in
            _ = try request(2, JSON(["type": "display_headroom", "value": value]))
            try publish()
        }
    }
    func redraw() {
        perform { [self] in try check(capy_apple_redraw(handle)) }
    }
    private var lastSnapshotTime: UInt64 = 0
    private var bundledFiltersLoaded = false
    private var canvasReady = false
    private var shadersReady = false
    private let trace: FrameTrace?
    private var latestTracedInput: UInt64 = 0
    private var gpuTimingEnabled = false
    private var gpuPollScheduled = false
    private var lastGpuClockSample: UInt64 = 0
    private var gpuSamples = [CapyGpuFrameSample](repeating: CapyGpuFrameSample(), count: 8)
    private var lastTraceState: UInt64?
    private let persistence: EditorPersistence
    private let managedWorkspaces: Bool
    private let observerID = UUID()
    private var settingsRequests = Set<UInt64>()
    private var settingsWrites = 0
    private var lastSettingsData: Data?
    private var failedSettingsWrite = false
    private var currentSettings = JSON()
    private var latestSettings: EditorPersistence.SettingsChange?
    private var appliedSettingsRevision: UInt64 = 0
    private var storageError: String?
    private var lastStorageStatus: Data?
    #if DEBUG
    private var initialActions: [JSON] = []
    private var workspaceInitialized = false
    private var surfaceSized = false
    #endif
    let receive: @Sendable (JSON?, String?) -> Void
    var persistenceRoot: URL? { persistence.root }

    init(platform: UInt32, persistence: EditorPersistence = .shared,
        traceDuration: TimeInterval? = nil, workload: [String: Any]? = nil, managedWorkspaces: Bool = false,
        receive: @escaping @Sendable (JSON?, String?) -> Void) throws {
        #if DEBUG
        let fixtureActions = try ProcessInfo.processInfo.environment["CAPY_INITIAL_ACTIONS"].map { try JSON.decode($0).array } ?? []
        #endif
        // Drain temporary native/Metal objects after each owner task, including
        // the last frame before idle, rather than inheriting a worker pool.
        let queue = DispatchQueue(label: "art.capycanvas.render", qos: .userInteractive,
            autoreleaseFrequency: .workItem)
        guard let handle = queue.sync(execute: { capy_apple_create(platform) }) else {
            throw HostFailure(message: "Could not create the native canvas session")
        }
        self.queue = queue; self.handle = handle; self.receive = receive
        self.persistence = persistence; self.managedWorkspaces = managedWorkspaces
        #if DEBUG
        initialActions = fixtureActions
        workspaceInitialized = !managedWorkspaces
        #endif
        trace = FrameTrace.configured(platform: platform, defaultDuration: traceDuration, workload: workload)
        // Reserve the first owner operation before exposing this instance.
        // Disk reads run on the I/O queue; no input or surface task can overtake
        // restoration, and the UI thread never waits for the filesystem.
        let loaded = PersistenceLoad()
        queue.suspend()
        queue.async { [self] in restore(loaded.value()) }
        persistence.load(observer: observerID, changed: { [weak self] change in
            self?.perform { [weak self] in
                guard let self else { return }
                if change.revision > appliedSettingsRevision { latestSettings = change }
                try applySharedSettings(); try publish()
            }
        }) { value in loaded.set(value); queue.resume() }
    }
    deinit {
        gpuHealth?.cancel()
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
        if let snapshot = try request(7) {
            if !snapshot["document_tabs"].isNull { selectedDocument = snapshot["document_tabs"]["selected"].uint }
            if !snapshot["proof_panel"].isNull {
                let hdr = snapshot["proof_panel"]["hdr"].bool
                if hdr != hdrDocument {
                    hdrDocument = hdr
                    gpuHealth?.schedule(deadline: .now(), repeating: hdr ? 0.2 : 1, leeway: .milliseconds(50))
                }
            }
            if !snapshot["canvas_ready"].isNull { canvasReady = snapshot["canvas_ready"].bool }
            if !snapshot["shaders_ready"].isNull { shadersReady = snapshot["shaders_ready"].bool }
            try persist(snapshot)
            receive(snapshot, nil)
        }
    }
    func filterPreviews(_ query: JSON, completion: @escaping @Sendable (FilterPreviewReply) -> Void) {
        queue.async { [self] in
            do {
                let status = try request(2, query) ?? JSON()
                let pointer = capy_apple_take_filter_previews(handle)
                if let error = capy_apple_error(handle) { throw HostFailure(message: String(cString: error)) }
                completion(FilterPreviewReply(status: status, atlas: pointer.map(NativeFilterPreviews.init), error: nil))
            } catch { completion(FilterPreviewReply(status: JSON(), atlas: nil, error: error.localizedDescription)) }
        }
    }
    func navigatorPlacements(_ value: JSON) {
        perform { [self] in
            let source = try value.encoded()
            try check(source.withCString { capy_apple_navigator_placements(handle, $0) })
            // Layout can settle after the canvas driver has gone idle.
            receive(nil, nil)
        }
    }
    /// Capture/transition/adoption alone run on the drawing owner. Storage and
    /// manager policy use NativeWorkspaceLibrary's independent serial queue.
    func workspaceSession(_ value: JSON, completion: @escaping @Sendable (JSON?, String?) -> Void) {
        queue.async { [self] in
            do {
                let reply = try request(6, value)
                try publish(); completion(reply, nil)
            } catch { completion(nil, error.localizedDescription) }
        }
    }
    private func restore(_ loaded: EditorPersistence.Loaded) {
        storageError = loaded.error
        if let data = loaded.settings {
            do {
                let value = try JSONSerialization.jsonObject(with: data)
                _ = try request(0, JSON(["type": "restore_settings", "settings": value]))
            } catch { storageError = "Could not restore settings: \(error.localizedDescription)" }
        }
        do {
            if managedWorkspaces { _ = try request(6, JSON(["type": "read_only", "value": true])) }
            try publish()
        } catch { receive(nil, error.localizedDescription) }
        reportStorage()
    }
    private func persist(_ snapshot: JSON) throws {
        guard !snapshot["state"].isNull else { return }
        currentSettings = snapshot["state"]["settings"]
        for request in snapshot["state"]["requests"].array where request["kind"]["type"].string == "save_settings" {
            let id = request["id"].uint
            guard !settingsRequests.contains(id) else { continue }
            let data = try JSONSerialization.data(withJSONObject: request["kind"]["settings"].raw, options: [.sortedKeys])
            settingsRequests.insert(id)
            saveSettings(data, request: id)
        }
        reportStorage()
    }
    private func saveSettings(_ data: Data, request id: UInt64?) {
        lastSettingsData = data; settingsWrites += 1
        persistence.saveSettings(data) { [self] error in
            perform { [self] in
                if let id {
                    _ = try self.request(0, JSON(["type": "complete_request", "id": id, "error": error as Any? ?? NSNull()]))
                    settingsRequests.remove(id)
                }
                settingsWrites -= 1; storageError = error; failedSettingsWrite = error != nil
                try applySharedSettings(); try publish(); reportStorage()
            }
        }
    }
    func retryPersistence() {
        perform { [self] in
            if failedSettingsWrite, settingsWrites == 0, let data = lastSettingsData { saveSettings(data, request: nil) }
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
    func projectTask(kind: NativeProjectTask.Kind, expected: (UInt64, UInt64)? = nil,
        placement: JSON? = nil,
        completion: @escaping @Sendable (NativeProjectTask?, String?) -> Void) {
        let placementText: String?
        do { placementText = try placement?.encoded() }
        catch { completion(nil, error.localizedDescription); return }
        let deadline = DispatchTime.now() + .seconds(30)
        @Sendable func poll() {
            // Inspection validates a committed snapshot in Rust; it does not
            // wait for unrelated filter-library compilation or block drawing.
            let ready = kind == .histogram ? 0 : kind == .save ? capy_apple_prepare_recovery(handle, FrameTrace.now()) : capy_apple_project_ready(handle)
            if ready == 1 {
                if DispatchTime.now() < deadline { queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: poll) }
                else { completion(nil, "Document preparation timed out") }
                return
            }
            if ready < 0 { completion(nil, capy_apple_error(handle).map(String.init(cString:)) ?? "Document is unavailable"); return }
            let pointer: OpaquePointer?
            if let placementText { pointer = placementText.withCString { capy_apple_project_task(handle, kind.rawValue, $0) } }
            else { pointer = capy_apple_project_task(handle, kind.rawValue, nil) }
            guard let pointer else {
                completion(nil, capy_apple_error(handle).map(String.init(cString:)) ?? "Document is unavailable")
                return
            }
            let task = NativeProjectTask(pointer)
            if let expected, capy_project_matches(task.handle, expected.0, expected.1) == 0 {
                completion(nil, "The document changed; review those changes before opening another drawing")
            } else { completion(task, nil) }
        }
        queue.async(execute: poll)
    }
    /// Source conversion and GPU previews stay on the worker. Only shared edit
    /// validation/candidate construction enters the serial document owner.
    func prepareEdit(_ task: NativeProjectTask, choice: JSON?, copy: Bool,
        completion: @escaping @Sendable (String?) -> Void) {
        NativeProjectTask.io.async { [self] in
            do {
                try task.prepareEdit(choice, copy: copy)
                queue.async { [self] in
                    do {
                        try check(capy_apple_project_candidate(handle, task.handle))
                        NativeProjectTask.io.async {
                            do { try task.compare(); completion(nil) }
                            catch { completion(error.localizedDescription) }
                        }
                    } catch { completion(error.localizedDescription) }
                }
            } catch { completion(error.localizedDescription) }
        }
    }
    /// Capture only a committed raster boundary. Active ink may continue; its
    /// preceding committed pixels remain recoverable until the next pen-up.
    func recoveryTask(document: UInt64? = nil, expected: (UInt64, UInt64), completion: @escaping @Sendable (NativeProjectTask?, String?) -> Void) {
        queue.async { [self] in
            defer { try? publish() }
            let id = document ?? selectedDocument
            let ready = id == selectedDocument ? capy_apple_prepare_recovery(handle, FrameTrace.now()) : 0
            guard ready == 0 else {
                completion(nil, ready < 0 ? capy_apple_error(handle).map(String.init(cString:)) : nil); return
            }
            guard let pointer = capy_apple_document_recovery(handle, id) else {
                completion(nil, capy_apple_error(handle).map(String.init(cString:))); return
            }
            let task = NativeProjectTask(pointer)
            completion(capy_project_matches(pointer, expected.0, expected.1) == 1 ? task : nil, nil)
        }
    }
    /// Poll only while document/export shaders prepare. GPU synchronization and
    /// pixel packing happen later on the file worker through the returned job.
    func exportTask(id: UInt64, completion: @escaping @Sendable (NativeProjectTask?, String?) -> Void) {
        let deadline = DispatchTime.now() + .seconds(30)
        @Sendable func poll() {
            var pointer: OpaquePointer?
            let ready = capy_apple_project_ready(handle)
            let result = ready == 1 ? 0 : capy_apple_export_task(handle, UInt32(id), FrameTrace.now(), &pointer)
            if result < 0 { completion(nil, capy_apple_error(handle).map(String.init(cString:)) ?? "Export failed") }
            else if let pointer { completion(NativeProjectTask(pointer), nil) }
            else if DispatchTime.now() >= deadline { completion(nil, "The canvas is not ready to export") }
            else { queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: poll) }
        }
        queue.async(execute: poll)
    }
    func finishProject(_ task: NativeProjectTask, opening: Bool, title: String, url: URL?, recovered: Bool = false,
        completion: @escaping @Sendable (String?) -> Void) {
        let deadline = DispatchTime.now() + .seconds(15)
        @Sendable func attempt() {
            do {
                let ready = opening ? capy_apple_project_prepare_adopt(handle, task.handle, FrameTrace.now()) : 0
                if ready == 1 {
                    guard DispatchTime.now() < deadline else { throw HostFailure(message: "Drawing capture did not finish") }
                    queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: attempt); return
                }
                try check(ready)
                try title.withCString { name in
                    try (url?.absoluteString ?? "").withCString { uri in
                        try check(recovered ? capy_apple_project_recover(handle, task.handle)
                            : opening ? capy_apple_project_adopt(handle, task.handle, name, uri)
                            : capy_apple_project_saved(handle, task.handle, name, uri))
                    }
                }
                try publish(); completion(nil)
                if opening { documentStorage() }
            } catch { completion(error.localizedDescription) }
        }
        queue.async(execute: attempt)
    }
    private final class DocumentJob: @unchecked Sendable {
        let handle: OpaquePointer
        init(_ handle: OpaquePointer) { self.handle = handle }
        deinit { let pointer = handle; NativeProjectTask.io.async { capy_document_free(pointer) } }
    }
    func switchDocument(_ id: UInt64, closing: Bool = false, completion: @escaping @Sendable (String?) -> Void) {
        let deadline = DispatchTime.now() + .seconds(15)
        @Sendable func attempt() {
            do {
                let ready = capy_apple_document_prepare_switch(handle, FrameTrace.now())
                if ready == 1 {
                    guard DispatchTime.now() < deadline else { throw HostFailure(message: "Drawing capture did not finish") }
                    queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: attempt); return
                }
                try check(ready)
                guard let task = capy_apple_document_switch(handle, id, closing) else { try check(-1); return }
                runDocumentJob(DocumentJob(task), completion: completion)
            } catch { completion(error.localizedDescription) }
        }
        queue.async(execute: attempt)
    }
    private func documentStorage() {
        guard let task = capy_apple_document_storage(handle) else { return }
        runDocumentJob(DocumentJob(task)) { [weak self] error in if let error { self?.receive(nil, error) } }
    }
    private func runDocumentJob(_ job: DocumentJob, completion: @escaping @Sendable (String?) -> Void) {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("capy-apple-drawing-tiles", isDirectory: true)
        NativeProjectTask.io.async { [self, job] in
            _ = directory.path.withCString { capy_document_prepare(job.handle, $0) }
            queue.async { [self, job] in
                do { try check(capy_apple_document_resume(handle, job.handle)); try publish(); completion(nil) }
                catch { try? publish(); completion(error.localizedDescription) }
            }
        }
    }
    func proofTask(id: UInt64, recipe: JSON?, completion: @escaping @Sendable (NativeProjectTask?, String?) -> Void) {
        let text: String
        do { text = try (recipe ?? JSON()).encoded() }
        catch { completion(nil, error.localizedDescription); return }
        queue.async { [self] in
            let pointer = text.withCString { capy_apple_proof_task(handle, UInt32(id), $0) }
            completion(pointer.map(NativeProjectTask.init), pointer == nil
                ? capy_apple_error(handle).map(String.init(cString:)) ?? "Proof is unavailable" : nil)
        }
    }
    func checkProof(_ task: NativeProjectTask, completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do { try check(capy_apple_proof_check(handle, task.handle)); completion(nil) }
            catch { completion(error.localizedDescription) }
        }
    }
    func finishProof(_ task: NativeProjectTask, preserved: Bool = false, failure: String? = nil,
        completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do {
                if let failure { try failure.withCString { try check(capy_apple_proof_failed(handle, task.handle, $0)) } }
                else { try check(capy_apple_proof_apply(handle, task.handle, preserved)) }
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
        storageError = nil
        guard !NSDictionary(dictionary: currentSettings.object).isEqual(settings) else { return }
        _ = try request(0, JSON(["type": "restore_settings", "settings": settings]))
    }
    private func reportStorage() {
        let status = JSON(["pending": settingsWrites, "error": storageError as Any? ?? NSNull(),
            "can_retry": failedSettingsWrite])
        guard let data = try? JSONSerialization.data(withJSONObject: status.raw, options: [.sortedKeys]), data != lastStorageStatus else { return }
        lastStorageStatus = data
        receive(JSON(["persistence": status.raw]), nil)
    }
    /// A barrier across both queues includes accepted edits, their writes and
    /// acknowledgments. Lifecycle adapters can hold a background/termination
    /// allowance without synchronously blocking the UI or render owner.
    // This prepares the committed snapshot and preferences only. EditorStore
    // separately waits for ArtworkRecovery's project worker and atomic manifest
    // publication before reporting that the lifecycle barrier has succeeded.
    func flushPersistence(_ completion: @escaping @Sendable (Bool) -> Void) {
        let deadline = DispatchTime.now() + .seconds(10)
        @Sendable func poll() {
            let result = persistence.root == nil ? 0 : capy_apple_prepare_recovery(handle, FrameTrace.now())
            do { try publish() } catch { receive(nil, error.localizedDescription) }
            if result == 1 && DispatchTime.now() < deadline {
                queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: poll); return
            }
            persistence.flush { [self] in queue.async { [self] in completion(result == 0 && storageError == nil) } }
        }
        queue.async(execute: poll)
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
    /// Resolve and apply the shared placement action on the same serial owner.
    func headerAction(_ value: JSON, completion: @escaping @Sendable () -> Void) {
        queue.async { [self] in
            defer { completion() }
            do {
                if let action = try request(2, JSON(["type":"header", "request":value.raw])), !action.isNull {
                    _ = try request(0, action)
                    try publish()
                }
            } catch { receive(nil, error.localizedDescription) }
        }
    }
    func attach(_ layer: CAMetalLayer, width: UInt32, height: UInt32, scale: Float) {
        let lease = MetalLayerLease(layer)
        perform { [self] in
            let previous = self.layer
            defer { withExtendedLifetime(previous) {}; try? publish() }
            self.layer = lease.value
            surfaceSize = (width, height, scale)
            startGpuHealthChecks()
            try attachCurrentLayer()
            #if DEBUG
            surfaceSized = true
            #endif
            try applyInitialActions()
        }
    }
    private func attachCurrentLayer() throws {
        guard let layer, let size = surfaceSize else { throw HostFailure(message: "The canvas has no presentation surface") }
        let cache = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("art.capycanvas.apple.shader-pipelines", isDirectory: true)
        try cache.path.withCString {
            try check(capy_apple_attach(handle, Unmanaged.passUnretained(layer).toOpaque(), size.width, size.height, size.scale, $0))
        }
    }
    private func startGpuHealthChecks() {
        guard gpuHealth == nil else { return }
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + 1, repeating: hdrDocument ? 0.2 : 1, leeway: .milliseconds(50))
        timer.setEventHandler { [weak self] in
            guard let self else { return }
            // EDR headroom can change while the display link is asleep. Poll
            // only HDR documents, using the existing foreground health timer.
            if hdrDocument { receive(JSON(["display_poll": true]), nil) }
            // Failure callbacks can arrive after the display link goes idle.
            // Healthy checks do not publish UI state or submit canvas work.
            let status = capy_apple_poll_renderer(handle)
            if status != 0 {
                do { try check(status); try publish() }
                catch { try? publish(); receive(nil, error.localizedDescription) }
            }
        }
        gpuHealth = timer; timer.resume()
    }
    #if DEBUG
    func testGpuFault(validation: Bool) {
        guard ProcessInfo.processInfo.environment["CAPY_GPU_RECOVERY_TEST"] == "1" else { return }
        perform { [self] in try check(capy_apple_test_gpu_fault(handle, validation ? 1 : 0)) }
    }
    #endif
    func restartCanvas(_ completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do {
                try check(capy_apple_suspend_renderer(handle))
                try publish()
                try attachCurrentLayer()
                try publish(); completion(nil)
            } catch {
                try? publish(); completion(error.localizedDescription)
            }
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
            _ = try request(2, JSON(["type": "load_filter_package", "manifest": manifest, "modules": modules, "mode": "merge", "library": true]))
        }
        bundledFiltersLoaded = true
        try publish()
    }
    func resize(width: UInt32, height: UInt32, scale: Float) {
        perform { [self] in
            try check(capy_apple_resize(handle, width, height, scale))
            surfaceSize = (width, height, scale)
            #if DEBUG
            surfaceSized = true
            #endif
            try applyInitialActions(); try publish()
        }
    }
    private func applyInitialActions() throws {
        #if DEBUG
        guard workspaceInitialized && surfaceSized && canvasReady else { return }
        if initialActions.contains(where: { $0["type"].string == "workspace_manager" }) {
            // Workspace fixture commands have the same idle requirement as
            // their UI entries. Bundled filter preparation can outlive launch.
            guard canvasReady && shadersReady && bundledFiltersLoaded,
                try request(6, JSON(["type": "capture", "generation": 0]))?["idle"].bool == true else { return }
        }
        // Fixture actions can collapse or resize columns. Apply them once,
        // after restoration and the first real surface size, not at 1×1 startup.
        let actions = initialActions; initialActions = []
        for action in actions { _ = try request(0, action) }
        #endif
    }
    func workspaceDidInitialize() {
        #if DEBUG
        perform { [self] in
            workspaceInitialized = true
            try applyInitialActions(); try publish()
        }
        #endif
    }
    func detach() {
        perform { [self] in
            try check(capy_apple_detach(handle))
            gpuHealth?.cancel(); gpuHealth = nil
            layer = nil; surfaceSize = nil
        }
    }
    func pointer(id: UInt64, tool: UInt32, button: UInt32, records: [Double], predicted: Bool, revision: UInt64,
                 updates: [UInt64] = [], correction: Bool = false) {
        precondition(updates.isEmpty || updates.count == records.count / 9 * 2)
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
                        g: correction ? 2 : predicted ? 1 : 0, h: FrameTrace.timestamp(records.last ?? 0), i: UInt64(tool), j: succeeded ? 1 : 0))
                    if succeeded && !predicted { latestTracedInput = queued }
                }
            }
            try records.withUnsafeBufferPointer {
                if updates.isEmpty {
                    try check(capy_apple_pointer(handle, id, tool, button, $0.baseAddress, $0.count, predicted ? 1 : 0, revision))
                } else {
                    let samples = $0
                    try updates.withUnsafeBufferPointer {
                        try check(capy_apple_pointer_updates(handle, id, tool, button, samples.baseAddress, samples.count,
                            $0.baseAddress, correction ? 1 : 0, revision))
                    }
                }
            }
            succeeded = true
        }
    }
    func observeTick(now: UInt64, target: UInt64, admitted: Bool, denial: UInt64 = 0) {
        if let trace, trace.isRecording {
            trace.record(FrameTraceEvent(kind: .tick, a: now, b: target, c: admitted ? 1 : 0, d: denial))
        }
    }
    func observeWorkload(_ event: FrameTraceEvent) {
        if let trace, trace.isRecording { trace.record(event) }
    }
    func finishTrace() { perform { [self] in trace?.finish() } }
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
        guard observation.recordsGpuTiming, observation.acceptsCompletions else { return }
        sampleGpuClock(observation)
        var status = CapyGpuFrameTimingStats()
        let capacity = gpuSamples.count
        let count = capy_apple_take_gpu_timing(handle, &gpuSamples, capacity, &status)
        if count >= 0 {
            for sample in gpuSamples.prefix(Int(count)) {
                observation.record(FrameTraceEvent(kind: .gpu, a: sample.frame, b: sample.elapsed_ns, c: sample.status,
                    d: sample.start_tick, e: sample.end_tick))
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
    private func sampleGpuClock(_ observation: FrameTrace) {
        guard let device = layer?.device else { return }
        let before = FrameTrace.now()
        // Clock sampling can enter the kernel. Keep it out of ordinary drawing
        // and limit the optional recorder to ten samples per second.
        guard before &- lastGpuClockSample >= 100_000_000 else { return }
        let clocks = device.sampleTimestamps()
        observation.record(FrameTraceEvent(kind: .gpuClock, a: before, b: clocks.cpu,
            c: clocks.gpu, d: FrameTrace.now()))
        lastGpuClockSample = before
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
                let wantsGpuTiming = observation?.recordsGpuTiming == true
                if gpuTimingEnabled != wantsGpuTiming {
                    try check(capy_apple_gpu_timing(handle, wantsGpuTiming ? 1 : 0))
                    gpuTimingEnabled = wantsGpuTiming
                }
                if wantsGpuTiming, let observation { sampleGpuClock(observation) }
                let result = capy_apple_frame(handle, now, max(now, target), &costs)
                try check(result)
                // Always flush the final state before the display link sleeps.
                // Throttling the pen-up frame can otherwise leave Undo/layers
                // stale indefinitely, until an unrelated action wakes the UI.
                if result == 0 || now >= lastSnapshotTime + 33_000_000 {
                    try publish(); lastSnapshotTime = now
                }
                #if DEBUG
                if !initialActions.isEmpty {
                    try applyInitialActions()
                    if initialActions.isEmpty { try publish() }
                }
                #endif
                if canvasReady && !bundledFiltersLoaded { try loadBundledFilters() }
                // Catalog ownership survives GPU replacement. Finish each new
                // device's startup gate without loading that catalog again;
                // the native operation is idempotent while shaders finish.
                if canvasReady && !shadersReady { try check(capy_apple_finish_startup_cache(handle)) }
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
                try? publish()
                receive(nil, error.localizedDescription)
                completion(false, capy_apple_camera_revision(handle), costs)
            }
        }
    }
}
