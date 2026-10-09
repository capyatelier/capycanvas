import SwiftUI

/// Presents the shared Rust workspace controller for one editor window. Rust
/// runs it on the drawing owner; SQLite stays on its storage worker.
@MainActor final class WorkspaceController: ObservableObject {
    @Published private(set) var view = JSON()
    @Published var formName = ""
    @Published var formDescription = ""
    @Published var formChoice = ""
    @Published private var dismissedError: String?
    private(set) weak var store: EditorStore?
    private var promptKey = ""
    private var focusKey = ""
    private var sent = 0
    private var closeWaiters: [(Int, @MainActor (Bool) -> Void)] = []
    private var detachWaiters: [(Int, @MainActor () -> Void)] = []
    private var flushWaiters: [(Int, @MainActor (Bool) -> Void)] = []
    private static let preferencesChanged = Notification.Name("art.capycanvas.workspace-preferences-changed")
    private var observer: NSObjectProtocol?
    var ready: Bool { view["ready"].bool }
    var busy: Bool { view["busy"].bool }
    var switcherBusy: Bool { view["switcher_busy"].bool }
    var readOnly: Bool { view["owner_lost"].bool || view["closing"].bool || view["closed"].bool }
    var error: String? { view["error"].isNull ? nil : view["error"].string }
    var visibleError: String? { error == dismissedError ? nil : error }
    func dismissError() { dismissedError = error }
    var presented: Bool { !view["page"].isNull || !view["prompt"].isNull }
    var page: String { view["page"].string }
    var hasUnsavedChanges: Bool { !ready || busy || view["dirty"].bool || view["saving"].bool }

