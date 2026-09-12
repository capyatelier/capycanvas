import Foundation

/// Serializes shared window requests. Native adapters report the actual result;
/// a requested mode must never masquerade as an observed window state.
@MainActor final class WindowPresentation {
    typealias Completion = (Bool, String?) -> Void
    typealias Change = (Bool, @escaping Completion) -> Void
    private weak var store: EditorStore?
    private var active: UInt64?
    private var completed: UInt64 = 0
    private var observedFullscreen: Bool?
    var changeFullscreen: Change? {
        didSet { if let store { receive(store.state.json) } }
    }
    init(store: EditorStore) { self.store = store }

    func receive(_ state: JSON) {
        guard active == nil, let changeFullscreen,
            let request = state["requests"].array.first(where: {
                $0["kind"]["type"].string == "set_fullscreen" && $0["id"].uint > completed
            }) else { return }
        let id = request["id"].uint
        active = id
        changeFullscreen(request["kind"]["fullscreen"].bool) { [weak self] actual, error in
            guard let self, self.active == id else { return }
            self.completed = id; self.active = nil
            self.observe(fullscreen: actual)
            self.store?.dispatch(["type": "complete_request", "id": id, "error": error as Any? ?? NSNull()])
            if let error { self.store?.failure = error }
            if let store = self.store { self.receive(store.state.json) }
        }
    }

    /// Also called for native controls, shortcuts, and restored window modes.
    func observe(fullscreen: Bool) {
        guard let store, observedFullscreen != fullscreen else { return }
        observedFullscreen = fullscreen
        store.dispatch(["type": "window_fullscreen", "fullscreen": fullscreen])
    }
}
