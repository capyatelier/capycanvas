import SwiftUI

struct WorkspaceRowFrame: Equatable {
    var row: CGRect = .zero
    var grip: CGRect = .zero
    var options: CGRect = .zero
}
struct WorkspaceRowFrames: PreferenceKey {
    static var defaultValue: [String: WorkspaceRowFrame] { [:] }
    static func reduce(value: inout [String: WorkspaceRowFrame], nextValue: () -> [String: WorkspaceRowFrame]) {
        value.merge(nextValue()) { old, next in
            WorkspaceRowFrame(row: next.row == .zero ? old.row : next.row,
                grip: next.grip == .zero ? old.grip : next.grip,
                options: next.options == .zero ? old.options : next.options)
        }
    }
}
struct WorkspaceRowMeasurement: ViewModifier {
    let id: String
    var part: WritableKeyPath<WorkspaceRowFrame, CGRect> = \.row
    func body(content: Content) -> some View {
        content.background(GeometryReader { geometry in
            let frame = geometry.frame(in: .named("workspace-manager-rows"))
            let value = { var value = WorkspaceRowFrame(); value[keyPath: part] = frame; return value }()
            Color.clear.preference(key: WorkspaceRowFrames.self, value: [id: value])
        })
    }
}

/// Native contact ownership and measured list geometry. A drop sends one shared
/// preference edit; no layout history or workspace capture is changed here.
@MainActor final class WorkspaceRowInteraction: ObservableObject, NativeReorderModel {
    struct Drag { let id: String; let origin: CGPoint; let bounds: CGRect; var point: CGPoint }
    struct Hint { let before: String?; let y: CGFloat }
    let contact = ReorderContact()
    @Published private(set) var menu: String?
    @Published private(set) var drag: Drag?
    @Published private(set) var hint: Hint?
    @Published private(set) var nativeDragging = false
    var frames: [String: WorkspaceRowFrame] = [:]
    var viewport = CGRect.zero
    var enabled = true
    var items: [JSON] = []
    var commit: (String, String?) -> Void = { _, _ in }
    var activate: (JSON) -> Void = { _ in }
    func nativeDragChanged(_ active: Bool) { if nativeDragging != active { nativeDragging = active } }
    func nativeMenu(at point: CGPoint) -> NativeReorderMenu? {
        guard enabled, viewport.contains(point),
              let (id, frame) = frames.first(where: { $0.value.row.contains(point) }),
              let row = items.first(where: { $0["id"].string == id }) else { return nil }
        let sections = [row["switcher_actions"].array,
            row["actions"].array.filter { !$0["primary"].bool }].map { section in
                section.map { item in
                    ["label": item["label"].raw, "enabled": item["enabled"].raw,
                     "selected": item["checked"].raw, "action": item["action"].raw]
                }
            }
        return NativeReorderMenu(id: id, bounds: frame.row,
            content: AppleContextMenu(JSON(["sections": sections])) { [weak self] action in
                guard let self, enabled, items.contains(where: { $0["id"].string == id }) else { return }
                closeMenu(); activate(action)
            })
    }
    func update(items: [JSON], enabled: Bool) {
        self.items = items; self.enabled = enabled
        if contact.target != nil { _ = contact.validate() }
        if let menu, !enabled || !items.contains(where: { $0["id"].string == menu }) { closeMenu() }
    }
    func showMenu(_ id: String, at point: CGPoint? = nil) {
        guard enabled, items.contains(where: { $0["id"].string == id }) else { return }
        menu = id
    }
    func closeMenu() { if menu != nil { menu = nil } }
    func cancel() {
        contact.cancel(); nativeDragChanged(false)
        if drag != nil { drag = nil }; if hint != nil { hint = nil }; closeMenu()
    }
    func acceptsContext(at point: CGPoint) -> Bool {
        enabled && viewport.contains(point) && frames.values.contains { $0.row.contains(point) }
    }
    func source(at point: CGPoint) -> ReorderTarget? {
        guard enabled, viewport.contains(point) else { closeMenu(); return nil }
        closeMenu()
        guard let (id, frame) = frames.first(where: { $0.value.row.contains(point) }),
            items.contains(where: { $0["id"].string == id }), !frame.options.contains(point) else { return nil }
        return ReorderTarget(id: id, surface: frame.grip.contains(point) ? .handle : .row,
            valid: { [weak self] _ in self?.enabled == true && self?.items.contains(where: { $0["id"].string == id }) == true },
            openContext: { [weak self] in self?.showMenu(id, at: point) },
            closeContext: { [weak self] in self?.closeMenu() },
            begin: { [weak self] origin in self?.drag = Drag(id: id, origin: origin, bounds: frame.row, point: origin) },
            move: { [weak self] point in self?.move(point) },
            finish: { [weak self] point in
                guard let self else { return }
                move(point)
                let before = hint
                drag = nil; hint = nil
                if let before { commit(id, before.before) }
            },
            cancel: { [weak self] in self?.drag = nil; self?.hint = nil; self?.nativeDragChanged(false) })
    }
    func context(at point: CGPoint) {
        guard !contact.dragging, viewport.contains(point),
            let id = frames.first(where: { $0.value.row.contains(point) })?.key else { return }
        showMenu(id, at: point)
    }
    func move(_ point: CGPoint) {
        guard var drag else { return }
        drag.point = point; self.drag = drag
        let rows = items.compactMap { item -> (String, CGRect)? in
            let id = item["id"].string
            guard let frame = frames[id]?.row else { return nil }
            return (id, frame)
        }.sorted { $0.1.minY < $1.1.minY }
        guard viewport.contains(point), let last = rows.last else { hint = nil; return }
        if let next = rows.first(where: { point.y <= $0.1.midY }) {
            hint = Hint(before: next.0, y: next.1.minY)
        } else {
            // Lazy stacks need not mount the next row yet. Dropping below the
            // last measured row still inserts before its next item in the list.
            let index = items.firstIndex { $0["id"].string == last.0 }!
            let next = index + 1 < items.count ? items[index + 1]["id"].string : nil
            hint = Hint(before: next, y: last.1.maxY)
        }
    }
}
