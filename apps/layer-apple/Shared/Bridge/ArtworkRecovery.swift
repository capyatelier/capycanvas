import SwiftUI

/// One capture/write at a time, with only the newest requested revision retained.
/// Each runtime owner gets a new identity so a restored scene's blank startup
/// can never overwrite the previous process's recovery copy.
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
    private var latestKey = ""
    private var savedKey = ""
    private var current: RecoveryRecord?
    private var restored: RecoveryRecord?
    private var selection: RecoveryRecord?
    private var scheduled: Task<Void, Never>?
    private var closed = false
    private var waiters: [(Bool) -> Void] = []
    private var flushDeadline: Date?
    var hasCurrentCopy: Bool { current != nil && latestKey == savedKey && !saving }
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
    func observe(_ next: JSON) {
        canOpen = store?.command("open_document")["enabled"].bool == true
        file = next
        let key = "\(next["epoch"].uint):\(next["revision"].uint):\(next["modified"].bool)"
        guard key != latestKey else { return }
        latestKey = key
        schedule()
    }
    private func schedule() {
        guard files.root != nil, !closed, scheduled == nil, !saving, latestKey != savedKey else { return }
        scheduled = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(2)) } catch { return }
            guard let self else { return }
            self.scheduled = nil; self.process()
        }
    }
    private func finish(_ failure: String? = nil) {
        saving = false; writeError = failure
        if failure != nil || latestKey == savedKey || closed {
            let callbacks = waiters; waiters.removeAll(); flushDeadline = nil
            callbacks.forEach { $0(failure == nil) }
        } else if !waiters.isEmpty {
            if Date() >= (flushDeadline ?? .distantPast) { finish("The drawing is still busy; its previous recovery copy is preserved.") }
            else { schedule() }
        } else { schedule() }
    }
    private func process() {
        guard !saving else { return }
        guard files.root != nil, !file.isNull else { savedKey = latestKey; finish(); return }
        if latestKey == savedKey && !closed { finish(); return }
        let key = latestKey, files = files
        if closed || !file["modified"].bool {
            let obsolete = [current, restored].compactMap { $0 }
            saving = true
            NativeProjectTask.io.async { [weak self] in
                do {
                    for record in obsolete { try files.remove(record) }
                    DispatchQueue.main.async {
                        guard let self else { return }
                        self.current = nil; self.restored = nil; self.savedKey = key; self.finish()
                    }
                } catch { let message = error.localizedDescription
                    DispatchQueue.main.async { self?.finish("Could not clear the recovery copy: \(message)") }
                }
            }
            return
        }
        guard !file["busy"].bool, let native = store?.native else { finish(); return }
        let title = file["location"]["name"].string
        saving = true
        native.recoveryTask(expected: (file["epoch"].uint, file["revision"].uint)) { [weak self] task, failure in
            DispatchQueue.main.async {
                guard let self else { return }
                guard let task else { self.finish(failure); return }
                let identity = self.identity, restored = self.restored
                NativeProjectTask.io.async { [weak self] in
                    do {
                        let record = try files.write(task, scene: identity, title: title.isEmpty ? "Untitled" : title)
                        if let restored { try files.remove(restored) }
                        DispatchQueue.main.async {
                            guard let self else { return }
                            self.current = record; self.savedKey = key
                            if self.restored == restored { self.restored = nil }
                            // A close accepted during the write still needs to
                            // remove that completed generation before replying.
                            if self.closed { self.saving = false; self.process() }
                            else { self.finish() }
                        }
                    } catch { let message = error.localizedDescription
                        DispatchQueue.main.async { self?.finish("Could not save the recovery copy: \(message)") }
                    }
                }
            }
        }
    }
    func flush(_ completion: @escaping (Bool) -> Void) {
        scheduled?.cancel(); scheduled = nil
        // Retain the coordinator until its barrier finishes, including an
        // accepted scene close that releases the last UI reference mid-write.
        waiters.append { [self] result in completion(result); withExtendedLifetime(self) {} }
        flushDeadline = Date().addingTimeInterval(10)
        process()
    }
    func close(_ completion: @escaping (Bool) -> Void = { _ in }) {
        closed = true; flush(completion)
    }
    func resume() { closed = false; savedKey = ""; schedule() }
    func didRestore(_ record: RecoveryRecord) { restored = record }
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
