import SwiftUI

@MainActor final class CameraReadout: ObservableObject {
    @Published var value = JSON()
}

@MainActor final class EditorStore: ObservableObject {
    private let ui = EditorSnapshotState()
    let camera = CameraReadout()
    @Published var catalog = JSON()
    @Published var failure: String?
    @Published var canvasSubmitted = false
    @Published var storageFailure: String?
    @Published var storagePending = true
    @Published var canRetryStorage = false
    private static let instances = NSHashTable<EditorStore>.weakObjects()
    /// Measured native window controls; editor geometry otherwise comes from Rust.
    @Published var headerLeadingInset: CGFloat = 0
    var cameraRevision: UInt64 = 0
    var wake: (() -> Void)?
    var interruptInput: (() -> Void)?
    private(set) var native: NativeOwner?
    private var drawingWorkload: DrawingWorkload?
    lazy var layerThumbnails = LayerThumbnails(store: self)
    lazy var filterPreviews = FilterPreviews(store: self)
    lazy var rendererStats = RendererStats(store: self)
    lazy var workspace = WorkspacePresentation(store: self)
    lazy var contentDrawers = ContentDrawersPresentation(store: self)
    lazy var projectFiles = ProjectFiles(store: self)
    lazy var windowPresentation = WindowPresentation(store: self)
    lazy var recovery = ArtworkRecovery(store: self)
    var snapshot: SnapshotProjection { ui.snapshot }
    var state: SnapshotProjection { ui.state }

