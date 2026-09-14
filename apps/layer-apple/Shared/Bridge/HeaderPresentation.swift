import SwiftUI

/// Native measurements and contact ownership. Shared Rust retains the drag's
/// frozen geometry and applies its single release action on the serial owner.
@MainActor final class HeaderPresentation: ObservableObject, NativeReorderModel {
    let contact = ReorderContact()
    var viewport = CGRect.zero
    @Published private(set) var geometry = JSON()
    @Published private(set) var preview = JSON()
    @Published private(set) var heldSource = JSON()
    @Published var selected: UInt64?
    @Published private(set) var overflowZone: Int?
    @Published private(set) var menu = JSON()
    private(set) var menuAnchor = CGRect.zero
    var sources: [String: HeaderSource] = [:] { didSet { if contact.target != nil { contact.validate() } } }
    var overflowSources: [String: HeaderSource] = [:] { didSet { if contact.target != nil { contact.validate() } } }
    private weak var store: EditorStore?
    private var layout = JSON()
    private var layoutKey = ""
    private var measurementKey = ""
    private let popup = UUID()
    private var menuRequest = UUID()
    private var querying = false
    private struct Drag {
        let id = UUID()
        var point: CGPoint
        var released = false
    }
    private var drag: Drag?
    init(store: EditorStore) { self.store = store }
    var enabled: Bool {
        guard let store else { return false }
        return store.snapshot["header"]["editing"].bool && store.failure == nil
            && !store.workspaceManager.presented && !store.workspace.hasPopover(excluding: popup)
            && ["picker", "toolbar_prompt", "toolbar_manager", "preferences"].allSatisfy { store.snapshot[$0].isNull }
    }
    func reconcile(_ specification: JSON) {
        let key = specification.stableKey
        guard key != layoutKey else { return }
        cancel(); layoutKey = key; layout = specification
        if let selected, !specification["model"]["zones"].array.flatMap(\.array).contains(where: { $0["id"].uint == selected }) {
            self.selected = nil
        }
        query(specification["request"].object) { [weak self] value in
            guard let self, self.layoutKey == key, !value.isNull else { return }
            self.geometry = value
            var items = value["items"].array.map(\.raw)
            // Hidden tools open drawers from their visible overflow control.
            for zone in 0..<3 where !value["overflow"][zone].isNull {
                items += value["hidden"][zone].array.map { ["id": $0.raw, "bounds": value["overflow"][zone].raw] }
            }
            let measurement = JSON(["type":"measure_header", "height":specification["height"].raw, "items":items])
            guard measurement.stableKey != self.measurementKey else { return }
            self.measurementKey = measurement.stableKey
            self.store?.dispatch(measurement)
        }
    }
    func action(_ value: [String: Any]) { store?.customize(["type":"header", "action":value]) }
    func step(forward: Bool) {
        guard let selected, enabled else { return }
        store?.headerAction(["op":"step", "id":selected, "forward":forward]) {}
    }
    func removeSelected() { if let selected, enabled { action(["type":"remove", "id":selected]) } }
    func showOverflow(_ zone: Int) {
        closeMenu(); overflowZone = zone; store?.workspace.popover(popup, open: true)
    }
    func closeOverflow() {
        guard overflowZone != nil else { return }
        overflowZone = nil; overflowSources = [:]; store?.workspace.popover(popup, open: false)
    }
    func source(at point: CGPoint) -> ReorderTarget? {
        guard enabled, viewport.contains(point), menu.isNull,
              let (id, source) = (overflowZone == nil ? sources : overflowSources).first(where: { $0.value.bounds.contains(point) }) else { return nil }
        let key = layoutKey
        return ReorderTarget(id: id, surface: .headerEditor,
            valid: { [weak self] dragging in
                guard let self, self.enabled, self.layoutKey == key else { return false }
                return dragging || self.sources[id] == source || self.overflowSources[id] == source
            },
            openContext: { [weak self] in self?.showMenu(source) },
            closeContext: { [weak self] in self?.closeMenu() },
            begin: { [weak self] in self?.begin(source, at: $0) },
            move: { [weak self] in self?.move($0) },
            finish: { [weak self] in self?.finish($0) },
            cancel: { [weak self] in self?.cancelDrag() })
    }
    func acceptsContext(at point: CGPoint) -> Bool {
        enabled && menu.isNull && sources.values.contains { $0.bounds.contains(point) && $0.value["kind"].string == "item" }
    }
    func context(at point: CGPoint) {
        guard acceptsContext(at: point), let source = sources.values.first(where: { $0.bounds.contains(point) }) else { return }
        showMenu(source)
    }
    func showMenu(_ source: HeaderSource) {
        guard source.value["kind"].string == "item", let store else { return }
        let id = source.value["value"].uint
        selected = id; menuAnchor = source.bounds
        let token = UUID(); menuRequest = token
        store.query(["type":"context", "target":["kind":"header", "id":id]]) { [weak self] value in
            guard let self, self.menuRequest == token, self.enabled else { return }
            self.menu = value; store.workspace.popover(self.popup, open: !value.isNull)
        }
    }
    func closeMenu() {
        menuRequest = UUID()
        if !menu.isNull { menu = JSON(); store?.workspace.popover(popup, open: false) }
    }
    func cancel() { contact.cancel(); cancelDrag(); closeMenu(); closeOverflow() }
    private func query(_ value: [String: Any], completion: @escaping @MainActor (JSON) -> Void) {
        store?.query(["type":"header", "request":value], completion: completion)
    }
    private func begin(_ source: HeaderSource, at point: CGPoint) {
        closeMenu(); closeOverflow(); heldSource = source.value
        if heldSource["kind"].string == "item" { selected = heldSource["value"].uint }
        let operation = Drag(point: point); drag = operation
        var request = layout["request"].object
        request["op"] = "begin"; request["source"] = source.value.raw
        request["press"] = [point.x, point.y]; request["grab"] = JSON(source.bounds).raw
        query(request) { [weak self] accepted in
            guard let self, self.drag?.id == operation.id else { return }
            if !accepted.bool { self.cancel() }
        }
    }
    private func move(_ point: CGPoint) {
        guard drag != nil else { return }
        drag?.point = point; updatePreview()
    }
    private func updatePreview() {
        guard !querying, let operation = drag, !operation.released else { return }
        querying = true
        query(["op":"preview", "position":[operation.point.x, operation.point.y]]) { [weak self] value in
            guard let self else { return }; self.querying = false
            guard self.drag?.id == operation.id, self.drag?.released == false else { return }
            self.preview = value
            if self.drag?.point != operation.point { self.updatePreview() }
        }
    }
    private func finish(_ point: CGPoint) {
        guard let operation = drag else { return }
        drag?.released = true
        store?.headerAction(["op":"finish", "position":[point.x, point.y], "cancel":false]) { [weak self] in
            guard let self, self.drag?.id == operation.id else { return }
            self.drag = nil; self.preview = JSON(); self.heldSource = JSON()
        }
    }
    private func cancelDrag() {
        guard drag != nil else { return }
        drag = nil; preview = JSON(); heldSource = JSON()
        query(["op":"finish", "position":[0,0], "cancel":true]) { _ in }
    }
}

struct HeaderSource: Equatable {
    let source: String
    let bounds: CGRect
    var value: JSON { (try? JSON.decode(source)) ?? JSON() }
}
struct HeaderSources: PreferenceKey {
    static let defaultValue: [String: HeaderSource] = [:]
    static func reduce(value: inout [String: HeaderSource], nextValue: () -> [String: HeaderSource]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}
struct HeaderSourceMeasurement: ViewModifier {
    let source: JSON
    @State private var id = UUID().uuidString
    func body(content: Content) -> some View {
        content.background(GeometryReader { geometry in
            Color.clear.preference(key: HeaderSources.self,
                value: [id: HeaderSource(source: source.stableKey, bounds: geometry.frame(in: .named("editor-workspace")))])
        })
    }
}
