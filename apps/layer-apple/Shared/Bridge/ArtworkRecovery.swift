import SwiftUI

/// Native timing and file execution for the shared recovery state machine.
/// Each runtime owns a fresh storage identity; a blank restored scene cannot
/// overwrite an abandoned process's recovery copy.
@MainActor final class ArtworkRecovery: ObservableObject {
    @Published var presented = false
    @Published private(set) var records: [RecoveryRecord] = []
    @Published private var writeError: String?
    @Published private var readError: String?
    @Published private(set) var saving = false
    @Published private(set) var canOpen = false
    private static let owners = NSHashTable<ArtworkRecovery>.weakObjects()
    let files: RecoveryFiles
    private let identity = UUID()
    private weak var store: EditorStore?
    private var file = JSON()
    private var policy = ""
    private var pendingWork: JSON?
    private var pendingAdoption: RecoveryRecord?
    private var isCurrent = false
    private var current: RecoveryRecord?
    private var restored: RecoveryRecord?
    private var selection: RecoveryRecord?
    private var scheduled: Task<Void, Never>?
    private var closed = false
    private var waiters: [(Bool) -> Void] = []
    private var flushDeadline: Date?
    var hasCurrentCopy: Bool { current != nil && isCurrent && !saving }
    var error: String? {
        let messages = [writeError, readError].compactMap { $0 }
        return messages.isEmpty ? nil : messages.joined(separator: "\n")
    }

