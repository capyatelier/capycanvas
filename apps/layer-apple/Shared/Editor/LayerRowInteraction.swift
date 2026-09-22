import SwiftUI

struct LayerRowFrame: Equatable {
    var row: CGRect = .zero
    var root: CGRect = .zero
    var grip: CGRect = .zero
    var name: CGRect = .zero
    var mask: CGRect = .zero
}
struct LayerRowFrames: PreferenceKey {
    static var defaultValue: [UInt64: LayerRowFrame] { [:] }
    static func reduce(value: inout [UInt64: LayerRowFrame], nextValue: () -> [UInt64: LayerRowFrame]) {
        value.merge(nextValue()) { old, next in
            LayerRowFrame(row: next.row == .zero ? old.row : next.row,
                root: next.root == .zero ? old.root : next.root,
                grip: next.grip == .zero ? old.grip : next.grip,
                name: next.name == .zero ? old.name : next.name,
                mask: next.mask == .zero ? old.mask : next.mask)
        }
    }
}
struct LayerRowMeasurement: ViewModifier {
    let id: UInt64
    var part: WritableKeyPath<LayerRowFrame, CGRect> = \.row
    var enabled = true
    @ViewBuilder func body(content: Content) -> some View {
        if enabled {
        content.background(GeometryReader { geometry in
            let frame = geometry.frame(in: .named("layer-rows"))
            let value = {
                var value = LayerRowFrame(); value[keyPath: part] = frame
                if part == \.row { value.root = geometry.frame(in: .named("editor-workspace")) }
                return value
            }()
            Color.clear.preference(key: LayerRowFrames.self, value: [id: value])
        })
        } else { content }
    }
}
enum LayerMenuSource: Equatable { case row(UInt64), footer }

/// One revealed native row per editor, including retained drawer instances.
@MainActor final class LayerSwipe: ObservableObject {
    @Published var owner: UUID?
    @Published var layer: UInt64?
    @Published var offset: CGFloat = 0
    @Published var tracking = false
    var bounds = CGRect.zero
    func close() { owner = nil; layer = nil; offset = 0; tracking = false }
    func contact(at point: CGPoint) { if owner != nil && !bounds.contains(point) { close() } }
}

