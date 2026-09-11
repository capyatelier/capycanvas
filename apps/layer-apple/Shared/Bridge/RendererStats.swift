import SwiftUI

@MainActor final class RendererStats: ObservableObject {
    @Published private(set) var view = JSON()
    private weak var store: EditorStore?
    private var viewers: Set<UUID> = []
    private var task: Task<Void, Never>?
    init(store: EditorStore) { self.store = store }
    func show(_ id: UUID) {
        viewers.insert(id)
        guard task == nil else { return }
        task = Task { [weak self] in
            while !Task.isCancelled {
                guard let store = self?.store else { return }
                let next: JSON = await withCheckedContinuation { continuation in
                    store.query(["type": "renderer_stats"]) { continuation.resume(returning: $0) }
                }
                if Task.isCancelled { return }
                if let self, self.view.stableKey != next.stableKey { self.view = next }
                do { try await Task.sleep(for: .milliseconds(200)) } catch { return }
            }
        }
    }
    func hide(_ id: UUID) {
        viewers.remove(id)
        if viewers.isEmpty { task?.cancel(); task = nil }
    }
}
