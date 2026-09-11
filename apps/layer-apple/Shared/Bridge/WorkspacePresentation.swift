import SwiftUI

/// Native measurements and input only. Rust resolves placement, animation
/// interpolation, drop eligibility and the action applied at release.
@MainActor final class WorkspacePresentation: ObservableObject {
    @Published private(set) var expansion = JSON()
    @Published private(set) var dropHint = JSON()
    private weak var store: EditorStore?
    private var shownPanel: String?
    private var configurationHeight = 0.0
    private var geometryKey = ""
    private var animation: Task<Void, Never>?
    var tabs: [JSON] = []
    var sources: [String: CGRect] = [:]
    private struct Drag {
        let token = UUID()
        let item: JSON
        var point: CGPoint
        var released = false
    }
    private var drag: Drag?
    private var queryingDrop = false
    init(store: EditorStore) { self.store = store }
    func refresh() {
        guard let store else { return }
        let desired = store.state["customization"]["expanded"]
        guard !desired.isNull || shownPanel != nil else { return }
        let key = JSON([desired.raw, store.snapshot["layout"].raw, configurationHeight]).stableKey
        guard geometryKey != key else { return }; geometryKey = key
        animation?.cancel()
        let closing = desired.isNull
        let panel = closing ? shownPanel! : desired.string
        let from = panel == shownPanel ? expansion : JSON()
        shownPanel = panel
        let duration = max(0.001, store.catalog["panel_expansion_ms"].number / 1000)
        animation = Task { [weak self] in
            let start = ProcessInfo.processInfo.systemUptime
            while !Task.isCancelled {
                guard let self, let store = self.store else { return }
                let fraction = min(1, (ProcessInfo.processInfo.systemUptime - start) / duration)
                let query: [String: Any] = ["type": "expansion", "panel": panel,
                    "heights": [0, self.configurationHeight], "progress": fraction,
                    "from": from.raw, "closing": closing]
                let next: JSON = await withCheckedContinuation { continuation in
                    store.query(query) { continuation.resume(returning: $0) }
                }
                guard !Task.isCancelled else { return }
                self.expansion = next
                if fraction == 1 || next.isNull {
                    if closing || next.isNull { self.shownPanel = nil; self.expansion = JSON() }
                    self.animation = nil; return
                }
                do { try await Task.sleep(for: .milliseconds(16)) } catch { return }
            }
        }
    }
    func measureConfiguration(_ height: Double) {
        guard height.isFinite, abs(height - configurationHeight) > 0.5 else { return }
        configurationHeight = height; refresh()
    }
    func source(at point: CGPoint) -> JSON? {
        sources.filter { $0.value.contains(point) }
            .min { $0.value.width * $0.value.height < $1.value.width * $1.value.height }
            .flatMap { try? JSON.decode($0.key) }
    }
    private func send(_ operation: Drag, phase: String) {
        guard operation.item["kind"].string != "tile", let store else { return }
        if !operation.item["type"].isNull {
            var action = operation.item.object
            action["phase"] = phase; action["position"] = [operation.point.x, operation.point.y]
            action["viewport"] = store.snapshot["layout"]["viewport"].raw
            store.dispatch(action); return
        }
        store.dispatch(["type": "drag_workspace", "item": operation.item.raw, "phase": phase,
            "position": [operation.point.x, operation.point.y], "viewport": store.snapshot["layout"]["viewport"].raw,
            "tabs": tabs.map(\.raw)])
    }
    func start(_ item: JSON, point: CGPoint) {
        if let drag { cancel(drag.item) }
        let operation = Drag(item: item, point: point); drag = operation
        if item["kind"].string != "tile" {
            animation?.cancel(); animation = nil; expansion = JSON(); shownPanel = nil; geometryKey = ""
        }
        send(operation, phase: "down")
    }
    func move(_ item: JSON, point: CGPoint, released: Bool = false) {
        guard drag?.item.stableKey == item.stableKey else { return }
        drag?.point = point; drag?.released = released
        if let current = drag, current.item["kind"].string != "tile" {
            send(current, phase: released ? "up" : "move")
            if released { drag = nil; dropHint = JSON(); return }
        }
        if item["type"].isNull { queryDrop() }
    }
    func cancel(_ item: JSON) {
        if let current = drag, current.item.stableKey == item.stableKey {
            send(current, phase: "cancel"); drag = nil; dropHint = JSON()
        }
    }
    private func queryDrop() {
        guard let store, let request = drag, request.item["type"].isNull, !queryingDrop else { return }
        queryingDrop = true
        store.query(["type": "drop", "item": request.item.raw,
            "position": [request.point.x, request.point.y], "tabs": tabs.map(\.raw), "expansion": expansion.raw]) { [weak self] result in
            guard let self else { return }
            self.queryingDrop = false
            guard let current = self.drag, current.token == request.token else { self.queryDrop(); return }
            guard current.point == request.point else { self.queryDrop(); return }
            if current.released {
                self.drag = nil; self.dropHint = JSON()
                if !result["action"].isNull { store.dispatch(result["action"]) }
            } else { self.dropHint = result }
        }
    }
}

/// Source views only register geometry. The gesture belongs to the workspace
/// root so a tab tearing off into a new floating group cannot destroy its input.
struct WorkspaceDrag: ViewModifier {
    let workspace: WorkspacePresentation
    let item: JSON
    @Environment(\.workspaceGesturesEnabled) private var enabled
    func body(content: Content) -> some View {
        content.background(GeometryReader { allocation in
            Color.clear.preference(key: WorkspaceSources.self,
                value: enabled ? [item.stableKey: allocation.frame(in: .named("editor-workspace"))] : [:])
        })
    }
}
struct WorkspaceRootDrag: ViewModifier {
    @ObservedObject var workspace: WorkspacePresentation
    @GestureState private var contact = false
    @State private var item: JSON?
    @State private var evaluated = false
    func body(content: Content) -> some View {
        content.simultaneousGesture(DragGesture(minimumDistance: 6, coordinateSpace: .named("editor-workspace"))
            .updating($contact) { _, value, _ in value = true }
            .onChanged { event in
                if !evaluated {
                    evaluated = true
                    if let source = workspace.source(at: event.startLocation) {
                        item = source; workspace.start(source, point: event.startLocation)
                    }
                }
                if let item { workspace.move(item, point: event.location) }
            }
            .onEnded { event in
                evaluated = false
                guard let source = item else { return }; item = nil
                workspace.move(source, point: event.location, released: true)
            })
            .onChange(of: contact) { _, touching in
                if !touching {
                    DispatchQueue.main.async { evaluated = false; if let source = item { item = nil; workspace.cancel(source) } }
                }
            }
            .onDisappear { if let source = item { item = nil; workspace.cancel(source) } }
    }
}
private struct WorkspaceGesturesEnabled: EnvironmentKey { static let defaultValue = true }
extension EnvironmentValues {
    var workspaceGesturesEnabled: Bool {
        get { self[WorkspaceGesturesEnabled.self] }
        set { self[WorkspaceGesturesEnabled.self] = newValue }
    }
}
struct WorkspaceSources: PreferenceKey {
    static var defaultValue: [String: CGRect] { [:] }
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) { value.merge(nextValue(), uniquingKeysWith: { _, next in next }) }
}

struct WorkspaceTabs: PreferenceKey {
    static var defaultValue: [String: CGRect] { [:] }
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) { value.merge(nextValue(), uniquingKeysWith: { _, next in next }) }
}