/// Native pickup and measured feedback only. Rust owns context selection,
/// menu capabilities, hierarchy edits and the single completed drop transaction.
@MainActor final class LayerRowInteraction: ObservableObject, NativeReorderModel {
    struct Drag {
        let id: UInt64
        let bounds: CGRect
        let origin: CGPoint
        var point: CGPoint
        var target: UInt64?
        var fraction: Double = 0
    }
    weak var store: EditorStore?
    let contact = ReorderContact()
    let swipeOwner = UUID()
    private var swipeOrigin = CGPoint.zero
    private var swipeStart: CGFloat = 0
    private var contactLayer: UInt64?
    var frames: [UInt64: LayerRowFrame] = [:]
    var viewport: CGRect = .zero
    @Published private(set) var drag: Drag?
    @Published private(set) var menu = JSON()
    @Published private(set) var menuSource: LayerMenuSource?
    private var menuRequest = UUID()
    private var layers: [JSON] { store?.state["layers"].array ?? [] }
    private var epoch: UInt64 { store?.state["document_file"]["epoch"].uint ?? 0 }
    private var renaming: UInt64? {
        guard let value = store?.state["layer_tools"]["rename_layer"], !value.isNull else { return nil }
        return value.uint
    }
    var enabled: Bool { store != nil && !layers.isEmpty }
    private func row(at point: CGPoint) -> JSON? {
        guard enabled, viewport.contains(point),
              let row = layers.first(where: { frames[$0["id"].uint]?.row.contains(point) == true }) else { return nil }
        if renaming == row["id"].uint && frames[row["id"].uint]?.name.contains(point) == true { return nil }
        return row
    }
    private func identity(_ id: UInt64) -> String { "\(epoch):layer:\(id)" }
    func source(at point: CGPoint) -> ReorderTarget? {
        guard let row = row(at: point), let frame = frames[row["id"].uint] else { return nil }
        closeMenu()
        let id = row["id"].uint, currentEpoch = epoch
        contactLayer = id
        let mask = row["has_mask"].bool && frame.mask.contains(point)
        return ReorderTarget(id: identity(id), surface: frame.grip.contains(point) ? .handle : .row,
            canDrag: row["can_drop_below"].bool,
            valid: { [weak self] _ in
                guard let self else { return false }
                return enabled && epoch == currentEpoch && renaming != id && layers.contains { $0["id"].uint == id }
            }, openContext: { [weak self] in self?.openMenu(id: id, mask: mask) },
            closeContext: { [weak self] in self?.closeMenu() },
            begin: { [weak self] origin in
                self?.store?.layerSwipe.close()
                self?.drag = Drag(id: id, bounds: frame.row, origin: origin, point: origin)
            },
            move: { [weak self] point in self?.move(point) },
            finish: { [weak self] point in
                guard let self else { return }
                move(point); let completed = drag; drag = nil
                if let completed, let target = completed.target {
                    store?.layer(["op": "drop", "id": id, "target": target, "fraction": completed.fraction])
                }
            }, cancel: { [weak self] in self?.drag = nil })
    }
    var swiping: Bool { store?.layerSwipe.owner == swipeOwner && store?.layerSwipe.tracking == true }
    func beginSwipe(at point: CGPoint) -> Bool {
        guard let store, contact.device != .mouse, contact.target?.surface == .row, !contact.held,
              let id = contactLayer, let row = layers.first(where: { $0["id"].uint == id }), row["can_delete"].bool,
              let frame = frames[id] else { return false }
        let delta = CGPoint(x: point.x - contact.origin.x, y: point.y - contact.origin.y)
        let swipe = store.layerSwipe
        let start = swipe.owner == swipeOwner && swipe.layer == id ? swipe.offset : 0
        guard abs(delta.x) > abs(delta.y), delta.x < 0 || start > 0 else { return false }
        swipeOrigin = contact.origin; swipeStart = start
        contact.suppressActivation(); closeMenu()
        swipe.owner = swipeOwner; swipe.layer = id; swipe.bounds = frame.root; swipe.tracking = true
        moveSwipe(to: point)
        return true
    }
    func moveSwipe(to point: CGPoint) {
        guard swiping, let swipe = store?.layerSwipe else { return }
        swipe.offset = min(72, max(0, swipeStart - (point.x - swipeOrigin.x)))
    }
    func finishSwipe(cancelled: Bool) {
        guard swiping, let swipe = store?.layerSwipe else { return }
        swipe.tracking = false
        if cancelled || swipe.offset < 72 * 0.4 { swipe.close() }
        else { swipe.offset = 72 }
    }
    private func move(_ point: CGPoint) {
        guard var drag else { return }
        drag.point = point; drag.target = nil; drag.fraction = 0
        if viewport.contains(point), let target = layers.first(where: {
            $0["id"].uint != drag.id && frames[$0["id"].uint]?.row.contains(point) == true
        }), let frame = frames[target["id"].uint]?.row, frame.height > 0 {
            drag.target = target["id"].uint
            drag.fraction = target["can_drop_below"].bool ? Double((point.y - frame.minY) / frame.height) : 0
        }
        self.drag = drag
    }
    func acceptsContext(at point: CGPoint) -> Bool { row(at: point) != nil }
    func context(at point: CGPoint) {
        guard !contact.dragging, let row = row(at: point) else { return }
        let id = row["id"].uint
        openMenu(id: id, mask: row["has_mask"].bool && frames[id]?.mask.contains(point) == true)
    }
    func openMenu(id: UInt64, mask: Bool, source: LayerMenuSource? = nil) {
        guard let store, layers.contains(where: { $0["id"].uint == id }) else { return }
        store.layerSwipe.close()
        menuSource = source ?? .row(id)
        let request = UUID(), currentEpoch = epoch; menuRequest = request
        store.layer(["op": "context", "id": id, "mask": mask])
        store.query(["type": "layer_menu", "id": id, "mask": mask]) { [weak self] result in
            guard let self, menuRequest == request, epoch == currentEpoch,
                  layers.contains(where: { $0["id"].uint == id }) else { return }
            menu = result
        }
    }
    func closeMenu() { menuRequest = UUID(); if menuSource != nil { menuSource = nil }; if !menu.isNull { menu = JSON() } }
    func validate() {
        // A document edit can remove the source while the pointer is stationary.
        // Retire it on publication, without waiting for a native move or release.
        if contact.target != nil, !contact.validate() { cancel() }
        if let swipe = store?.layerSwipe, swipe.owner == swipeOwner,
           !layers.contains(where: { $0["id"].uint == swipe.layer && $0["can_delete"].bool }) { swipe.close() }
    }
    func cancel() {
        contact.cancel(); if drag != nil { drag = nil }; closeMenu()
        if store?.layerSwipe.owner == swipeOwner { store?.layerSwipe.close() }
    }
}