    init(store: EditorStore) {
        self.store = store; files = RecoveryFiles(root: store.native?.persistenceRoot)
        Self.owners.add(self)
        refresh(offer: true)
    }
    func refresh(offer: Bool = false) {
        let files = files
        NativeProjectTask.io.async { [weak self] in
            do {
                let listing = try files.list()
                DispatchQueue.main.async {
                    guard let self else { return }
                    let active = Set(Self.owners.allObjects.map(\.identity))
                    self.records = listing.records.filter { !active.contains($0.scene) && $0 != self.restored }
                    self.readError = listing.errors.isEmpty ? nil : listing.errors.joined(separator: "\n")
                    if offer && !self.records.isEmpty { self.presented = true }
                }
            } catch { let message = error.localizedDescription
                DispatchQueue.main.async { self?.readError = "Could not read recovered drawings: \(message)" }
            }
        }
    }
    /// File changes wake the native debounce; only shared policy decides whether
    /// an observed document needs capture or retirement.
    func observe(_ next: JSON) {
        canOpen = store?.command("open_document")["enabled"].bool == true
        guard !SnapshotProjection.equal(file.raw, next.raw) else { return }
        file = next; isCurrent = false
        schedule()
    }
    private func update(_ event: [String: Any]) throws {
        let event = try JSON(event).encoded()
        let pointer = policy.withCString { state in event.withCString { capy_recovery_update(state, $0) } }
        guard let pointer else { throw HostFailure(message: "Recovery policy is unavailable") }
        defer { capy_apple_string_free(pointer) }
        let result = try JSON.decode(String(cString: pointer))
        if !result["error"].isNull { throw HostFailure(message: result["error"].string) }
        let view = result["update"]
        if !view["work"].isNull {
            guard pendingWork == nil else { throw HostFailure(message: "Recovery work is already queued") }
            pendingWork = view["work"]
        }
        policy = result["state"].string; isCurrent = view["current"].bool
        for key in view["release"].array {
            if let restored, try Self.record(key.string) == restored { self.restored = nil }
        }
    }
    private static func key(_ record: RecoveryRecord) throws -> String {
        let encoder = JSONEncoder(); encoder.outputFormatting = .sortedKeys
        return String(decoding: try encoder.encode(record), as: UTF8.self)
    }
    private static func record(_ key: String) throws -> RecoveryRecord {
        try JSONDecoder().decode(RecoveryRecord.self, from: Data(key.utf8))
    }
    private func schedule() {
        guard files.root != nil, !closed, scheduled == nil, !saving else { return }
        scheduled = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(2)) } catch { return }
            guard let self else { return }
            self.scheduled = nil; self.process()
        }
    }
    private func finish(_ failure: String? = nil) {
        saving = false; writeError = failure
        if failure != nil || (isCurrent && pendingAdoption == nil) || files.root == nil {
            let callbacks = waiters; waiters.removeAll(); flushDeadline = nil
            callbacks.forEach { $0(failure == nil) }
        } else if !waiters.isEmpty && Date() >= (flushDeadline ?? .distantPast) {
            finish("The drawing is still busy; its previous recovery copy is preserved.")
            return
        }
        if failure == nil && (!isCurrent || pendingAdoption != nil) { schedule() }
    }
    private func process() {
        guard !saving else { return }
        guard files.root != nil else { finish(); return }
        if let work = pendingWork {
            pendingWork = nil; saving = true; execute(work); return
        }
        guard let native = store?.native else { finish("The drawing owner is unavailable"); return }
        saving = true
        native.submit(2, JSON(["type": "recovery_document"])) { [weak self] document in
            DispatchQueue.main.async {
                guard let self else { return }
                self.saving = false
                // A close can queue storage work while the owner query runs.
                if self.pendingWork != nil { self.process(); return }
                guard let document else { self.finish("Could not observe the drawing for recovery"); return }
                do {
                    if let adopted = self.pendingAdoption {
                        // Finish prior tickets before adopting an origin. Supply
                        // its actual new document before enabling its first work.
                        try self.update(["type": "observe", "document": document.raw, "owned": false])
                        try self.update(["type": "adopted", "key": Self.key(adopted)])
                        self.pendingAdoption = nil
                        if self.closed { try self.update(["type": "retire", "discard_origin": true]) }
                    }
                    try self.update(["type": "observe", "document": document.raw, "owned": true])
                    if self.pendingWork != nil { self.process() } else { self.finish() }
                } catch { self.finish(error.localizedDescription) }
            }
        }
    }
    private func completed(_ work: JSON, success: Bool, failure: String? = nil) {
        do {
            try update(["type": "complete", "token": work["token"].uint, "success": success])
            saving = false
            if success || pendingWork != nil { process() }
            else { finish(failure) }
        } catch { finish(error.localizedDescription) }
    }
    private func execute(_ work: JSON) {
        let files = files
        switch work["kind"]["type"].string {
        case "capture":
            guard let native = store?.native else {
                completed(work, success: false, failure: "The drawing owner is unavailable"); return
            }
            let document = work["document"], identity = identity
            let title = file["location"]["name"].string
            native.recoveryTask(expected: (document["epoch"].uint, document["revision"].uint)) { [weak self] task, failure in
                guard let task else {
                    DispatchQueue.main.async { self?.completed(work, success: false, failure: failure) }; return
                }
                NativeProjectTask.io.async { [weak self] in
                    do {
                        let record = try files.write(task, scene: identity, title: title.isEmpty ? "Untitled" : title)
                        DispatchQueue.main.async {
                            guard let self else { return }
                            self.current = record; self.completed(work, success: true)
                        }
                    } catch { let message = error.localizedDescription
                        DispatchQueue.main.async { self?.completed(work, success: false, failure: "Could not save the recovery copy: \(message)") }
                    }
                }
            }
        case "retire", "retire_origin":
            do {
                let origin = work["kind"]["type"].string == "retire_origin"
                let record = try origin ? Self.record(work["kind"]["key"].string) : current
                NativeProjectTask.io.async { [weak self] in
                    do {
                        if let record { try files.remove(record) }
                        DispatchQueue.main.async {
                            guard let self else { return }
                            if !origin { self.current = nil }
                            self.completed(work, success: true)
                        }
                    } catch { let message = error.localizedDescription
                        DispatchQueue.main.async { self?.completed(work, success: false, failure: "Could not clear the recovery copy: \(message)") }
                    }
                }
            } catch { completed(work, success: false, failure: error.localizedDescription) }
        default:
            completed(work, success: false, failure: "Unsupported recovery storage work")
        }
    }
    func flush(_ completion: @escaping (Bool) -> Void) {
        scheduled?.cancel(); scheduled = nil
        // Retain the coordinator through an accepted lifecycle barrier, even
        // when the scene releases its final editor reference during the write.
        waiters.append { [self] result in completion(result); withExtendedLifetime(self) {} }
        flushDeadline = Date().addingTimeInterval(10)
        process()
    }
    func close(_ completion: @escaping (Bool) -> Void = { _ in }) {
        do {
            try update(["type": "retire", "discard_origin": true])
            try update(["type": "close"])
            closed = true; flush(completion)
        } catch { finish(error.localizedDescription); completion(false) }
    }
    func resume() {
        // Cancelling an app-wide close also visits windows not yet prepared.
        // Their ordinary checkpoint work must finish without interruption.
        guard closed else { return }
        do { try update(["type": "resume"]); closed = false; schedule() }
        catch { finish(error.localizedDescription) }
    }
    func didRestore(_ record: RecoveryRecord) {
        restored = record; pendingAdoption = record; isCurrent = false; schedule()
    }
    func restore(_ record: RecoveryRecord) {
        store?.projectFiles.recover(record)
    }
    func choose(_ record: RecoveryRecord) { selection = record; presented = false }
    func dismissed() {
        guard let record = selection else { return }
        selection = nil; restore(record)
    }
    func discard(_ record: RecoveryRecord) {
        let files = files
        NativeProjectTask.io.async { [weak self] in
            do { try files.remove(record); DispatchQueue.main.async { self?.refresh() } }
            catch { let message = error.localizedDescription
                DispatchQueue.main.async { self?.writeError = "Could not discard the recovery copy: \(message)" }
            }
        }
    }
}