    init(platform: UInt32, scene: String = UUID().uuidString, persistence: EditorPersistence = .shared) {
        do {
            let workload = try DrawingWorkloadPlan.configured()
            // Performance runs never read or replace the artist's preferences
            // or recovery copies. Keep ordinary persistence costs in the run.
            let storage = workload == nil ? persistence : EditorPersistence(root:
                FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                    .appendingPathComponent("CapyPerformanceSessions/\(UUID().uuidString)", isDirectory: true))
            native = try NativeOwner(platform: platform, scene: scene, persistence: storage,
                traceDuration: workload.map { $0.seconds + 140 }, workload: workload?.metadata) { [weak self] snapshot, failure in
                DispatchQueue.main.async { self?.receive(snapshot, failure) }
            }
            native?.submit(2, JSON(["type": "catalog"])) { [weak self] result in
                DispatchQueue.main.async { self?.catalog = result ?? JSON() }
            }
            if let workload { drawingWorkload = DrawingWorkload(store: self, plan: workload) }
        } catch { failure = error.localizedDescription }
        Self.instances.add(self)
    }
    private func receive(_ next: JSON?, _ error: String?) {
        if let error { failure = error }
        if let next {
            if !next["persistence"].isNull {
                storagePending = next["persistence"]["pending"].uint != 0
                canRetryStorage = next["persistence"]["can_retry"].bool
                storageFailure = next["persistence"]["error"].isNull ? nil : next["persistence"]["error"].string
                return
            }
            if !next["state"].isNull {
                ui.receive(next)
                filterPreviews.refresh()
                if !SnapshotProjection.equal(camera.value.raw, state["camera"].raw) { camera.value = state["camera"] }
                projectFiles.receive(state.json)
                windowPresentation.receive(state.json)
                recovery.observe(state["document_file"])
                contentDrawers.refresh()
                workspace.refresh()
            }
            else if !next["camera"].isNull {
                // Camera patches update the readout alone; dragging the canvas
                // must not rebuild every panel and brush preview at input rate.
                ui.receive(next)
                if !SnapshotProjection.equal(camera.value.raw, next["camera"].raw) { camera.value = next["camera"] }
            }
            cameraRevision = state["camera"]["revision"].uint
        }
        wake?()
    }
    func flushPersistence(_ completion: @escaping @MainActor (Bool) -> Void) {
        guard let native else { completion(false); return }
        native.flushPersistence { [weak self] succeeded in DispatchQueue.main.async {
            guard let self else { completion(false); return }
            self.recovery.flush { completion(succeeded && $0) }
        } }
    }
    static func flushAll(_ completion: @escaping @MainActor (Bool) -> Void) {
        let stores = instances.allObjects
        guard !stores.isEmpty else { completion(true); return }
        var remaining = stores.count, succeeded = true
        for store in stores {
            store.flushPersistence { result in
                succeeded = succeeded && result; remaining -= 1
                if remaining == 0 { completion(succeeded) }
            }
        }
    }
    static func confirmCloseAll(_ completion: @escaping @MainActor (Bool) -> Void) {
        var stores = instances.allObjects
        func next() {
            guard let store = stores.popLast() else { completion(true); return }
            store.projectFiles.confirmClose { allowed in
                if allowed { next() }
                else {
                    resetCloseApprovals()
                    completion(false)
                }
            }
        }
        next()
    }
    static func resetCloseApprovals() {
        for live in instances.allObjects { live.native?.documentRequest(closeDecision: 4) { _ in } }
    }
    static func finishClosingAll(_ completion: @escaping @MainActor () -> Void) {
        let stores = instances.allObjects
        guard !stores.isEmpty else { completion(); return }
        var remaining = stores.count
        for store in stores {
            store.recovery.close { _ in remaining -= 1; if remaining == 0 { completion() } }
        }
    }
    func dispatch(_ action: JSON) { native?.submit(0, action); wake?() }
    func dispatch(_ value: [String: Any]) { dispatch(JSON(value)) }
    func edit(_ value: [String: Any], completion: @escaping @MainActor (String?) -> Void) {
        guard let native else { completion("The canvas session is unavailable"); return }
        native.edit(JSON(value)) { error in DispatchQueue.main.async { completion(error) } }
        wake?()
    }
    func invoke(_ command: String) { dispatch(["type": "invoke", "command": command]) }
    func layer(_ action: [String: Any]) { dispatch(["type": "layer", "action": action]) }
    func importLayer(_ url: URL) { native?.importLayer(url); wake?() }
    func customize(_ action: [String: Any]) { dispatch(["type": "customize", "action": action]) }
    func doubleClickHandle(_ item: JSON) {
        query(["type": "panel_handle_target", "item": item.raw]) { [weak self] group in
            guard let self, !group.isNull else { return }
            self.dispatch(["type": "double_click_panel_handle", "group": group.raw,
                "viewport": self.snapshot["layout"]["viewport"].raw])
        }
    }
    func input(_ value: [String: Any]) {
        if value["type"] as? String == "blur" { interruptInput?() }
        native?.submit(1, JSON(value)); wake?()
    }
    /// A captured chord is a complete input pair; closing its sheet cannot leave
    /// a held key in the canvas interaction state.
    func captureShortcut(key: String, command: Bool, shift: Bool, alt: Bool) {
        guard !snapshot["preferences"]["capture"].isNull else { return }
        for pressed in [true, false] {
            input(["type": "key", "key": key, "pressed": pressed, "repeat": false,
                "modifiers": ["command": command, "shift": shift, "alt": alt]])
        }
    }
    func command(_ id: String) -> JSON { ui.command(id) }
    func panel(_ id: String) -> JSON { ui.panel(id) }
    func applicationMenu(_ id: String) -> JSON { ui.applicationMenu(id) }
    func query(_ value: [String: Any], completion: @escaping @MainActor (JSON) -> Void) {
        guard let native else { completion(JSON()); return }
        native.submit(2, JSON(value)) { result in DispatchQueue.main.async { completion(result ?? JSON()) } }
    }
    func numeric(_ control: JSON, value: Double, operation: [String: Any], completion: @escaping @MainActor (JSON) -> Void) {
        do { completion(try resolveNumber(control, value: value, operation: operation)) }
        catch { failure = error.localizedDescription }
    }
    func resolveNumber(_ control: JSON, value: Double, operation: [String: Any]) throws -> JSON {
        let request = try JSON(["control": control.raw, "value": value, "operation": operation]).encoded()
        guard let response = request.withCString({ capy_apple_numeric($0) }) else { throw HostFailure(message: "Numeric input failed") }
        defer { capy_apple_string_free(response) }
        let value = try JSON.decode(String(cString: response))
        if !value["error"].isNull { throw HostFailure(message: value["error"].string) }
        return value
    }
}
