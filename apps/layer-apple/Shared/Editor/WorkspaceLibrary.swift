import SwiftUI

/// Serializes storage/adoption across two native owners without blocking either
/// MainActor or drawing on SQLite. The editor is locked only for an explicit
/// transition; ordinary autosave captures a committed boundary asynchronously.
@MainActor final class WorkspaceLibrary: ObservableObject {
    @Published private(set) var ready = false
    @Published private(set) var busy = false
    @Published private(set) var readOnly = false
    @Published private(set) var pendingEdits = false
    @Published private(set) var status = JSON()
    @Published var error: String?
    private weak var store: EditorStore?
    private let owner: NativeWorkspaceLibrary
    private let scene: String
    private var tail: Task<Void, Never>?
    private var autosave: Task<Void, Never>?
    private var renewal: Task<Void, Never>?
    private var generation = JSON()
    private var bindingKey = ""
    private var edits: UInt64 = 0
    private var observed: UInt64 = 0
    private var suspended = false
    private var closed = false
    private(set) var previewingLayout = false
    var hasUnsavedChanges: Bool { !ready || busy || pendingEdits || status["dirty"].bool || status["saving"].bool }

    init(store: EditorStore, platform: UInt32, root: URL, scene: String) throws {
        self.store = store; self.scene = UUID(uuidString: scene)?.uuidString ?? "default"
        owner = try NativeWorkspaceLibrary(platform: platform, root: root, scene: scene)
    }
    deinit { autosave?.cancel(); renewal?.cancel() }
    private var now: UInt64 { UInt64(Date().timeIntervalSince1970 * 1000) }
    private func serialized<T>(_ body: @escaping @MainActor () async throws -> T) async throws -> T {
        let previous = tail
        let task = Task { @MainActor in
            await previous?.value
            return try await body()
        }
        tail = Task { _ = await task.result }
        return try await task.value
    }
    private func session(_ value: [String: Any]) async throws -> JSON {
        guard let native = store?.native else { throw HostFailure(message: "The canvas session is unavailable") }
        return try await withCheckedThrowingContinuation { continuation in
            native.workspaceSession(JSON(value)) { value, error in
                // Publication is enqueued first, so editor state is coherent
                // before storage receives the adoption acknowledgement.
                DispatchQueue.main.async {
                    if let error { continuation.resume(throwing: HostFailure(message: error)) }
                    else { continuation.resume(returning: value ?? JSON()) }
                }
            }
        }
    }
    private func checked(_ reply: JSON) throws -> JSON {
        if !reply["status"].isNull {
            if !SnapshotProjection.equal(status.raw, reply["status"].raw) { status = reply["status"] }
            if !status["error"].isNull { error = status["error"]["message"].string }
        }
        if !reply["error"].isNull {
            error = reply["error"]["message"].string
            throw WorkspaceLibraryFailure(detail: reply["error"])
        }
        return reply["value"]
    }
    private func request(_ value: [String: Any]) async throws -> JSON {
        let reply: JSON = await withCheckedContinuation { continuation in
            owner.request(JSON(value)) { continuation.resume(returning: $0) }
        }
        let result = try checked(reply)
        if value["type"] as? String == "flush", observed == edits, pendingEdits { pendingEdits = false }
        return result
    }
    func start() async throws {
        try await serialized { [self] in
            guard !ready && !closed else { return }
            busy = true
            defer { busy = false }
            let migrated: JSON = await withCheckedContinuation { continuation in
                owner.migrateLegacy { continuation.resume(returning: $0) }
            }
            let mappings = try checked(migrated)["mappings"]
            let sceneID = mappings["workspaces/\(scene).json"]
            let preferred = sceneID.isNull ? mappings["workspace.json"] : sceneID
            let deadline = Date().addingTimeInterval(30)
            // Startup shader preparation can temporarily defer document-idle.
            // No interaction is canceled to make initialization succeed.
            while true {
                do { _ = try await session(["type": "begin"]); break }
                catch {
                    guard Date() < deadline else { throw error }
                    try await Task.sleep(for: .milliseconds(16))
                }
            }
            do {
                let incoming = try await request(["type": "initialize", "now": now, "preferred": preferred.raw])
                try await adopt(incoming)
                _ = try await session(["type": "end"])
                ready = true; error = status["error"].isNull ? nil : status["error"]["message"].string
                store?.native?.workspaceDidInitialize()
                if let store { store.workspaceManager.receive(store.state.json) }
                scheduleRenewal()
            } catch {
                _ = try? await session(["type": "end"])
                self.error = error.localizedDescription
                throw error
            }
        }
    }
    private func adopt(_ value: JSON) async throws {
        let adoption = value["adoption"]
        if !adoption.isNull {
            do { _ = try await session(["type": "adopt", "capture": adoption["capture"].raw]) }
            catch {
                _ = try? await request(["type": "reject", "token": adoption["token"].raw])
                throw error
            }
            let active = try await request(["type": "activate", "token": adoption["token"].raw])
            try await configure(active["binding"])
            generation = JSON(); observed = edits
            pendingEdits = false
            try await setReadOnly(suspended)
        } else { try await configure(value["binding"]) }
    }
    private func configure(_ binding: JSON) async throws {
        guard !binding.isNull, binding.stableKey != bindingKey else { return }
        _ = try await session(["type": "configure", "binding": binding.raw])
        bindingKey = binding.stableKey
    }
    private func setReadOnly(_ value: Bool) async throws {
        readOnly = value
        _ = try await session(["type": "read_only", "value": value])
    }
    /// Full editor publications signal potentially changed working state. Motion
    /// and camera patches never schedule database work or copy layout history.
    func observe() {
        guard ready, !closed, !busy else { return }
        edits &+= 1
        if !pendingEdits { pendingEdits = true }
        scheduleAutosave()
    }
    private func scheduleAutosave() {
        guard autosave == nil, ready, !closed, !suspended, !readOnly else { return }
        autosave = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(250)) }
            catch { return }
            guard let self else { return }
            do {
                try await self.serialized {
                    guard !self.closed && !self.suspended && !self.readOnly else { return }
                    let idle = try await self.capture()
                    if idle { _ = try await self.request(["type": "flush"]) }
                }
                autosave = nil
                if observed != edits { scheduleAutosave() }
            } catch {
                autosave = nil
                self.error = error.localizedDescription
                await protectOwnership(error)
            }
        }
    }
    @discardableResult private func capture() async throws -> Bool {
        let revision = edits
        let snapshot = try await session(["type": "capture", "generation": generation.raw])
        _ = try await request(["type": "observe", "capture": snapshot["capture"].raw,
            "working": snapshot["working"].raw, "now": now])
        if !snapshot["capture"].isNull { generation = snapshot["generation"] }
        if snapshot["idle"].bool { observed = revision }
        return snapshot["idle"].bool
    }
    /// This path also handles imports, pruning and recovery that may prepare an
    /// adoption. Every operation captures the latest editor state after taking
    /// the interaction lock, then acknowledges storage before changing identity.
    func perform(_ value: [String: Any], recovering: Bool = false) async throws -> JSON {
        try await serialized { [self] in
            guard ready && !closed else { throw HostFailure(message: "Workspace storage is not ready") }
            if try await storedOnly(value) {
                do {
                    var message = value; message["now"] = now
                    let result = try await request(message)
                    try await configure(result["binding"])
                    error = status["error"].isNull ? nil : status["error"]["message"].string
                    return result
                } catch { await protectOwnership(error); throw error }
            }
            busy = true
            defer { busy = false; if observed != edits { scheduleAutosave() } }
            let latest = try await session(["type": "begin"])
            do {
                var message = value; message["now"] = now
                if recovering {
                    var operation = JSON(message["operation"] ?? NSNull()).object
                    operation["capture"] = latest["capture"].raw; message["operation"] = operation
                } else {
                    _ = try await request(["type": "observe", "capture": latest["capture"].raw,
                        "working": latest["capture"]["working"].raw, "now": now])
                    if !["retry", "recover"].contains(value["type"] as? String ?? "") {
                        _ = try await request(["type": "flush"])
                    }
                }
                let result = try await request(message)
                try await adopt(result)
                _ = try await session(["type": "end"])
                error = status["error"].isNull ? nil : status["error"]["message"].string
                return result
            } catch {
                _ = try? await session(["type": "end"])
                self.error = error.localizedDescription
                await protectOwnership(error)
                throw error
            }
        }
    }
    func operation(_ value: [String: Any]) async throws -> JSON {
        try await perform(["type": "operation", "operation": value], recovering: value["type"] as? String == "save_as_new")
    }
    private func storedOnly(_ value: [String: Any]) async throws -> Bool {
        if value["type"] as? String == "import" { return value["kind"] as? String != "workspace_backup" }
        guard value["type"] as? String == "operation" else { return false }
        let operation = JSON(value["operation"] ?? NSNull())
        switch operation["type"].string {
        case "rename", "restore_metadata", "restore_deleted", "delete_permanently", "restore_version": return true
        case "delete": return operation["id"].string != status["active_id"].string
        case "duplicate":
            return try await request(["type": "load", "id": operation["id"].raw])["entity"]["metadata"]["kind"].string != "workspace"
        default: return false
        }
    }
    func installToolbar(id: String? = nil, name: String? = nil, replace: JSON = JSON(), group: JSON = JSON()) async throws -> JSON {
        try await serialized { [self] in
            guard ready && !closed else { throw HostFailure(message: "Workspace storage is not ready") }
            busy = true
            defer { busy = false; if observed != edits { scheduleAutosave() } }
            let latest = try await session(["type": "begin"])
            do {
                _ = try await request(["type": "observe", "capture": latest["capture"].raw,
                    "working": latest["capture"]["working"].raw, "now": now])
                _ = try await request(["type": "flush"])
                let toolbar = try await request(["type": "toolbar", "id": id as Any? ?? NSNull(), "name": name as Any? ?? NSNull()])
                let result = try await session(["type": "install_toolbar", "config": toolbar["config"].raw,
                    "replace": replace.raw, "group": group.raw, "exact_name": name != nil])
                _ = try await capture()
                _ = try await request(["type": "flush"])
                _ = try await session(["type": "end"])
                error = status["error"].isNull ? nil : status["error"]["message"].string
                return result["panel"]
            } catch {
                _ = try? await session(["type": "end"])
                self.error = error.localizedDescription
                await protectOwnership(error)
                throw error
            }
        }
    }
    func read(_ value: [String: Any]) async throws -> JSON {
        try await serialized { [self] in
            var message = value; message["now"] = now
            let kind = value["type"] as? String ?? ""
            guard ["catalog", "view", "load", "interrupted", "export", "storage", "prompt", "history"].contains(kind),
                !(value["apply"] as? Bool ?? false) else {
                throw HostFailure(message: "This workspace operation requires an editor transition")
            }
            if ["view", "prompt", "history"].contains(kind), ready && !closed { message["idle"] = try await capture() }
            if kind == "export" {
                if ready {
                    guard try await capture() else { throw HostFailure(message: "Finish the current interaction before exporting the workspace") }
                } else {
                    let snapshot = try await session(["type": "capture"])
                    guard snapshot["idle"].bool && !snapshot["capture"].isNull else {
                        throw HostFailure(message: "Finish the current interaction before exporting the workspace")
                    }
                    message["capture"] = snapshot["capture"].raw
                }
                if message["id"] as? String == status["active_id"].string { message["id"] = NSNull() }
            }
            let result = try await request(message)
            try await configure(result["binding"])
            return result
        }
    }
    func flush() async throws {
        try await serialized { [self] in
            guard ready && !closed else { throw HostFailure(message: "Workspace storage is not ready") }
            guard try await capture() else { throw HostFailure(message: "Finish the current interaction before saving the workspace") }
            _ = try await request(["type": "flush"])
        }
    }
    func backup(to url: URL) async throws {
        try await serialized { [self] in _ = try await request(["type": "backup", "path": url.path]) }
    }
    func snapshotForCopy() async throws -> JSON {
        try await serialized { [self] in
            guard ready && !closed && !readOnly else { throw HostFailure(message: "The source window must recover workspace ownership first") }
            busy = true
            defer { busy = false }
            let latest = try await session(["type": "begin"])
            do {
                _ = try await request(["type": "observe", "capture": latest["capture"].raw,
                    "working": latest["capture"]["working"].raw, "now": now])
                _ = try await request(["type": "flush"])
                let source = try await request(["type": "load", "id": status["active_id"].raw])
                _ = try await session(["type": "end"])
                return source["entity"]
            } catch {
                _ = try? await session(["type": "end"])
                throw error
            }
        }
    }
    func close() async throws {
        if previewingLayout {
            store?.workspaceManager.presented = false
            try await finishLayoutPreview()
        }
        try await serialized { [self] in
            guard !closed else { return }
            guard ready else { throw HostFailure(message: "Workspace storage is not ready") }
            busy = true
            defer { busy = false }
            let latest = try await session(["type": "begin"])
            do {
                _ = try await request(["type": "observe", "capture": latest["capture"].raw,
                    "working": latest["capture"]["working"].raw, "now": now])
                _ = try await request(["type": "close"])
                try await setReadOnly(true)
                _ = try await session(["type": "end"])
                closed = true; ready = false
                autosave?.cancel(); renewal?.cancel()
            } catch {
                _ = try? await session(["type": "end"])
                self.error = error.localizedDescription
                await protectOwnership(error)
                throw error
            }
        }
    }
    func beginLayoutPreview() async throws {
        try await serialized { [self] in
            guard ready && !closed && !readOnly else {
                throw HostFailure(message: "Recover workspace ownership before previewing a layout")
            }
            let latest = try await session(["type": "begin"])
            busy = true
            do {
                _ = try await request(["type": "observe", "capture": latest["capture"].raw,
                    "working": latest["capture"]["working"].raw, "now": now])
                _ = try await request(["type": "flush"])
                _ = try await session(["type": "preview_begin"])
                previewingLayout = true
            } catch {
                _ = try? await session(["type": "end"]); busy = false
                await protectOwnership(error); throw error
            }
        }
    }
    func previewLayout(_ layout: JSON) async throws {
        try await serialized { [self] in
            guard previewingLayout else { throw HostFailure(message: "Open Layout History first") }
            _ = try await session(["type": "preview_layout", "layout": layout.raw])
        }
    }
    func finishLayoutPreview(revision: String? = nil) async throws {
        try await serialized { [self] in
            guard previewingLayout else { return }
            defer { previewingLayout = false; busy = false; if observed != edits { scheduleAutosave() } }
            do {
                _ = try await session(["type": "preview_cancel"])
                if let revision {
                    let result = try await request(["type": "operation", "now": now,
                        "operation": ["type": "reset", "id": status["active_id"].raw, "revision": revision]])
                    try await adopt(result)
                }
                _ = try await session(["type": "end"])
            } catch {
                _ = try? await session(["type": "preview_cancel"])
                _ = try? await session(["type": "end"])
                self.error = error.localizedDescription
                await protectOwnership(error); throw error
            }
        }
    }
    func suspend() {
        suspended = true; readOnly = true
        if previewingLayout { store?.workspaceManager.presented = false }
        renewal?.cancel(); renewal = nil
        Task { [self] in
            try? await finishLayoutPreview()
            _ = try? await session(["type": "read_only", "value": true])
        }
    }
    /// The OS has discarded the scene, or the user explicitly chose to quit
    /// after a save failed. Preserve saved data while retiring this claim.
    func detach() async {
        readOnly = true
        autosave?.cancel(); renewal?.cancel()
        _ = try? await serialized { [self] in
            closed = true; ready = false
            _ = try? await session(["type": "read_only", "value": true])
            _ = try await request(["type": "detach"])
        }
    }
    func reopenAfterCancelledClose() async throws {
        guard closed else { return }
        closed = false; ready = true
        try await resume()
    }
    func resume() async throws {
        readOnly = true
        try await serialized { [self] in
            guard ready && !closed else { return }
            try await setReadOnly(true)
            do {
                _ = try await request(["type": "revalidate", "now": now])
                suspended = false
                try await setReadOnly(false)
                error = status["error"].isNull ? nil : status["error"]["message"].string
                scheduleRenewal()
                if observed != edits { scheduleAutosave() }
            } catch { self.error = error.localizedDescription; throw error }
        }
    }
    private func scheduleRenewal() {
        renewal?.cancel()
        guard ready && !closed && !suspended else { return }
        let milliseconds = max(1, status["renew_after_ms"].uint)
        renewal = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(milliseconds)) }
            catch { return }
            guard let self else { return }
            do {
                try await self.serialized {
                    guard self.ready && !self.closed && !self.suspended else { return }
                    _ = try await self.request(["type": "revalidate", "now": self.now])
                }
                scheduleRenewal()
            } catch { self.error = error.localizedDescription; try? await setReadOnly(true) }
        }
    }
    private func protectOwnership(_ failure: Error) async {
        let kind = (failure as? WorkspaceLibraryFailure)?.detail["kind"].string ?? ""
        if ["owned_elsewhere", "conflict"].contains(kind)
            || (!status["active_id"].isNull && status["lease_expires_at_ms"].uint <= now) {
            try? await setReadOnly(true)
        }
    }
}

private struct WorkspaceLibraryFailure: Error, LocalizedError {
    let detail: JSON
    var errorDescription: String? { detail["message"].string }
}
