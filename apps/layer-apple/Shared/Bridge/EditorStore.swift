import SwiftUI
import Darwin

@MainActor final class CameraReadout: ObservableObject {
    @Published var value = JSON()
}

@MainActor final class EditorStore: ObservableObject {
    private let ui = EditorSnapshotState()
    private(set) var colorPreferences = ColorPreferencesStore(locations: nil)
    let camera = CameraReadout()
    @Published var displayDetails = DisplayDetails()
    @Published var displayHeadroom: Double = 1
    @Published var catalog = JSON()
    @Published private(set) var bootstrap = JSON()
    @Published private(set) var interfaceLanguage = ""
    @Published private(set) var languageGeneration: UInt64 = 0
    var bootstrapChanged: (() -> Void)?
    @Published var failure: String?
    private var lastCanvasDiagnostic: String?
    private static let diagnosticEnvironment: String = {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown"
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "unknown"
        #if os(macOS)
        var size = 0
        sysctlbyname("hw.model", nil, &size, nil, 0)
        var model = [UInt8](repeating: 0, count: max(1, size))
        let machine = sysctlbyname("hw.model", &model, &size, nil, 0) == 0
            ? String(decoding: model.prefix { $0 != 0 }, as: UTF8.self) : "unknown"
        #else
        var system = utsname()
        uname(&system)
        let machine = withUnsafeBytes(of: &system.machine) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
        #endif
        return "Capy Canvas \(version) (\(build))\n\(ProcessInfo.processInfo.operatingSystemVersionString)\n\(machine)"
    }()
    var canvasDiagnostic: String { Self.diagnostic(failure ?? snapshot["error"].string) }
    private static func diagnostic(_ error: String) -> String { "\(error)\n\n\(diagnosticEnvironment)" }
    @Published var canvasSubmitted = false
    @Published private(set) var restartingCanvas = false
    @Published var storageFailure: String?
    @Published var storagePending = true
    @Published var canRetryStorage = false
    private static let instances = NSHashTable<EditorStore>.weakObjects()
    var acceptsGamepad: (() -> Bool)?
    static var gamepadTarget: EditorStore? { instances.allObjects.first { $0.acceptsGamepad?() == true } }
    /// Measured native window controls; editor geometry otherwise comes from Rust.
    @Published var headerLeadingInset: CGFloat = 0
    var cameraRevision: UInt64 = 0
    var wake: (() -> Void)?
    var observeDisplayHeadroom: (() -> Void)?
    var interruptInput: (() -> Void)?
    var focusWindow: (() -> Void)?
    var focusCanvas: (() -> Void)?
    var panCursor = false
    var cursorChanged: (() -> Void)?
    var handCursor: Bool { panCursor || state["layer_tools"]["tool"].string == "hand" }
    var systemSceneID: String?
    let sessionIdentity: String
    private(set) var native: NativeOwner?
    private(set) var workspaces: WorkspaceController?
    private var drawingWorkload: DrawingWorkload?
    lazy var layerThumbnails = LayerThumbnails(store: self)
    let layerSwipe = LayerSwipe()
    lazy var filterPreviews = FilterPreviews(store: self)
    lazy var rendererStats = RendererStats(store: self)
    lazy var workspace = WorkspacePresentation(store: self)
    lazy var header = HeaderPresentation(store: self)
    lazy var panelMeasurements = PanelMeasurements(store: self)
    lazy var contentDrawers = ContentDrawersPresentation(store: self)
    lazy var projectFiles = ProjectFiles(store: self)
    lazy var windowPresentation = WindowPresentation(store: self)
    lazy var drawingTabs = DrawingTabsController(store: self)
    lazy var recovery = ArtworkRecovery(store: self)
    let scopes = ScopePlots()
    lazy var proof = ProofController(store: self)
    lazy var palettes = PaletteController(store: self)
    lazy var colorEditing = ColorEditingController(store: self)
    lazy var strokeRecording = StrokeRecording(store: self)
    let canvasBar = CanvasBarPresence()
    let notice = CanvasNoticePresence()
    lazy var glass = GlassRegistry(store: self)
    var snapshot: SnapshotProjection { ui.snapshot }
    var state: SnapshotProjection { ui.state }
    var displayColors: JSON {
        let mask = state["layer_tools"]["mask_editing"]["colors"]
        return mask.isNull ? state["colors"] : mask
    }
    var colorPreviewing: Bool { !snapshot["color_preview"]["picker"]["preview"].isNull }
    var colorPanel: JSON { colorPreviewing ? snapshot["color_preview"]["view"] : snapshot["color_panel"] }
    var panelColors: JSON { colorPreviewing ? snapshot["color_preview"]["colors"] : displayColors }
    var colorViewing: JSON {
        JSON(["document_space": displayColors["rgb_space"].raw, "recipe": snapshot["proof_panel"]["recipe"].raw,
              "document_depth": state["layer_tools"]["mask_editing"].isNull ? snapshot["proof_panel"]["depth"].raw : displayColors["hdr_depth"].raw, "headroom": displayHeadroom, "hdr": snapshot["color_panel"]["hdr"].bool])
    }
    var paintPair: JSON { snapshot["paint_pair"] }
    var workspaceMotion: WorkspaceMotion { ui.workspace }

