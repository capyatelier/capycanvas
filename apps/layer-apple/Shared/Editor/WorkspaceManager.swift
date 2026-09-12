import SwiftUI

/// UI routing around shared Rust projections and the serial library coordinator.
/// Selecting a row never adopts it. Only explicit actions change a workspace.
@MainActor final class WorkspaceManager: ObservableObject {
    @Published var presented = false
    @Published var page = "workspaces"
    @Published var query = ""
    @Published var selection: String?
    @Published private(set) var catalog = JSON()
    @Published private(set) var view = JSON()
    @Published private(set) var history = JSON()
    @Published private(set) var historyPreview = JSON()
    @Published private(set) var storage = JSON()
    @Published private(set) var showingStorage = false
    @Published private(set) var processing = false
    @Published private(set) var selecting = false
    @Published private(set) var toolbarMode = false
    @Published var error: String?
    @Published var note: String?
    @Published private(set) var undoDeletion: String?
    @Published private(set) var prompt: JSON?
    @Published var formName = ""
    @Published var formDescription = ""
    @Published var formChoice = ""
    @Published private(set) var formError: String?
    let files: WorkspacePackageFiles
    private weak var store: EditorStore?
    private var formCompletion: ((JSON?) -> Void)?
    private var hostRequest: UInt64?
    private var requestGeneration: UInt64 = 0
    private var historyID = ""
    private var historyMode = "layout"
    private var pendingCustomization: JSON?
    private var selectionTask: Task<Void, Never>?
    private var selectionGeneration: UInt64 = 0
    var library: WorkspaceLibrary? { store?.workspaceLibrary }
    var title: String { catalog[toolbarMode ? "toolbar_title" : "title"].string }
    init(store: EditorStore, files: WorkspacePackageFiles? = nil) {
        self.store = store; self.files = files ?? WorkspacePackageFiles()
    }
    func receive(_ state: JSON) {
        guard hostRequest == nil, !processing, library?.ready == true,
            let request = state["requests"].array.first(where: { $0["kind"]["type"].string == "workspace" }) else { return }
        let id = request["id"].uint; hostRequest = id
        let command = request["kind"]["command"]
        Task { [self] in
            var failure: String?
            do { try await handle(command) }
            catch { failure = error.localizedDescription; self.error = failure; presented = true }
            if let store {
                _ = await withCheckedContinuation { continuation in
                    store.edit(["type": "complete_request", "id": id, "error": failure as Any? ?? NSNull()]) { continuation.resume(returning: $0) }
                }
            }
            hostRequest = nil
            if let store { receive(store.state.json) }
        }
    }
    private func handle(_ command: JSON) async throws {
        let current = library?.status["active_id"] ?? JSON()
        switch command["type"].string {
        case "manage": try await show("workspaces")
        case "manage_toolbars": try await show("this_workspace")
        case "switch": try await run(JSON(["type": "switch", "value": command["id"].raw]))
        case "new": try await run(JSON(["type": "new"]))
        case "reset_brushes": try await run(JSON(["type": "reset_brushes"]))
        case "reset_layout": try await run(JSON(["type": "reset", "value": current.raw]))
        case "layout_history": try await run(JSON(["type": "history", "value": current.raw]))
        case "save_toolbar": try await run(JSON(["type": "save_toolbar", "value": command["panel"].raw]))
        case "new_toolbar": try await run(JSON(["type": "new_toolbar", "value": command["group"].raw]))
        default: throw HostFailure(message: "This workspace command is unavailable")
        }
    }
    private func service() throws -> WorkspaceLibrary {
        guard let library else { throw HostFailure(message: "Workspace storage is unavailable") }
        return library
    }
    private func loadCatalog() async throws {
        if catalog.isNull { catalog = try await service().read(["type": "catalog"]) }
    }
    func show(_ page: String) async throws {
        guard ["workspaces", "this_workspace", "toolbar_library", "recently_deleted"].contains(page) else {
            throw HostFailure(message: "This workspace page is unavailable")
        }
        await selectionTask?.value
        try await library?.finishLayoutPreview()
        try await loadCatalog()
        self.page = page
        if page != "recently_deleted" { toolbarMode = ["this_workspace", "toolbar_library"].contains(page) }
        query = ""; selection = page == "workspaces" ? library?.status["active_id"].string : nil
        history = JSON(); showingStorage = false; presented = true
        try await refresh()
    }
    func refresh() async throws {
        guard !showingStorage && history.isNull else { return }
        requestGeneration &+= 1
        let generation = requestGeneration
        let library = try service()
        var result = try await library.read(["type": "view", "page": page, "query": query,
            "selected": selection as Any? ?? NSNull(), "idle": true])
        guard generation == requestGeneration else { return }
        if !result["rows"].array.contains(where: { $0["id"].string == selection }) { selection = nil }
        if selection == nil && toolbarMode, let first = result["rows"].array.first {
            selection = first["id"].string
            result = try await library.read(["type": "view", "page": page, "query": query,
                "selected": selection as Any? ?? NSNull(), "idle": true])
        }
        guard generation == requestGeneration else { return }
        view = result
        if !result["detail_error"].isNull { error = result["detail_error"].string }
    }
    func select(_ id: String?) {
        selection = id
        requestGeneration &+= 1
        selectionGeneration &+= 1
        let generation = selectionGeneration
        selecting = true
        let previous = selectionTask
        selectionTask = Task { [self] in
            defer { if generation == selectionGeneration { selecting = false } }
            await previous?.value
            guard selection == id && presented else { return }
            do {
                try await refresh()
                guard let library, page == "workspaces" else { return }
                guard let id, selection == id, presented,
                    !view["details"]["preview"].isNull, view["idle"].bool else {
                    try await library.finishLayoutPreview(); return
                }
                let layout = view["details"]["preview"]
                if !library.previewingLayout { try await library.beginLayoutPreview() }
                if presented && selection == id { try await library.previewLayout(layout) }
                else if !presented { try await library.finishLayoutPreview() }
            } catch { self.error = error.localizedDescription }
        }
    }
    func search() { selection = nil; select(nil) }
    func activate(_ action: JSON) {
        Task {
            do { try await run(action) }
            catch { self.error = error.localizedDescription; presented = true }
            if let store { receive(store.state.json) }
        }
    }
    func run(_ action: JSON) async throws {
        guard !["save_as_template", "use_template", "update_from_current", "import_template", "edit_as_workspace", "new_from_template"].contains(action["type"].string) else {
            throw HostFailure(message: "This workspace action is unavailable")
        }
        guard !processing else { throw HostFailure(message: "Finish the current workspace action first") }
        processing = true
        let resumePreview = library?.previewingLayout == true
        defer {
            processing = false
            if resumePreview && presented && history.isNull && prompt == nil { select(selection) }
        }
        await selectionTask?.value
        if history.isNull { try await library?.finishLayoutPreview() }
        try await loadCatalog()
        let library = try service(), type = action["type"].string, value = action["value"]
        error = nil
        switch type {
        case "switch":
            guard value.string != library.status["active_id"].string else { return }
            let item = try await library.read(["type": "load", "id": value.raw])
            let claim = item["claim"]
            if !claim.isNull && claim["owner"]["id"].string != library.status["owner"].string
                && (UInt64(claim["expires_at_ms"].string) ?? claim["expires_at_ms"].uint) > UInt64(Date().timeIntervalSince1970 * 1000) {
                try focusWindow(value.string, item: item)
            } else {
                _ = try await library.operation(["type": "switch", "id": value.raw])
            }
            presented = false
        case "switch_to_window":
            let item = try await library.read(["type": "load", "id": value.raw])
            try focusWindow(value.string, item: item); presented = false
        case "history", "versions", "metadata":
            historyID = value.string; historyMode = type == "history" ? "layout" : type
            showingStorage = false; presented = true
            if type == "history" {
                guard value.string == library.status["active_id"].string else { throw HostFailure(message: "Switch to this workspace to view its history") }
                try await library.beginLayoutPreview()
            }
            do { try await refreshHistory(nil) }
            catch { try? await library.finishLayoutPreview(); throw error }
        case "storage":
            presented = true; showingStorage = true; history = JSON()
            storage = try await library.read(["type": "storage", "clear_older": false, "apply": false])["storage"]
        case "export", "export_current":
            presented = true
            let package = try await library.read(["type": "export", "id": type == "export" ? value.raw : NSNull()])
            if try await files.export(package) { note = "Export completed." }
        case "export_database":
            presented = true
            if try await files.exportDatabase(library) { note = "Database export completed." }
        case "import_toolbar", "import_backup":
            let kind: WorkspacePackageKind = type == "import_toolbar" ? .toolbar : .workspaceBackup
            presented = true
            if let text = try await files.read(kind: kind) { try await importText(text, kind: kind) }
        case "retry_storage":
            if !library.ready { try await library.start() }
            else {
                if library.readOnly { try await library.resume() }
                _ = try await library.perform(["type": "retry"])
            }
            note = "Storage is available."; try await refresh()
        case "restore_deleted":
            _ = try await library.operation(["type": "restore_deleted", "id": value.raw])
            if undoDeletion == value.string { undoDeletion = nil }
            selection = nil; try await refresh()
        case "add_toolbar":
            _ = try await library.installToolbar(id: value.string); presented = false
        case "show_toolbar":
            try await customize(["type": "set_panel_visible", "panel": value[0].raw, "visible": value[1].bool])
            try await library.flush(); try await refresh()
        case "rename_toolbar", "duplicate_toolbar", "delete_toolbar":
            pendingCustomization = JSON(["type": type, "panel": value.raw]); presented = false
        default:
            let wasPresented = presented
            let spec = try await library.read(["type": "prompt", "action": action.raw])["prompt"]
            let succeeded = try await form(spec) { [self] fields in try await execute(action, fields: fields) }
            if !succeeded && !wasPresented { presented = false }
        }
    }
    private func execute(_ action: JSON, fields: JSON) async throws {
        let library = try service(), type = action["type"].string, value = action["value"]
        let name = fields["name"].string, description = fields["description"].string, choice = fields["choice"].string
        var operation: [String: Any] = ["id": value.raw, "name": name, "description": description]
        var nextPage: String?
        var closeAfter = false
        switch type {
        case "reset_brushes":
            try await library.resetBrushes(); presented = false; return
        case "new":
            operation["type"] = "new"; closeAfter = true
        case "rename": operation["type"] = "rename"
        case "duplicate":
            operation["type"] = "duplicate"
            let source = try await library.read(["type": "load", "id": value.raw])
            if source["entity"]["metadata"]["kind"].string == "workspace",
                !source["claim"].isNull, source["claim"]["owner"]["id"].string != library.status["owner"].string,
                (UInt64(source["claim"]["expires_at_ms"].string) ?? 0) > UInt64(Date().timeIntervalSince1970 * 1000) {
                guard let other = EditorStore.workspaceOwner(id: value.string, owner: source["claim"]["owner"]["id"].string)?.workspaceLibrary else {
                    throw HostFailure(message: "Close the workspace in its other application process to finish saving before copying it here.")
                }
                operation["source"] = try await other.snapshotForCopy().raw
                operation["type"] = "duplicate_snapshot"
            }
            closeAfter = source["entity"]["metadata"]["kind"].string == "workspace"
        case "reset": operation["type"] = "reset"
        case "save_toolbar": operation["type"] = "save_toolbar"; operation["panel"] = value.raw; nextPage = "toolbar_library"
        case "update_toolbar": operation["type"] = "update_toolbar"; operation["panel"] = try JSON.decode(choice).raw
        case "delete":
            operation["type"] = "delete"; operation["replacement"] = choice.isEmpty ? NSNull() : choice as Any
        case "delete_permanently": operation["type"] = "delete_permanently"
        case "save_as_new": operation["type"] = "save_as_new"; closeAfter = true
        case "replace_toolbar":
            _ = try await library.installToolbar(id: choice, replace: value); presented = false; return
        case "new_toolbar":
            _ = try await library.installToolbar(id: choice.isEmpty ? nil : choice, name: name, group: value); presented = false; return
        case "recover_interrupted":
            _ = try await library.perform(["type": "recover", "id": choice]); try await refresh(); return
        case "clear_older_history":
            storage = try await library.perform(["type": "storage", "clear_older": true, "apply": true])["storage"]; return
        default: throw HostFailure(message: "This workspace form is unavailable")
        }
        let result = try await library.operation(operation)
        if type == "delete" { undoDeletion = value.string; note = "Moved to Recently Deleted." }
        if closeAfter { presented = false; return }
        if let nextPage { page = nextPage; toolbarMode = nextPage == "toolbar_library" }
        if !result["selected"].isNull { selection = result["selected"].string }
        if ["delete", "delete_permanently"].contains(type) { selection = nil }
        try await refresh()
    }
    private func focusWindow(_ id: String, item: JSON) throws {
        guard let target = EditorStore.workspaceOwner(id: id, owner: item["claim"]["owner"]["id"].string),
            let focus = target.focusWindow else { throw HostFailure(message: "This workspace is open in another application window. Use that window or duplicate the workspace.") }
        focus()
    }
    private func form(_ spec: JSON, operation: (JSON) async throws -> Void) async throws -> Bool {
        var previous = JSON(), failure: String?
        while let values = await ask(spec, previous: previous, error: failure) {
            do { try await operation(values); return true }
            catch { previous = values; failure = error.localizedDescription }
        }
        return false
    }
    private func ask(_ spec: JSON, previous: JSON, error: String?) async -> JSON? {
        presented = true; formError = error
        formName = previous.isNull ? spec["name"].string : previous["name"].string
        formDescription = previous.isNull ? spec["description"].string : previous["description"].string
        formChoice = previous.isNull ? spec["selected"].string : previous["choice"].string
        return await withCheckedContinuation { continuation in
            formCompletion = { continuation.resume(returning: $0) }; prompt = spec
        }
    }
    func answer(confirm: Bool) {
        let completion = formCompletion; formCompletion = nil; prompt = nil
        completion?(confirm ? JSON(["name": formName, "description": formDescription, "choice": formChoice]) : nil)
    }
    func dismissed() {
        answer(confirm: false)
        Task { [self] in
            await selectionTask?.value
            do { try await library?.finishLayoutPreview() } catch { self.error = error.localizedDescription }
        }
        if let action = pendingCustomization {
            pendingCustomization = nil
            DispatchQueue.main.async { [weak self] in self?.store?.customize(action.object) }
        }
    }
    private func customize(_ action: [String: Any]) async throws {
        guard let store else { throw HostFailure(message: "The canvas session is unavailable") }
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            store.edit(["type": "customize", "action": action]) { error in
                if let error { continuation.resume(throwing: HostFailure(message: error)) } else { continuation.resume() }
            }
        }
    }
    func refreshHistory(_ selected: String?) async throws {
        requestGeneration &+= 1
        let generation = requestGeneration
        let result = try await service().read(["type": "history", "id": historyID, "mode": historyMode,
            "selected": selected as Any? ?? NSNull()])
        guard generation == requestGeneration else { return }
        history = result["history"]; historyPreview = result["preview"]
        if historyMode == "layout", library?.previewingLayout == true, !history["preview"].isNull {
            try await service().previewLayout(history["preview"])
        }
    }
    func selectHistory(_ id: String) { Task { do { try await refreshHistory(id) } catch { self.error = error.localizedDescription } } }
    func historyAction() {
        Task {
            guard !processing else { return }
            processing = true
            defer { processing = false }
            do {
                if historyMode == "layout" {
                    try await service().finishLayoutPreview(revision: history["selected"].string)
                    presented = false
                    return
                }
                let id = historyID, mode = historyMode, selected = history["selected"].string
                _ = try await form(history["restore"]) { [self] _ in
                    var operation: [String: Any] = ["id": id, "version": selected]
                    operation["type"] = mode == "versions" ? "restore_version" : "restore_metadata"
                    _ = try await service().operation(operation)
                    try await refreshHistory(selected)
                }
            } catch { self.error = error.localizedDescription }
        }
    }
    func back() {
        history = JSON(); showingStorage = false
        Task { do { try await refresh() } catch { self.error = error.localizedDescription } }
    }
    func openURL(_ url: URL, kind: WorkspacePackageKind) {
        Task {
            guard !processing else { error = "Finish the current workspace action first"; return }
            processing = true
            defer { processing = false }
            do {
                try await loadCatalog(); presented = true
                if let text = try await files.read(kind: kind, url: url) { try await importText(text, kind: kind) }
            } catch { self.error = error.localizedDescription }
        }
    }
    private func importText(_ text: String, kind: WorkspacePackageKind) async throws {
        guard kind != .template else { throw HostFailure(message: "Loading saved layouts is no longer available") }
        let result = try await service().perform(["type": "import", "kind": kind.rawValue, "text": text])
        if kind == .workspaceBackup { presented = false }
        else {
            page = "toolbar_library"; toolbarMode = true
            history = JSON(); showingStorage = false; selection = result["selected"].string
            note = "Toolbar imported. Choose Add to Workspace to use it."
            try await refresh()
        }
    }
}
