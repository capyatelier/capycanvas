import SwiftUI
#if os(iOS)
import UIKit
#endif

@MainActor final class ArtworkRecovery: ObservableObject {
    @Published private(set) var saving = false
    @Published private(set) var restoring = true
    @Published private(set) var error: String?
    @Published private(set) var restoreError: String?
    @Published private var dismissedError: String?
    private weak var store: EditorStore?
    private var scheduled: Task<Void, Never>?
    private var observed = ""
    private var durable = ""
    private var closed = false
    private var started = false
    private var restoreFailed = false
    private var finalCheckpoint = false
    private var flushDeadline: Date?
    private var waiters: [(Bool) -> Void] = []
    var copy: JSON { store?.bootstrap["recovery"] ?? JSON() }
    var hasCurrentCopy: Bool { !restoring && !restoreFailed && !saving && observed == durable }
    var failure: String? { error ?? restoreError ?? (restoring ? copy["restoring"].string : nil) }
    var visibleError: String? {
        let message = error ?? restoreError
        return message == dismissedError ? nil : message
    }

    init(store: EditorStore) { self.store = store }
    func observe() {
        guard let store else { return }
        if !started {
            guard store.snapshot["canvas_ready"].bool, store.workspaces?.ready != false else { return }
            started = true
            guard let sessions = store.native?.sessions else { restoring = false; return }
            store.native?.restoreSession(sessions: sessions, scene: store.sessionIdentity, adopt: Self.adoptsUnrestoredSessions) {
                [weak self] failure, adopted in DispatchQueue.main.async {
                guard let self else { return }
                self.restoring = false
                if self.restoreError != failure { self.dismissedError = nil }
                self.restoreError = failure
                self.restoreFailed = failure != nil && !adopted
                if !self.restoreFailed { self.schedule() }
                self.store?.wake?()
            } }
            return
        }
        let tabs = store.snapshot["document_tabs"], camera = store.camera.value
        let next = JSON(["stamps": tabs["session_stamps"].raw, "order": tabs["tabs"].array.map { $0["id"].raw },
            "active": tabs["selected"].raw, "camera": ["translation": camera["translation"].raw,
                "zoom": camera["zoom"].raw, "rotation": camera["rotation"].raw, "flipped": camera["flipped"].raw]]).stableKey
        guard next != observed else { return }
        observed = next
        if !restoring { schedule() }
    }
    private func schedule() {
        guard !closed, !restoring, !restoreFailed, scheduled == nil, !saving, store?.native?.sessions != nil else { return }
        scheduled = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(2)) } catch { return }
            guard let self else { return }
            self.scheduled = nil
            self.write()
        }
    }
    private func finish(_ failure: String?, accepted: Bool = false, exclusion: UInt64 = 0) {
        saving = false
        if error != failure { dismissedError = nil }
        error = failure
        let pending = exclusion == 0 && store?.native?.sessions != nil && (observed != durable || finalCheckpoint)
        if (failure == nil || accepted) && !waiters.isEmpty && pending {
            if Date() < (flushDeadline ?? .distantFuture) { write(); return }
            error = store?.catalog["document_delivery_copy"]["change_in_progress"].string ?? ""
        }
        let callbacks = waiters; waiters.removeAll()
        flushDeadline = nil
        callbacks.forEach { $0((error == nil || accepted) && !pending) }
        if exclusion == 0 && (failure == nil || accepted) && observed != durable { schedule() }
    }
    private func write(exclusion: UInt64 = 0, cleanExit: Bool = false) {
        guard !saving else { return }
        guard let native = store?.native, native.sessions != nil else { finish(nil); return }
        guard !restoring else { finish(copy["restoring"].string); return }
        saving = true
        let cleanExit = cleanExit || finalCheckpoint
        finalCheckpoint = false
        let captured = observed
        native.checkpointSession(exclusion: exclusion, cleanExit: cleanExit, waits: !waiters.isEmpty) { [self] failure, committed in DispatchQueue.main.async { [self] in
            if committed { durable = captured }
            finish(failure, accepted: committed, exclusion: exclusion)
        } }
    }
    func flush(cleanExit: Bool = false, _ completion: @escaping (Bool) -> Void) {
        scheduled?.cancel(); scheduled = nil
        if restoring {
            completion(store?.native?.sessions == nil); return
        }
        guard !restoreFailed else { completion(false); return }
        waiters.append { [self] saved in completion(saved); withExtendedLifetime(self) {} }
        finalCheckpoint = finalCheckpoint || cleanExit
        flushDeadline = Date().addingTimeInterval(10)
        if saving { return }
        write(cleanExit: cleanExit)
    }
    func removeDrawing(_ id: UInt64, completion: @escaping (Bool) -> Void) {
        scheduled?.cancel(); scheduled = nil
        if saving { flush { [self] saved in if saved { removeDrawing(id, completion: completion) } else { completion(false) } }; return }
        waiters.append(completion)
        write(exclusion: id)
    }
    func close(_ completion: @escaping (Bool) -> Void = { _ in }) {
        scheduled?.cancel(); scheduled = nil
        if saving { flush { [self] saved in if saved { close(completion) } else { completion(false) } }; return }
        closed = true
        waiters.append(completion)
        write(exclusion: UInt64.max)
    }
    func resume() { closed = false; durable = ""; schedule() }
    /// A scene the system still holds may reconnect to its own session.
    private static var adoptsUnrestoredSessions: Bool {
        #if os(iOS)
        return UIApplication.shared.openSessions.allSatisfy { $0.scene != nil }
        #else
        return true
        #endif
    }
    func retry() {
        guard !restoring, !saving else { return }
        dismissedError = nil
        guard restoreError != nil, let store, let sessions = store.native?.sessions else { flush { _ in }; return }
        scheduled?.cancel(); scheduled = nil
        let available = !restoreFailed
        restoring = true
        store.native?.restoreSession(sessions: sessions, scene: store.sessionIdentity, adopt: false, retry: true) {
            [weak self] failure, adopted in DispatchQueue.main.async {
            guard let self else { return }
            self.restoring = false; self.restoreFailed = failure != nil && !adopted && !available; self.restoreError = failure
            if !self.restoreFailed { self.schedule() }
        } }
    }
    func later() { dismissedError = error ?? restoreError }
}
