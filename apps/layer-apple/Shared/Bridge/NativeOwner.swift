import Foundation
import QuartzCore
import Metal

/// ARC lease crossing the queue boundary. UIKit/AppKit owns view geometry;
/// only the render owner uses the layer's Metal surface and drawable APIs.
private final class MetalLayerLease: @unchecked Sendable {
    let value: CAMetalLayer
    init(_ value: CAMetalLayer) { self.value = value }
}

private struct NativeOwnerCleanup: @unchecked Sendable {
    let handle: UInt64?
    let layer: CAMetalLayer?
}

private final class LanguageJob: @unchecked Sendable {
    let handle: UInt64
    init(_ handle: UInt64) { self.handle = handle }
    deinit { capy_language_free(nativeValue(handle)) }
}

private final class SessionJob: @unchecked Sendable {
    let handle: UInt64
    private let failure: String
    private let access: ProjectFileAccess
    init(_ handle: UInt64, failure: String, access: ProjectFileAccess) { self.handle = handle; self.failure = failure; self.access = access }
    deinit { let handle = handle; NativeProjectTask.io.async { capy_session_free(nativeValue(handle)) } }
    func work() -> String? {
        let result = capy_session_work(nativeValue(handle), { uri, context in
            guard let uri, let context, let original = URL(string: String(cString: uri)), original.isFileURL else { return 0 }
            let job = Unmanaged<SessionJob>.fromOpaque(context).takeUnretainedValue()
            let url = (try? job.access.resolve(original.absoluteString)) ?? original
            var observed: UInt64?
            do {
                try ProjectFileIO.coordinate(url, writing: false) { source in
                    let file = try FileHandle(forReadingFrom: source)
                    defer { try? file.close() }
                    observed = nativeHandle(capy_session_destination_read(file.fileDescriptor))
                }
                return nativeValue(observed)
            } catch {
                capy_session_destination_free(nativeValue(observed))
                return 0
            }
        }, Unmanaged.passUnretained(self).toOpaque())
        return result < 0 ? diagnostic ?? failure : nil
    }
    var failures: [JSON] {
        guard let text = takeNativeText(capy_session_failures(nativeValue(handle))) else { return [] }
        return (try? JSON.decode(text))?.array ?? []
    }
    var diagnostic: String? {
        guard let text = takeNativeText(capy_session_error(nativeValue(handle))) else { return nil }

        return text
    }
}

private final class NativeLaunch: @unchecked Sendable {
    let handle: UInt64?
    let failure: String?
    init(configuration: String) {
        var error: UInt64 = 0
        handle = configuration.withCString { nativeHandle(capy_apple_prepare_launch($0, &error)) }
        failure = takeNativeText(error)

    }
    deinit { let handle = handle; NativeProjectTask.io.async { if let handle { capy_apple_launch_free(nativeValue(handle)) } } }
}
private final class PersistenceLoad: @unchecked Sendable {
    struct Value: Sendable {
        var storage = EditorPersistence.Loaded()
        var launch: NativeLaunch?
        var failure: String?
    }
    private let lock = NSLock()
    private var loaded = Value()
    func set(_ value: Value) { lock.lock(); loaded = value; lock.unlock() }
    func value() -> Value { lock.lock(); defer { lock.unlock() }; return loaded }
}

/// The only owner of Rust and GPU state. UI callbacks submit owned input batches.
struct ScreenReport: Equatable {
    var name = "", wide = false, headroom = 1.0
}

final class NativeOwner: @unchecked Sendable {
    private let queue: RenderOwnerQueue
    private let languageWorker = DispatchQueue(label: "art.capycanvas.language", qos: .userInitiated)
    private var languageReady = false
    private var languagePollScheduled = false
    private var requestedLanguagePreference: String?
    private var currentLanguagePreference = ""
    var languageInputBusy: (@MainActor @Sendable () -> Bool)?
    private var scopesReceived: (@Sendable (ScopeUpdate) -> Void)?
    private var scopesBusy = false
    private var handle: UInt64?
    private let platform: UInt32
    private var launchConfiguration: NativeLaunch?
    private var launchFailure: String?
    private var workspaceStart: JSON?
    private var layer: CAMetalLayer?
    private var surfaceSize: (width: UInt32, height: UInt32, scale: Float)?
    private var gpuHealth: DispatchSourceTimer?
    private var hdrDocument = false
    private var selectedDocument: UInt64 = 1
    private var penDevice: NativePenDevice?
    private var penDocumentEpoch: UInt64 = 0
    private var launchSessionPending = false
    private var pendingClose: DocumentJob?
    private var documentDeliveryCopy = JSON()
    /// Returning from occlusion/suspension needs a fresh frame even when the
    /// document has no further edits.
    func displayHeadroom(_ value: Double) {
        perform { [self] in
            _ = try request(2, JSON(["type": "display_headroom", "value": value]))
            try publish()
        }
    }
    func touchPolicy(milliseconds: UInt32, slop: Float) {
        perform { [self] in _ = try request(2, JSON(["type": "touch_policy", "tap_ms": Int(milliseconds), "slop": Double(slop)])) }
    }
    func screenReport(_ report: ScreenReport) {
        perform { [self] in
            _ = try request(2, JSON(["type": "screen_report", "name": report.name, "wide": report.wide, "headroom": report.headroom]))
            try publish()
        }
    }
    func redraw() {
        perform { [self] in try check(capy_apple_redraw(nativeValue(handle))) }
    }
    private var lastSnapshotTime: UInt64 = 0
    private var canvasReady = false
    private var shadersReady = false
    private var catalogLoaded = false
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
    private var preparationError: String?
    private var lastStorageStatus: Data?
    #if DEBUG
    private var initialActions: [JSON] = []
    private var workspaceInitialized = false
    private var surfaceSized = false
    #endif
    let receive: @Sendable (JSON?, String?) -> Void
    var sessions: URL? { persistence.locations?.sessions }