    init(platform: UInt32, scene: String = UUID().uuidString, persistence: EditorPersistence = .shared,
        managedWorkspaces: Bool = true) {
        sessionIdentity = UUID(uuidString: scene)?.uuidString ?? UUID().uuidString
        do {
            let workload = try DrawingWorkloadPlan.configured()
            // Performance runs never read or replace the artist's preferences
            // or recovery copies. Keep ordinary persistence costs in the run.
            let storage = workload == nil ? persistence : EditorPersistence(root:
                FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                    .appendingPathComponent("CapyPerformanceSessions/\(UUID().uuidString)", isDirectory: true))
            colorPreferences = ColorPreferencesStore(locations: storage.locations)
            let usesWorkspaceLibrary = managedWorkspaces && storage.locations != nil
            native = try NativeOwner(platform: platform, persistence: storage,
                traceDuration: workload.map { $0.seconds + 140 }, workload: workload?.metadata,
                managedWorkspaces: usesWorkspaceLibrary) { [weak self] snapshot, failure in
                DispatchQueue.main.async { self?.receive(snapshot, failure) }
            }
            native?.languageInputBusy = { [weak self] in self?.workspace.languageInputBusy == true }
            native?.scopesReceived = { [weak self] update in DispatchQueue.main.async { self?.scopes.receive(update) } }
            native?.submit(2, JSON(["type": "catalog"])) { [weak self] result in
                DispatchQueue.main.async {
                    self?.catalog = result ?? JSON()
                    self?.canvasBar.delay = (result?["canvas_bar_reappear_ms"].number ?? 0) / 1000
                }
            }
            if usesWorkspaceLibrary, let locations = storage.locations {
                workspaces = WorkspaceController(store: self, directory: locations.workspaces, scene: scene)
            }
            notice.answer = { [weak self] id, accept, action in self?.dispatch(["type": "notice", "id": id, "accept": accept, "action": action ?? NSNull()]) }
            native?.canvasBarHoldChanged = { [weak self] hold in
                DispatchQueue.main.async { self?.canvasBar.hold(hold) }
            }
            if let workload { drawingWorkload = DrawingWorkload(store: self, plan: workload) }
        } catch { failure = error.localizedDescription }
        Self.instances.add(self)
    }
    private func receive(_ next: JSON?, _ error: String?) {
        if next?["display_poll"].bool == true { observeDisplayHeadroom?(); return }
        if let error {
            if failure != error { NSLog("Capy Canvas native diagnostic: %@", Self.diagnostic(error)) }
            failure = error
        }
        if let next {
            if !next["bootstrap"].isNull {
                bootstrap = next["bootstrap"]; interfaceLanguage = bootstrap["active_tag"].string
                colorPreferences.setLanguage(interfaceLanguage)
                if !next["catalog"].isNull {
                    catalog = next["catalog"]
                    canvasBar.delay = catalog["canvas_bar_reappear_ms"].number / 1000
                }
                languageGeneration = next["language_generation"].uint
                if !next["workspace_view"].isNull { workspaces?.receiveLanguage(next["workspace_view"]) }
                palettes.refreshLanguage()
                bootstrapChanged?()
                if next["state"].isNull { wake?(); return }
            }
            if !next["persistence"].isNull {
                storagePending = next["persistence"]["pending"].uint != 0
                canRetryStorage = next["persistence"]["can_retry"].bool
                storageFailure = next["persistence"]["error"].isNull ? nil : next["persistence"]["error"].string
                return
            }
            if !next["error"].isNull && lastCanvasDiagnostic != next["error"].string {
                lastCanvasDiagnostic = next["error"].string
                NSLog("Capy Canvas renderer diagnostic: %@", Self.diagnostic(next["error"].string))
            }
            let hadRenderer = snapshot["gpu_ready"].bool
            let documentEpoch = state["document_file"]["epoch"].uint, hand = handCursor
            switch ui.receive(next) {
            case .full:
                if hand != handCursor { cursorChanged?() }
                // Replacing a document retires its GPU readbacks even when
                // the new renderer is already ready in the same publication.
                if hadRenderer != snapshot["gpu_ready"].bool || documentEpoch != state["document_file"]["epoch"].uint {
                    layerThumbnails.reset()
                }
                if hadRenderer != snapshot["gpu_ready"].bool {
                    filterPreviews.reset()
                    if !snapshot["gpu_ready"].bool { canvasSubmitted = false }
                    bootstrapChanged?()
                }
                filterPreviews.refresh()
                if !SnapshotProjection.equal(camera.value.raw, state["camera"].raw) { camera.value = state["camera"] }
                proof.receive(state.json, gpuReady: snapshot["gpu_ready"].bool)
                drawingTabs.receive()
                colorEditing.receive(state["color_picker"])
                projectFiles.receive(state.json)
                windowPresentation.receive(state.json)
                contentDrawers.refresh()
                workspace.refresh()
                if state["requests"].array.contains(where: { $0["kind"]["type"].string == "workspace" }) {
                    workspaces?.tick()
                }
            case .reflow:
                if !SnapshotProjection.equal(camera.value.raw, state["camera"].raw) { camera.value = state["camera"] }
                contentDrawers.refresh()
                workspace.refresh()
            case .search: break
            case .workspace, .camera:
                // Camera patches update the readout alone; dragging the canvas
                // must not rebuild every panel and brush preview at input rate.
                if !next["camera"].isNull && !SnapshotProjection.equal(camera.value.raw, next["camera"].raw) {
                    camera.value = next["camera"]
                }
            case .ignored: break
            }
            cameraRevision = state["camera"]["revision"].uint
            notice.publish(state["notice"])
            recovery.observe()
        }
        wake?()
    }
    func restartCanvas() {
        guard !restartingCanvas, let native else { return }
        interruptInput?()
        restartingCanvas = true
        canvasSubmitted = false
        bootstrapChanged?()
        native.restartCanvas { [weak self] error in
            DispatchQueue.main.async {
                guard let self else { return }
                self.restartingCanvas = false
                self.failure = error
                self.bootstrapChanged?()
                self.wake?()
            }
        }
    }
    func flushPersistence(cleanExit: Bool = false, _ completion: @escaping @MainActor (Bool) -> Void) {
        guard let native else { completion(false); return }
        // Scene teardown may release its view while this barrier is in flight.
        // Keep the document owner through the final recovery acknowledgement.
        native.flushPersistence { [self] succeeded in DispatchQueue.main.async {
            Task { @MainActor in
                let finish = { [self] (stored: Bool) in
                    self.recovery.flush(cleanExit: cleanExit) { [self] result in
                        completion(succeeded && stored && result); withExtendedLifetime(self) {}
                    }
                }
                if let workspaces = self.workspaces { workspaces.flush(finish) } else { finish(true) }
            }
        } }
    }
    static func flushAll(_ completion: @escaping @MainActor (Bool) -> Void) {
        let stores = instances.allObjects
        guard !stores.isEmpty else { completion(true); return }
        var remaining = stores.count, succeeded = true
        for store in stores {
            store.flushPersistence(cleanExit: true) { result in
                succeeded = succeeded && result; remaining -= 1
                if remaining == 0 { completion(succeeded) }
            }
        }
    }
    static func workspaceOwner(_ owner: String) -> EditorStore? {
        instances.allObjects.first { $0.workspaces?.ready == true && $0.workspaces?.view["owner"].string == owner }
    }
    static func suspendWorkspaces() {
        for store in instances.allObjects {
            store.input(["type": "blur"])
            store.workspaces?.suspend()
            store.flushPersistence { _ in }
        }
    }
    static func resumeWorkspaces() {
        for store in instances.allObjects {
            store.workspaces?.resume()
            store.wake?()
        }
    }
    static func discardSceneSessions(_ identifiers: Set<String>) {
        for store in instances.allObjects where store.systemSceneID.map(identifiers.contains) == true {
            store.workspaces?.close { saved in if !saved { store.workspaces?.detach() } }
        }
    }
    static func detachWorkspaceOwners(_ completion: @escaping @MainActor () -> Void) {
        var remaining = instances.allObjects.compactMap(\.workspaces)
        func next() {
            guard let workspaces = remaining.popLast() else { completion(); return }
            workspaces.detach { next() }
        }
        next()
    }
    func prepareClose(_ completion: @escaping @MainActor (Bool) -> Void) {
        flushPersistence { [self] saved in
            guard saved else { completion(false); return }
            let retire: @MainActor (Bool) -> Void = { [self] saved in
                guard saved else { cancelPreparedClose { completion(false) }; return }
                recovery.close { saved in completion(saved) }
            }
            if let workspaces { workspaces.close(retire) } else { retire(true) }
        }
    }
    func cancelPreparedClose(_ completion: @escaping @MainActor () -> Void = {}) {
        query(["type": "document_tabs", "op": "reset_close"]) { _ in }
        recovery.resume()
        workspaces?.keepOpen()
        completion()
    }
    func dispatch(_ action: JSON) {
        if action["type"].string == "invoke" && action["command"].string == "search_commands" { reportCommandFocus() }
        native?.submit(0, action); wake?()
    }
    func commandFocus() -> String {
        #if os(macOS)
        let editingText = NSApp.keyWindow?.firstResponder is NSText
        #else
        let editingText = FocusedResponder.find() is UITextInput
        #endif
        return editingText ? "text" : palettes.focused ? "palette" : "canvas"
    }
    private func reportCommandFocus() {
        native?.submit(0, JSON(["type": "command_search", "action": ["type": "focus", "focus": commandFocus()]]))
    }
    func dispatch(_ value: [String: Any]) { dispatch(JSON(value)) }
    func edit(_ value: [String: Any], completion: @escaping @MainActor (String?) -> Void) {
        guard let native else { completion("The canvas session is unavailable"); return }
        native.edit(JSON(value)) { error in DispatchQueue.main.async { completion(error) } }
        wake?()
    }
    func invoke(_ command: String) { dispatch(["type": "invoke", "command": command]) }
    func layer(_ action: [String: Any]) { dispatch(["type": "layer", "action": action]) }
    func object(_ action: [String: Any]) { dispatch(["type": "object", "action": action]) }
    func customize(_ action: [String: Any]) { dispatch(["type": "customize", "action": action]) }
    func doubleClickHandle(_ item: JSON) {
        query(["type": "panel_handle_target", "item": item.raw]) { [weak self] group in
            guard let self, !group.isNull else { return }
            self.dispatch(["type": "double_click_panel_handle", "group": group.raw,
                "viewport": self.snapshot["layout"]["viewport"].raw])
        }
    }
    func input(_ value: [String: Any]) {
        if value["type"] as? String == "blur" { interruptInput?(); workspace.dismissTransients(at: nil) }
        if value["type"] as? String == "key", let key = value["key"] as? String,
           let modifiers = value["modifiers"] as? [String: Any],
           palettes.key(key, pressed: value["pressed"] as? Bool == true,
               command: modifiers["command"] as? Bool == true, shift: modifiers["shift"] as? Bool == true) { return }
        if value["type"] as? String == "key", value["key"] as? String == "Escape", value["pressed"] as? Bool == true {
            workspace.dismissTransients(at: nil)
        }
        if value["type"] as? String == "key", value["pressed"] as? Bool == true,
           (value["modifiers"] as? [String: Any])?["command"] as? Bool == true { reportCommandFocus() }
        native?.submit(1, JSON(value)) { [weak self] reply in
            guard let pan = reply?["pan_cursor"], !pan.isNull else { return }
            DispatchQueue.main.async {
                guard let self, self.panCursor != pan.bool else { return }
                self.panCursor = pan.bool; self.cursorChanged?()
            }
        }
        wake?()
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
    func headerAction(_ value: [String: Any], completion: @escaping @MainActor () -> Void) {
        guard let native else { completion(); return }
        native.headerAction(JSON(value)) { DispatchQueue.main.async { completion() } }
        wake?()
    }
    func numeric(_ control: JSON, value: Double, operation: [String: Any], completion: @escaping @MainActor (JSON) -> Void) {
        do { completion(try resolveNumber(control, value: value, operation: operation)) }
        catch { failure = error.localizedDescription }
    }
    func resolveNumber(_ control: JSON, value: Double, operation: [String: Any]) throws -> JSON {
        let request = try JSON(["language": interfaceLanguage, "request": ["control": control.raw, "value": value, "operation": operation]]).encoded()
        guard let response = request.withCString({ capy_apple_numeric($0) }) else { throw HostFailure(message: "Numeric input failed") }
        defer { capy_apple_string_free(response) }
        let value = try JSON.decode(String(cString: response))
        if !value["error"].isNull { throw HostFailure(message: value["error"].string) }
        return value
    }
}