    init(store: EditorStore, directory: URL, scene: String) {
        self.store = store
        observer = NotificationCenter.default.addObserver(forName: Self.preferencesChanged, object: nil, queue: .main) { [weak self] event in
            MainActor.assumeIsolated {
                guard let self, event.object as AnyObject? !== self else { return }
                self.send(["type": "refresh_switcher"])
            }
        }
        send(["type": "start", "directory": directory.path, "scene": UUID(uuidString: scene)?.uuidString ?? "default"])
        Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] timer in
            MainActor.assumeIsolated {
                guard let self, !self.view["closed"].bool else { timer.invalidate(); return }
                self.tick()
            }
        }
    }
    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }
    func receiveLanguage(_ next: JSON) { view = next }
    func tick() { send(["type": "tick"]) }
    @discardableResult func send(_ input: [String: Any]) -> Int {
        if input["type"] as? String == "retry" { dismissedError = nil }
        sent += 1
        let number = sent
        guard let native = store?.native else { return number }
        native.workspace(JSON(input)) { [weak self] reply, error in
            DispatchQueue.main.async { self?.receive(number, reply, error) }
        }
        return number
    }
    /// Row, details and menu buttons carry a ManagerAction; switcher menus carry a preference edit.
    func activate(_ action: JSON) {
        if action["type"].string == "edit_switcher" { send(["type": "edit_switcher", "edit": action["edit"].raw]) }
        else { send(["type": "action", "action": action.raw]) }
    }
    func form(_ action: [String: Any]) { send(["type": "form", "action": action]) }
    func open(_ page: String) { send(["type": "open", "page": page]) }
    func select(_ id: String?) { send(["type": "select", "id": id as Any? ?? NSNull()]) }
    func search(_ query: String) { send(["type": "search", "query": query]) }
    func switchTo(_ id: String) { send(["type": "switch", "id": id]) }
    func dismiss() { if presented { send(["type": "dismiss"]) } }
    func cancelPrompt() { send(["type": "cancel"]) }
    func submit() {
        var input: [String: Any] = ["type": "submit", "name": formName]
        if !view["prompt"]["description"].isNull { input["description"] = formDescription }
        if !view["prompt"]["choices"].array.isEmpty { input["choice"] = formChoice }
        send(input)
    }
    func openURL(_ url: URL, kind: WorkspacePackageKind) {
        Task { [weak self] in
            do {
                let text = try await WorkspacePackageFiles.read(from: url)
                self?.send(["type": "import", "kind": kind.rawValue, "text": text])
            } catch { self?.send(["type": "focus_failed", "error": error.localizedDescription]) }
        }
    }
    func suspend() { send(["type": "suspend"]) }
    func resume() { send(["type": "resume"]) }
    func refreshSwitcher() { send(["type": "refresh_switcher"]) }
    /// Completes true once the claim is released after a final save, or false
    /// when saving fails and the window must stay open for recovery.
    func close(_ completion: @escaping @MainActor (Bool) -> Void) {
        if view["closed"].bool || store?.native == nil { completion(true); return }
        closeWaiters.append((send(["type": "close"]), completion))
    }
    func keepOpen() { send(["type": "resume"]) }
    /// Completes true once accepted edits are stored, or false after a storage error.
    func flush(_ completion: @escaping @MainActor (Bool) -> Void) {
        if view["closed"].bool || store?.native == nil { completion(true); return }
        flushWaiters.append((send(["type": "tick"]), completion))
    }
    /// Release this window's claims without saving; completes once released.
    func detach(_ completion: @escaping @MainActor () -> Void = {}) {
        if view["closed"].bool || store?.native == nil { completion(); return }
        detachWaiters.append((send(["type": "detach"]), completion))
    }
    private func receive(_ number: Int, _ reply: JSON?, _ failure: String?) {
        guard let reply, !reply["view"].isNull else {
            if let failure, view["error"].isNull { view = view.replacing("error", with: JSON(failure)) }
            settle(number, transportFailed: failure != nil)
            return
        }
        if reply["wake"].bool { store?.wake?() }
        if !SnapshotProjection.equal(view.raw, reply["view"].raw) { present(reply["view"]) }
        settle(number)
    }
    private func present(_ next: JSON) {
        let wasReady = view["ready"].bool, revision = view["switcher_revision"].uint
        if view["error"].string != next["error"].string { dismissedError = nil }
        view = next
        let key = next["prompt"].isNull ? "" : next["prompt_action"].stableKey + next["prompt"].stableKey
        if key != promptKey {
            promptKey = key
            formName = next["prompt"]["name"].string
            formDescription = next["prompt"]["description"].string
            formChoice = next["prompt"]["selected"].string
        }
        focus(next["focus_window"])
        if next["ready"].bool && !wasReady {
            store?.native?.workspaceDidInitialize()
            store?.projectFiles.submitExternalOpen()
        } else if wasReady && revision != next["switcher_revision"].uint {
            NotificationCenter.default.post(name: Self.preferencesChanged, object: self)
        }
    }
    private func settle(_ number: Int, transportFailed: Bool = false) {
        let closed = view["closed"].bool, failed = transportFailed || !view["busy"].bool && !view["error"].isNull
        let stored = view["ready"].bool && !view["busy"].bool && !view["dirty"].bool && !view["saving"].bool
        func take<T>(_ waiters: inout [(Int, T)], when done: Bool) -> [T] {
            guard done else { return [] }
            let due = waiters.filter { $0.0 <= number }.map(\.1)
            waiters.removeAll { $0.0 <= number }
            return due
        }
        take(&closeWaiters, when: closed || failed).forEach { $0(closed) }
        take(&detachWaiters, when: closed || transportFailed).forEach { $0() }
        take(&flushWaiters, when: closed || failed || stored).forEach { $0(closed || !failed) }
    }
    private func focus(_ target: JSON) {
        let key = target.isNull ? "" : target["owner"].string + "|" + target["id"].string
        guard key != focusKey else { return }
        focusKey = key
        guard !target.isNull else { return }
        if let window = EditorStore.workspaceOwner(target["owner"].string), let focus = window.focusWindow { focus() }
        else {
            send(["type": "focus_failed",
                "error": "This workspace is open in another application window. Use that window or try again after it closes."])
        }
    }
}