    init(platform: UInt32, persistence: EditorPersistence = .shared, screen: JSON? = nil,
        traceDuration: TimeInterval? = nil, workload: [String: Any]? = nil, managedWorkspaces: Bool = false,
        receive: @escaping @Sendable (JSON?, String?) -> Void) throws {
        #if DEBUG
        let fixtureActions = try ProcessInfo.processInfo.environment["CAPY_INITIAL_ACTIONS"].map { try JSON.decode($0).array } ?? []
        #endif
        let queue = RenderOwnerQueue()
        let preferredLanguages = Locale.preferredLanguages
        self.queue = queue; self.receive = receive
        self.persistence = persistence; self.managedWorkspaces = managedWorkspaces
        self.platform = platform
        launchSessionPending = persistence.locations != nil
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
        queue.async { [self] in
            do {
                let value = loaded.value()
                guard let launch = value.launch else { throw HostFailure(message:value.failure ?? "Native session initialization failed") }
                launchConfiguration = launch
                try self.launch()
                try publish()
                storageError = value.storage.error
                reportStorage()
            } catch {
                if handle == nil { launchFailure = error.localizedDescription }
                receive(nil, error.localizedDescription)
            }
        }
        persistence.load(observer: observerID, changed: { [weak self] change in
            self?.perform { [weak self] in
                guard let self else { return }
                if change.revision > appliedSettingsRevision { latestSettings = change }
                try applySharedSettings(); try publish()
            }
        }) { value in
            var prepared = PersistenceLoad.Value(storage:value)
            do {
                let saved = value.settings.map { String(decoding:$0,as:UTF8.self) } ?? ""
                let configuration = try JSON(["saved":saved,"preferred_languages":preferredLanguages,
                    "paper_directory":persistence.locations.map { $0.papers.path as Any } ?? NSNull(),
                    "screen":screen?.raw ?? NSNull()]).encoded()
                prepared.launch = NativeLaunch(configuration:configuration)
            } catch { prepared.failure = error.localizedDescription }
            loaded.set(prepared); queue.resume()
        }
    }
    private func launch() throws {
        var bootstrap: UInt64 = 0
        var error: UInt64 = 0
        guard let prepared = launchConfiguration?.handle else {
            throw HostFailure(message:launchConfiguration?.failure ?? "Native session initialization failed")
        }
        handle = nativeHandle(capy_apple_launch(platform,nativeValue(prepared), &bootstrap, &error))

        let launchError = takeNativeText(error)
        if let bootstrap = takeNativeText(bootstrap) {
            let view = try JSON.decode(bootstrap)
            let catalog = handle == nil ? nil : try request(2, JSON(["type": "catalog"]))
            catalogLoaded = catalog?.isNull == false
            receive(JSON(["bootstrap": view.raw, "catalog": catalog?.raw ?? NSNull()]), nil)
        }
        guard handle != nil else {
            launchFailure = launchError ?? "Native session initialization failed"
            throw HostFailure(message: launchFailure!)
        }
        launchFailure = nil
        launchConfiguration = nil
        if let workspaceStart { _ = try request(6, workspaceStart) }
    }
    deinit {
        gpuHealth?.cancel()
        persistence.unsubscribe(observerID)
        trace?.finish()
        let cleanup = NativeOwnerCleanup(handle: handle, layer: layer)
        queue.finish {
            (cleanup.layer as? ObservedMetalLayer)?.frameRate = nil
            capy_apple_detach(nativeValue(cleanup.handle))
            capy_apple_destroy(nativeValue(cleanup.handle))
            withExtendedLifetime(cleanup.layer) {}
        }
    }
    private func check(_ result: Int32) throws {
        if result < 0 { throw HostFailure(message: launchFailure ?? takeNativeText(capy_apple_error(nativeValue(handle))) ?? "Native operation failed") }
    }
    private func request(_ kind: UInt32, _ value: JSON = JSON()) throws -> JSON? {
        guard let handle else { throw HostFailure(message: launchFailure ?? "Native session is unavailable") }
        if kind == 0 && value["type"].string == "preferences" && value["action"]["id"].string == "language" {
            requestedLanguagePreference = nil
        }
        let source = try value.encoded()
        let result = source.withCString { takeNativeText(capy_apple_request(nativeValue(handle), kind, $0)) }
        guard let result else {
            if let error = takeNativeText(capy_apple_error(nativeValue(handle))) { throw HostFailure(message: error) }
            return nil
        }

        return try JSON.decode(result)
    }
    private func publish() throws {
        if let snapshot = try request(7) {
            if !snapshot["catalog"].isNull {
                catalogLoaded = true
                documentDeliveryCopy = snapshot["catalog"]["document_delivery_copy"]
            }
            if !snapshot["document_tabs"].isNull {
                let selected = snapshot["document_tabs"]["selected"].uint
                if selected != selectedDocument { penDevice = nil }
                selectedDocument = selected
            }
            if !snapshot["state"].isNull {
                let epoch = snapshot["state"]["document_file"]["epoch"].uint
                if epoch != penDocumentEpoch { penDevice = nil; penDocumentEpoch = epoch }
            }
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
            takeScopes()
        }
        try prepareLanguage()
    }
    private func takeScopes() {
        guard !scopesBusy, scopesReceived != nil,
            let scopes = nativeHandle(capy_apple_take_scopes(nativeValue(handle))) else { return }
        scopesBusy = true
        DispatchQueue.global(qos: .utility).async { [self] in
            var info = CapyScopeInfo()
            var update: ScopeUpdate?
            if capy_scopes_read(scopes, &info), let plots = takeNativeText(info.plots),
                let header = try? JSON.decode(plots),
                let pixels = copyNativeData(info.count, copy: { capy_scopes_copy(scopes, $0, $1, $2) }) {
                update = ScopeUpdate(header: header, pixels: pixels)
            }
            capy_scopes_free(scopes)
            let result = update
            queue.async { [self] in
                scopesBusy = false
                if let result { scopesReceived?(result) }
                takeScopes()
            }
        }
    }
    func setScopesReceiver(_ receive: @escaping @Sendable (ScopeUpdate) -> Void) {
        queue.async { [self] in scopesReceived = receive; takeScopes() }
    }
    private func prepareLanguage() throws {
        let preference = currentLanguagePreference
        guard preference != requestedLanguagePreference else { return }
        requestedLanguagePreference = preference
        let tags = try JSON(Locale.preferredLanguages).encoded()
        guard let pointer = tags.withCString({ nativeHandle(capy_apple_language_request(nativeValue(handle), $0)) }) else { return }
        let job = LanguageJob(pointer)
        languageWorker.async { [self, job] in
            capy_language_prepare(nativeValue(job.handle))
            queue.async { [self, job] in
                if capy_apple_language_prepared(nativeValue(handle),nativeValue(job.handle)) {
                    languageReady = true
                    publishLanguageWhenIdle()
                }
            }
        }
    }
    private func publishLanguageWhenIdle() {
        guard languageReady, !languagePollScheduled else { return }
        languagePollScheduled = true
        DispatchQueue.main.async { [self] in
            let busy = NativeTextContext.inputBusy || languageInputBusy?() == true
            queue.async { [self] in
                languagePollScheduled = false
                let result = capy_apple_language_publish(nativeValue(handle), busy)
                if result != 0 { languageReady = false }
                if result == 1 {
                    do { try publish() } catch { receive(nil, error.localizedDescription) }
                } else if languageReady {
                    queue.asyncAfter(deadline: .now() + .milliseconds(50)) { [self] in publishLanguageWhenIdle() }
                }
            }
        }
    }
    func filterPreviews(_ query: JSON, completion: @escaping @Sendable (FilterPreviewReply) -> Void) {
        queue.async { [self] in
            do {
                let status = try request(2, query) ?? JSON()
                let pointer = nativeHandle(capy_apple_take_filter_previews(nativeValue(handle)))
                if let error = takeNativeText(capy_apple_error(nativeValue(handle))) { throw HostFailure(message: error) }
                completion(FilterPreviewReply(status: status, atlas: pointer.map(NativeFilterPreviews.init), error: nil))
            } catch { completion(FilterPreviewReply(status: JSON(), atlas: nil, error: error.localizedDescription)) }
        }
    }
    func glassRegions(_ value: JSON) {
        perform { [self] in
            let source = try value.encoded()
            try check(source.withCString { capy_apple_glass_regions(nativeValue(handle), $0) })
            receive(nil, nil)
        }
    }
    func navigatorPlacements(_ value: JSON) {
        perform { [self] in
            let source = try value.encoded()
            try check(source.withCString { capy_apple_navigator_placements(nativeValue(handle), $0) })
            // Layout can settle after the canvas driver has gone idle.
            receive(nil, nil)
        }
    }
    /// The shared workspace controller runs on the drawing owner; its SQLite
    /// requests run on the Rust storage worker.
    func workspace(_ value: JSON, completion: @escaping @Sendable (JSON?, String?) -> Void) {
        queue.async { [self] in
            if value["type"].string == "start" { workspaceStart = value }
            do {
                if value["type"].string == "retry", let workspaceStart { _ = try request(6, workspaceStart) }
                let reply = try request(6, value)
                do { try publish() } catch { receive(nil, error.localizedDescription) }
                completion(reply, nil)
            } catch { completion(nil, error.localizedDescription) }
        }
    }
    private func persist(_ snapshot: JSON) throws {
        guard !snapshot["state"].isNull else { return }
        currentSettings = snapshot["state"]["settings"]
        currentLanguagePreference = currentSettings["language"].stableKey
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
                if let decision = closeDecision { try check(capy_apple_document_close(nativeValue(handle), UInt32(id), decision)) }
                else { try check(capy_apple_document_complete(nativeValue(handle), UInt32(id), succeeded == true ? 1 : 0)) }
                try publish(); completion(nil)
            } catch { completion(error.localizedDescription) }
        }
    }
    func hasClip(nonce: String, completion: @escaping @Sendable (Bool) -> Void) {
        queue.async { [self] in
            let current = takeNativeText(capy_apple_clip_nonce(nativeValue(handle)))

            completion(current.map { $0 == nonce } ?? false)
        }
    }
    func paperTask(id: UInt32, epoch: UInt64, directory: URL,
        completion: @escaping @Sendable (NativePaperTask?, String?) -> Void) {
        queue.async { [self] in
            guard let pointer = directory.path.withCString({ nativeHandle(capy_apple_paper_task(nativeValue(handle),id,epoch,$0)) }) else {
                let error = takeNativeText(capy_apple_error(nativeValue(handle))) ?? documentDeliveryCopy["change_in_progress"].string
                if id != 0 { _ = try? request(0,JSON(["type":"complete_request","id":id,"error":error])) }
                try? publish(); completion(nil,error); return
            }
            completion(NativePaperTask(pointer),nil)
        }
    }
    func paperImport(directory: URL, completion: @escaping @Sendable (NativePaperTask?, String?) -> Void) {
        queue.async { [self] in
            guard let pointer = directory.path.withCString({ nativeHandle(capy_apple_paper_import(nativeValue(handle), $0)) }) else {
                completion(nil, takeNativeText(capy_apple_error(nativeValue(handle))) ?? documentDeliveryCopy["change_in_progress"].string); return
            }
            completion(NativePaperTask(pointer), nil)
        }
    }
    func runPaperTask(_ job: NativePaperTask, url: URL?, completion: @escaping @Sendable (String?) -> Void) {
        NativeProjectTask.io.async { [self] in
            job.work(url)
            queue.async { [self] in
                let error = takeNativeText(capy_paper_error(nativeValue(job.handle)))

                do {
                    try check(capy_apple_paper_apply(nativeValue(handle),nativeValue(job.handle))); try publish()
                    completion(error)
                } catch { completion(error.localizedDescription) }
            }
        }
    }
    func paperPreviews(ids: [String], completion: @escaping @Sendable (NativePaperPreviews?) -> Void) {
        guard let text = try? JSON(ids).encoded() else { completion(nil); return }
        queue.async { [self] in
            completion(text.withCString { nativeHandle(capy_apple_paper_previews(nativeValue(handle),$0)) }.map { NativePaperPreviews($0,ids:ids) })
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
            let ready = kind == .lookup || kind == .clip ? 0 : kind == .save ? capy_apple_prepare_recovery(nativeValue(handle), FrameTrace.now()) : capy_apple_project_ready(nativeValue(handle))
            if ready == 1 {
                if DispatchTime.now() < deadline { queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: poll) }
                else { completion(nil, "Document preparation timed out") }
                return
            }
            if ready < 0 { completion(nil, takeNativeText(capy_apple_error(nativeValue(handle))) ?? "Document is unavailable"); return }
            let pointer: UInt64?
            if let placementText { pointer = placementText.withCString { nativeHandle(capy_apple_project_task(nativeValue(handle), kind.rawValue, $0)) } }
            else { pointer = nativeHandle(capy_apple_project_task(nativeValue(handle), kind.rawValue, nil)) }
            guard let pointer else {
                completion(nil, takeNativeText(capy_apple_error(nativeValue(handle))) ?? "Document is unavailable")
                return
            }
            let task = NativeProjectTask(pointer)
            if let expected, capy_project_matches(nativeValue(task.handle), expected.0, expected.1) == 0 {
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
                        try check(capy_apple_project_candidate(nativeValue(handle),nativeValue(task.handle)))
                        NativeProjectTask.io.async {
                            do { try task.compare(); completion(nil) }
                            catch { completion(error.localizedDescription) }
                        }
                    } catch { completion(error.localizedDescription) }
                }
            } catch { completion(error.localizedDescription) }
        }
    }
    func restoreSession(sessions: URL, scene: String, adopt: Bool, retry: Bool = false,
        completion: @escaping @Sendable (String?, Bool, [JSON]) -> Void) {
        queue.async { [self] in
            let opened = sessions.path.withCString { sessions in
                scene.withCString { nativeHandle(capy_apple_session_open(nativeValue(handle), sessions, $0, adopt, retry)) }
            }
            guard let pointer = opened else {
                launchSessionPending = false
                completion(takeNativeText(capy_apple_error(nativeValue(handle))) ?? documentDeliveryCopy["open_operation"].string, false, []); return
            }
            let task = SessionJob(pointer, failure: documentDeliveryCopy["change_in_progress"].string, access: ProjectFileAccess(state: persistence.locations?.state))
            NativeProjectTask.io.async { [self] in
                if let failure = task.work() { queue.async { [self] in launchSessionPending = false; completion(failure, false, []) }; return }
                queue.async { [self] in
                    do { try check(capy_apple_session_adopt(nativeValue(handle),nativeValue(task.handle))) }
                    catch { launchSessionPending = false; completion(error.localizedDescription, false, []); return }
                    do { try publish() } catch { receive(nil, error.localizedDescription) }
                    NativeProjectTask.io.async { [self] in
                        _ = capy_session_restore_finished(nativeValue(task.handle))
                        let failure = task.diagnostic
                        let failures = task.failures
                        queue.async { [self] in
                            launchSessionPending = false
                            completion(failure, true, failures)
                        }
                    }
                }
            }
        }
    }
    func checkpointSession(exclusion: UInt64 = 0, cleanExit: Bool = false, waits: Bool,
        completion: @escaping @Sendable (String?, Bool) -> Void) {
        let deadline = DispatchTime.now() + .seconds(30)
        @Sendable func poll() {
            if let pendingClose, capy_apple_document_close_validate(nativeValue(handle),nativeValue(pendingClose.handle)) < 0 {
                completion(takeNativeText(capy_apple_error(nativeValue(handle))) ?? documentDeliveryCopy["selected_changed"].string, false); return
            }
            let ready = capy_apple_prepare_recovery(nativeValue(handle), FrameTrace.now())
            if ready == 1 {
                if !waits { completion(nil, false) }
                else if DispatchTime.now() < deadline { queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: poll) }
                else { completion(documentDeliveryCopy["change_in_progress"].string, false) }
                return
            }
            guard ready == 0, let pointer = nativeHandle(capy_apple_session_capture(nativeValue(handle), exclusion, cleanExit)) else {
                completion(takeNativeText(capy_apple_error(nativeValue(handle))) ?? documentDeliveryCopy["change_in_progress"].string, false); return
            }
            let task = SessionJob(pointer, failure: documentDeliveryCopy["change_in_progress"].string, access: ProjectFileAccess(state: persistence.locations?.state))
            NativeProjectTask.io.async { let failure = task.work(); completion(failure ?? task.diagnostic, capy_session_committed(nativeValue(task.handle))) }
        }
        queue.async(execute: poll)
    }
    /// Poll only while document/export shaders prepare. GPU synchronization and
    /// pixel packing happen later on the file worker through the returned job.
    func exportTask(id: UInt64, completion: @escaping @Sendable (NativeProjectTask?, String?) -> Void) {
        let deadline = DispatchTime.now() + .seconds(30)
        @Sendable func poll() {
            var pointer: UInt64 = 0
            let ready = capy_apple_project_ready(nativeValue(handle))
            let result = ready == 1 ? 0 : capy_apple_export_task(nativeValue(handle), UInt32(id), FrameTrace.now(), &pointer)
            if result < 0 { completion(nil, takeNativeText(capy_apple_error(nativeValue(handle))) ?? "Export failed") }
            else if let pointer = nativeHandle(pointer) { completion(NativeProjectTask(pointer), nil) }
            else if DispatchTime.now() >= deadline { completion(nil, "The canvas is not ready to export") }
            else { queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: poll) }
        }
        queue.async(execute: poll)
    }
    func finishProject(_ task: NativeProjectTask, opening: Bool, title: String, url: URL?,
        completion: @escaping @Sendable (String?) -> Void) {
        let deadline = DispatchTime.now() + .seconds(15)
        @Sendable func attempt() {
            do {
                let ready = opening ? capy_apple_project_prepare_adopt(nativeValue(handle),nativeValue(task.handle), FrameTrace.now()) : 0
                if ready == 1 {
                    guard DispatchTime.now() < deadline else { throw HostFailure(message: "Drawing capture did not finish") }
                    queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: attempt); return
                }
                try check(ready)
                try title.withCString { name in
                    try (url?.absoluteString ?? "").withCString { uri in
                        try check(opening ? capy_apple_project_adopt(nativeValue(handle),nativeValue(task.handle), name, uri)
                            : capy_apple_project_saved(nativeValue(handle),nativeValue(task.handle), name, uri))
                    }
                }
                try publish()
                if let url {
                    let access = ProjectFileAccess(state: persistence.locations?.state)
                    NativeProjectTask.io.async { try? access.remember(url); completion(nil) }
                } else { completion(nil) }
                if opening { documentStorage() }
            } catch { completion(error.localizedDescription) }
        }
        queue.async(execute: attempt)
    }
    func savedDestination(_ uri: String, completion: @escaping @Sendable (URL?) -> Void) {
        let access = ProjectFileAccess(state: persistence.locations?.state)
        NativeProjectTask.io.async { completion(try? access.resolve(uri)) }
    }
    private final class DocumentJob: @unchecked Sendable {
        let handle: UInt64
        init(_ handle: UInt64) { self.handle = handle }
        deinit { let pointer = handle; NativeProjectTask.io.async { capy_document_free(nativeValue(pointer)) } }
    }
    func switchDocument(_ id: UInt64, closing: Bool = false,
        removing: (@Sendable (@escaping @Sendable (Bool) -> Void) -> Void)? = nil,
        completion: @escaping @Sendable (String?) -> Void) {
        let deadline = DispatchTime.now() + .seconds(15)
        @Sendable func attempt() {
            do {
                let ready = capy_apple_document_prepare_switch(nativeValue(handle), FrameTrace.now())
                if ready == 1 {
                    guard DispatchTime.now() < deadline else { throw HostFailure(message: "Drawing capture did not finish") }
                    queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: attempt); return
                }
                try check(ready)
                guard let task = nativeHandle(capy_apple_document_switch(nativeValue(handle), id, closing)) else { try check(-1); return }
                let job = DocumentJob(task)
                if closing {
                    guard let removing else {
                        try check(capy_apple_document_close_commit(nativeValue(handle),nativeValue(task), false))
                        completion(documentDeliveryCopy["close_operation"].string); return
                    }
                    pendingClose = job
                    removing { [self, job] accepted in queue.async { [self, job] in
                        do {
                            pendingClose = nil
                            try check(capy_apple_document_close_commit(nativeValue(handle),nativeValue(job.handle), accepted))
                            try publish()
                            if accepted { runDocumentJob(job, completion: completion) }
                            else { completion(documentDeliveryCopy["close_operation"].string) }
                        } catch { completion(error.localizedDescription) }
                    } }
                } else { runDocumentJob(job, completion: completion) }
            } catch { completion(error.localizedDescription) }
        }
        queue.async(execute: attempt)
    }
    private func documentStorage() {
        guard let task = nativeHandle(capy_apple_document_storage(nativeValue(handle))) else { return }
        runDocumentJob(DocumentJob(task)) { [weak self] error in if let error { self?.receive(nil, error) } }
    }
    private func runDocumentJob(_ job: DocumentJob, completion: @escaping @Sendable (String?) -> Void) {
        NativeProjectTask.io.async { [self, job] in
            _ = capy_document_prepare(nativeValue(job.handle))
            queue.async { [self, job] in
                do { try check(capy_apple_document_resume(nativeValue(handle),nativeValue(job.handle))); try publish(); completion(nil) }
                catch { try? publish(); completion(error.localizedDescription) }
            }
        }
    }
    func proofTask(id: UInt64, recipe: JSON?, completion: @escaping @Sendable (NativeProjectTask?, String?) -> Void) {
        let text: String
        do { text = try (recipe ?? JSON()).encoded() }
        catch { completion(nil, error.localizedDescription); return }
        queue.async { [self] in
            let pointer = text.withCString { nativeHandle(capy_apple_proof_task(nativeValue(handle), UInt32(id), $0)) }
            completion(pointer.map(NativeProjectTask.init), pointer == nil
                ? takeNativeText(capy_apple_error(nativeValue(handle))) ?? "Proof is unavailable" : nil)
        }
    }
    func strokeRecordingData(completion: @escaping @Sendable (Result<Data, Error>) -> Void) {
        queue.async { [self] in
            guard let bytes = nativeHandle(capy_apple_stroke_recording_data(nativeValue(handle))) else {
                completion(.failure(HostFailure(message: takeNativeText(capy_apple_error(nativeValue(handle))) ?? "The recording is unavailable")))
                return
            }
            NativeProjectTask.io.async {
                defer { capy_bytes_free(bytes) }
                if let data = copyNativeData(capy_bytes_length(bytes), copy: { capy_bytes_copy(bytes, $0, $1, $2) }) {
                    completion(.success(data))
                } else { completion(.failure(HostFailure(message: "The recording is unavailable"))) }
            }
        }
    }
    func checkProof(_ task: NativeProjectTask, completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do { try check(capy_apple_proof_check(nativeValue(handle),nativeValue(task.handle))); completion(nil) }
            catch { completion(error.localizedDescription) }
        }
    }
    func finishProof(_ task: NativeProjectTask, preserved: Bool = false, failure: String? = nil,
        completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do {
                if let failure { try failure.withCString { try check(capy_apple_proof_failed(nativeValue(handle),nativeValue(task.handle), $0)) } }
                else { try check(capy_apple_proof_apply(nativeValue(handle),nativeValue(task.handle), preserved)) }
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
        storageError = nil
        if let settings = try? JSONSerialization.jsonObject(with: change.data),
            NSDictionary(dictionary: currentSettings.object).isEqual(settings) { return }
        _ = try request(0, JSON(["type": "restore_saved_settings", "saved": String(decoding: change.data, as: UTF8.self)]))
    }
    private func reportStorage() {
        let status = JSON(["pending": settingsWrites, "error": (preparationError ?? storageError) as Any? ?? NSNull(),
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
            let result = persistence.locations == nil ? 0 : capy_apple_prepare_recovery(nativeValue(handle), FrameTrace.now())
            preparationError = nil
            if result < 0 {
                do { try check(result) } catch { preparationError = error.localizedDescription }
            }
            do { try publish() } catch { receive(nil, error.localizedDescription) }
            if result == 1 && DispatchTime.now() < deadline {
                queue.asyncAfter(deadline: .now() + .milliseconds(16), execute: poll); return
            }
            if result == 1 { preparationError = documentDeliveryCopy["change_in_progress"].string }
            reportStorage()
            persistence.flush { [self] in queue.async { [self] in completion(result == 0 && !failedSettingsWrite) } }
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
                do { try publish() } catch { receive(nil, error.localizedDescription) }
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
            (previous as? ObservedMetalLayer)?.frameRate = nil
            defer { withExtendedLifetime(previous) {}; try? publish() }
            self.layer = lease.value
            surfaceSize = (width, height, scale)
            try attachCurrentLayer()
            startGpuHealthChecks()
            #if DEBUG
            surfaceSized = true
            #endif
            try applyInitialActions()
        }
    }
    private func attachCurrentLayer() throws {
        guard handle != nil else { throw HostFailure(message: launchFailure ?? "Native session is unavailable") }
        guard let layer, let size = surfaceSize else { throw HostFailure(message: "The canvas has no presentation surface") }
        let surface = Unmanaged.passUnretained(layer).toOpaque()
        (layer as? ObservedMetalLayer)?.frameRate = nil
        try check(capy_apple_attach(nativeValue(handle), surface, size.width, size.height, size.scale))
        (layer as? ObservedMetalLayer)?.frameRate = CanvasFrameRateObserver(nativeValue(handle))
    }
    private func startGpuHealthChecks() {
        guard gpuHealth == nil else { return }
        let timer = DispatchSource.makeTimerSource(queue: DispatchQueue.global(qos: .userInteractive))
        timer.schedule(deadline: .now() + 1, repeating: hdrDocument ? 0.2 : 0.25, leeway: .milliseconds(50))
        timer.setEventHandler { [weak self] in
            self?.queue.async { [weak self] in
            guard let self else { return }
            // EDR headroom can change while the display link is asleep. Poll
            // only HDR documents, using the existing foreground health timer.
            if hdrDocument { receive(JSON(["display_poll": true]), nil) }
            // Failure callbacks can arrive after the display link goes idle.
            // Healthy checks do not publish UI state or submit canvas work.
            let status = capy_apple_poll_renderer(nativeValue(handle))
            if status != 0 {
                do { try check(status); try publish() }
                catch { try? publish(); receive(nil, error.localizedDescription) }
            }
            }
        }
        gpuHealth = timer; timer.resume()
    }
    #if DEBUG
    func testGpuFault(validation: Bool) {
        guard ProcessInfo.processInfo.environment["CAPY_GPU_RECOVERY_TEST"] == "1" else { return }
        perform { [self] in try check(capy_apple_test_gpu_fault(nativeValue(handle), validation ? 1 : 0)) }
    }
    #endif
    func restartCanvas(_ completion: @escaping @Sendable (String?) -> Void) {
        queue.async { [self] in
            do {
                if handle == nil { try launch() }
                try check(capy_apple_suspend_renderer(nativeValue(handle)))
                try publish()
                try attachCurrentLayer()
                startGpuHealthChecks()
                try publish(); completion(nil)
            } catch {
                try? publish(); completion(error.localizedDescription)
            }
        }
    }
    private let shaderInputPending = NativeInputGate()
    func shaderInput() {
        guard shaderInputPending.claim() else { return }
        queue.async { [self] in
            shaderInputPending.release()
            capy_apple_shader_input(nativeValue(handle))
        }
    }
    func resize(width: UInt32, height: UInt32, scale: Float) {
        perform { [self] in
            try check(capy_apple_resize(nativeValue(handle), width, height, scale))
            surfaceSize = (width, height, scale)
            #if DEBUG
            surfaceSized = true
            #endif
            try applyInitialActions(); try publish()
        }
    }
    private func applyInitialActions() throws {
        #if DEBUG
        guard workspaceInitialized && surfaceSized && canvasReady && !launchSessionPending else { return }
        if initialActions.contains(where: { $0["type"].string == "workspace_manager" }) {
            // Workspace fixture commands have the same idle requirement as
            // their UI entries.
            guard canvasReady && shadersReady,
                try request(6, JSON(["type": "tick"]))?["view"]["busy"].bool == false else { return }
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
            (layer as? ObservedMetalLayer)?.frameRate = nil
            try check(capy_apple_detach(nativeValue(handle)))
            gpuHealth?.cancel(); gpuHealth = nil
            layer = nil; surfaceSize = nil
        }
    }
    private func registerPenDevice(_ device: NativePenDevice) throws {
        guard penDevice != device else { return }
        _ = try request(0, device.action())
        penDevice = device
    }
    func penInput(_ device: NativePenDevice, sample: [Double]? = nil) {
        perform { [self] in
            guard !launchSessionPending else { return }
            if let sample {
                _ = try request(0, device.action(sample: sample)); penDevice = device
            } else { try registerPenDevice(device) }
        }
    }
    func pointer(id: UInt64, tool: UInt32, button: UInt32, records: [Double], predicted: Bool, revision: UInt64,
                 updates: [UInt64] = [], correction: Bool = false, deviceFlags: UInt32 = 0, device: NativePenDevice? = nil) {
        precondition(updates.isEmpty || updates.count == records.count / 9 * 2)
        let observation = trace.flatMap { $0.isRecording ? $0 : nil }
        let queued = observation == nil ? 0 : FrameTrace.now()
        perform { [self] in
            guard !launchSessionPending else { return }
            if let device { try registerPenDevice(device) }
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
                    try check(capy_apple_pointer(nativeValue(handle), id, tool, button, $0.baseAddress, $0.count, predicted ? 1 : 0, revision, deviceFlags))
                } else {
                    let samples = $0
                    try updates.withUnsafeBufferPointer {
                        try check(capy_apple_pointer_updates(nativeValue(handle), id, tool, button, samples.baseAddress, samples.count,
                            $0.baseAddress, correction ? 1 : 0, revision, deviceFlags))
                    }
                }
            }
            succeeded = true
            if !predicted { syncCanvasBarHold() }
        }
    }
    private var canvasBarHold: UInt32 = 0
    private var canvasBarHoldChanged: (@Sendable (UInt32) -> Void)?
    func setCanvasBarHoldChanged(_ receive: @escaping @Sendable (UInt32) -> Void) {
        perform { [self] in canvasBarHoldChanged = receive }
    }
    private func syncCanvasBarHold() {
        let hold = capy_apple_canvas_bar_hold(nativeValue(handle))
        guard hold != canvasBarHold else { return }
        canvasBarHold = hold
        canvasBarHoldChanged?(hold)
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
            try check(capy_apple_scroll(nativeValue(handle), x, y, dx, dy, scale, zoom ? 1 : 0, horizontal ? 1 : 0))
            try publish()
        }
    }
    func gesture(x: Float, y: Float, scale: Float, rotation: Float, began: Bool) {
        perform { [self] in
            try check(capy_apple_gesture(nativeValue(handle), x, y, scale, rotation, began ? 1 : 0))
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
        let count = capy_apple_take_gpu_timing(nativeValue(handle), &gpuSamples, capacity, &status)
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
            guard handle != nil else { completion(false, 0, costs); return }
            let start = observation == nil ? 0 : FrameTrace.now()
            var encodedCameraRevision: UInt64 = 0, frameOutcome: UInt64 = 0
            (layer as? ObservedMetalLayer)?.observation = observation.map { ($0, now) }
            defer {
                (layer as? ObservedMetalLayer)?.observation = nil
                observation?.record(FrameTraceEvent(kind: .frame, a: now, b: target, c: start, d: FrameTrace.now(),
                    e: costs[0], f: costs[1], g: costs[2], h: costs[3], i: costs[4], j: latestTracedInput))
                if let observation {
                    observation.record(FrameTraceEvent(kind: .consumedPaint, a: now,
                        b: capy_apple_consumed_paint_ns(nativeValue(handle)), c: capy_apple_submitted_canvas_updates(nativeValue(handle)),
                        d: encodedCameraRevision, e: frameOutcome))
                }
                if let trace { collectGpuTiming(trace) }
            }
            do {
                let wantsGpuTiming = observation?.recordsGpuTiming == true
                if gpuTimingEnabled != wantsGpuTiming {
                    try check(capy_apple_gpu_timing(nativeValue(handle), wantsGpuTiming ? 1 : 0))
                    gpuTimingEnabled = wantsGpuTiming
                }
                if wantsGpuTiming, let observation { sampleGpuClock(observation) }
                let result = capy_apple_frame(nativeValue(handle), now, max(now, target), &costs)
                if observation != nil && (result == 0 || result == 1) {
                    encodedCameraRevision = capy_apple_camera_revision(nativeValue(handle))
                    frameOutcome = UInt64(result) + 1
                }
                try check(result)
                syncCanvasBarHold()
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
                if canvasReady && !shadersReady { try check(capy_apple_finish_startup_catalog(nativeValue(handle))) }
                if let observation {
                    let state: UInt64 = (canvasReady ? 1 : 0) | (catalogLoaded ? 2 : 0) | (result == 1 ? 4 : 0) | (shadersReady ? 8 : 0)
                    if state != lastTraceState {
                        observation.record(FrameTraceEvent(kind: .state, a: FrameTrace.now(), b: now, c: state))
                        lastTraceState = state
                    }
                }
                completion(result == 1, capy_apple_camera_revision(nativeValue(handle)), costs)
            } catch {
                observation?.record(FrameTraceEvent(kind: .state, a: FrameTrace.now(), b: now, d: 1))
                try? publish()
                receive(nil, error.localizedDescription)
                completion(false, capy_apple_camera_revision(nativeValue(handle)), costs)
            }
        }
    }
}
