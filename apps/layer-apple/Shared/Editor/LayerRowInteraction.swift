import SwiftUI

struct LayerRowFrame: Equatable {
    var row: CGRect = .zero
    var root: CGRect = .zero
    var swipe: CGRect = .zero
    var grip: CGRect = .zero
    var name: CGRect = .zero
    var content: CGRect = .zero
    var mask: CGRect = .zero
}
struct LayerRowFrames: PreferenceKey {
    static var defaultValue: [UInt64: LayerRowFrame] { [:] }
    static func reduce(value: inout [UInt64: LayerRowFrame], nextValue: () -> [UInt64: LayerRowFrame]) {
        value.merge(nextValue()) { old, next in
            LayerRowFrame(row: next.row == .zero ? old.row : next.row,
                root: next.root == .zero ? old.root : next.root,
                swipe: next.swipe == .zero ? old.swipe : next.swipe,
                grip: next.grip == .zero ? old.grip : next.grip,
                name: next.name == .zero ? old.name : next.name,
                content: next.content == .zero ? old.content : next.content,
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
    private struct Row: Hashable { let owner: UUID; let layer: UInt64 }
    @Published private var returning: [Row: UUID] = [:]
    @Published var owner: UUID?
    @Published var layer: UInt64?
    @Published var offset: CGFloat = 0
    @Published var tracking = false
    var bounds = CGRect.zero
    func atRest(owner: UUID, layer: UInt64) -> Bool {
        returning[Row(owner: owner, layer: layer)] == nil && (self.owner != owner || self.layer != layer || offset == 0)
    }
    func progress(owner: UUID, layer: UInt64, revealWidth: CGFloat) -> CGFloat {
        guard self.owner == owner, self.layer == layer else { return 0 }
        return min(1, abs(offset) / (offset > 0 ? max(1, revealWidth) : 72))
    }
    func close() {
        let row = owner.flatMap { owner in layer.map { Row(owner: owner, layer: $0) } }
        let token = UUID()
        if let row, offset != 0 { returning[row] = token }
        withAnimation(.easeOut(duration: 0.15), completionCriteria: .removed) {
            owner = nil; layer = nil; offset = 0; tracking = false
        } completion: {
            if let row, self.returning[row] == token { self.returning.removeValue(forKey: row) }
        }
    }
    func contact(at point: CGPoint) { if owner != nil && !bounds.contains(point) { close() } }
}

/// Native pickup and measured feedback only. Rust owns context selection,
/// menu capabilities, hierarchy edits and the single completed drop transaction.
@MainActor final class LayerRowInteraction: ObservableObject, NativeLayerTouchModel {
    var layerTouchModel: (any NativeLayerTouchModel)? { self }
    private static var nextTouchSurface: UInt64 = 0
    let touchSurface: UInt64
    init() { Self.nextTouchSurface &+= 1; touchSurface = Self.nextTouchSurface }
    func touchRow(at point: CGPoint) -> UInt64? { row(at: point).map { $0["id"].uint } }
    func layerTouch(_ id: UInt64, phase: String, point: CGPoint, row: UInt64, timestamp: TimeInterval, scale: CGFloat) {
        store?.input(["type": "layer_touch", "surface": touchSurface, "id": id, "phase": phase,
            "position": [point.x * scale, point.y * scale], "row": row, "time_ns": UInt64(max(0, timestamp) * 1_000_000_000)])
    }
    struct DropHit: Equatable {
        let target: UInt64
        let fraction: Double
        let surface: String
    }
    struct Drag {
        let id: UInt64
        let bounds: CGRect
        let origin: CGPoint
        var point: CGPoint
        var hit: DropHit?
        var target: UInt64?
        var position: String?
        var effectOwner: UInt64?
    }
    weak var store: EditorStore?
    weak var scrollPosition: EditorScrollPosition?
    let contact = ReorderContact()
    let swipeOwner = UUID()
    private var swipeOrigin = CGPoint.zero
    private var swipeStart: CGFloat = 0
    private var swipeMinimum: CGFloat = 0
    private var swipeMaximum: CGFloat = 72
    private var contactLayer: UInt64?
    var frames: [UInt64: LayerRowFrame] = [:]
    var viewport: CGRect = .zero
    @Published private(set) var drag: Drag?
    @Published private(set) var menu = JSON()
    @Published private(set) var menuSource: LayerMenuSource?
    private var menuRequest = UUID()
    private var dropRequest = UUID()
    private var layers: [JSON] { store?.state["layers"].array ?? [] }
    private var epoch: UInt64 { store?.state["document_file"]["epoch"].uint ?? 0 }
    private var renaming: UInt64? { store?.state["layer_tools"]["rename_layer"].optionalUInt }
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
        closeMenu(); cancelDrop()
        let id = row["id"].uint, currentEpoch = epoch
        contactLayer = id
        let mask = row["has_mask"].bool && frame.mask.contains(point)
        let resting = store?.layerSwipe.atRest(owner: swipeOwner, layer: id) == true
        let grip = resting && frame.grip.contains(point)
        let pickup = { (row: JSON) in grip ? row["drag_from_handle"].bool : !row["drag_from_body"].array.isEmpty }
        let canDrag = resting && pickup(row) && (grip || ThumbnailSelectionLoad.current() == nil)
        return ReorderTarget(id: identity(id), surface: grip ? .handle : .row,
            canDrag: canDrag,
            dragDevices: grip ? [.mouse, .touch, .pen] : row["drag_from_body"].array.compactMap { ReorderDevice(rawValue: $0.string) },
            valid: { [weak self] _ in
                guard let self else { return false }
                return enabled && epoch == currentEpoch && renaming != id && layers.contains {
                    $0["id"].uint == id && (!row["can_drop_below"].bool || $0["can_drop_below"].bool) && (!canDrag || pickup($0))
                }
            }, openContext: { [weak self] in self?.openMenu(id: id, mask: mask) },
            closeContext: { [weak self] in self?.closeMenu() },
            begin: { [weak self] origin in
                self?.store?.layerSwipe.close()
                self?.drag = Drag(id: id, bounds: frame.row, origin: origin, point: origin)
            },
            move: { [weak self] point in self?.move(point) },
            finish: { [weak self] point in self?.finishDrop(point) },
            cancel: { [weak self] in self?.cancelDrop() })
    }
    var swiping: Bool { store?.layerSwipe.owner == swipeOwner && store?.layerSwipe.tracking == true }
    func beginSwipe(at point: CGPoint) -> Bool {
        guard let store, contact.device != .mouse, contact.target?.surface == .row, !contact.held,
              let id = contactLayer, let row = layers.first(where: { $0["id"].uint == id }),
              let frame = frames[id] else { return false }
        let delta = CGPoint(x: point.x - contact.origin.x, y: point.y - contact.origin.y)
        let swipe = store.layerSwipe
        let start = swipe.owner == swipeOwner && swipe.layer == id ? swipe.offset : 0
        let allowed = delta.x < 0 ? row["swipe_actions"].array.contains { $0["enabled"].bool } : start > 0 || !row["right_swipe"].isNull
        guard abs(delta.x) > abs(delta.y), allowed else { return false }
        swipeOrigin = contact.origin; swipeStart = start
        swipeMinimum = start == 0 && !row["right_swipe"].isNull ? -72 : 0
        swipeMaximum = min(CGFloat(row["swipe_actions"].array.count) * 72, frame.root.width)
        contact.suppressActivation(); closeMenu()
        swipe.owner = swipeOwner; swipe.layer = id; swipe.bounds = frame.root; swipe.tracking = true
        moveSwipe(to: point)
        return true
    }
    func moveSwipe(to point: CGPoint) {
        guard swiping, let swipe = store?.layerSwipe else { return }
        swipe.offset = min(swipeMaximum, max(swipeMinimum, swipeStart - (point.x - swipeOrigin.x)))
    }
    func finishSwipe(cancelled: Bool) {
        guard swiping, let store, let id = store.layerSwipe.layer else { return }
        let swipe = store.layerSwipe, toggle = !cancelled && swipe.offset <= -72 * 0.4
        swipe.tracking = false
        let threshold = swipeStart > 0 ? swipeStart - 72 * 0.4 : 72 * 0.4
        if cancelled || swipe.offset < threshold { swipe.close() }
        else { swipe.offset = swipeMaximum }
        if toggle, let row = layers.first(where: { $0["id"].uint == id }), !row["right_swipe"].isNull {
            store.layer(row["right_swipe"].object)
        }
    }
    private func hit(_ point: CGPoint, excluding id: UInt64) -> DropHit? {
        guard viewport.contains(point), let target = layers.first(where: {
            $0["id"].uint != id && frames[$0["id"].uint]?.row.contains(point) == true
        }), let frame = frames[target["id"].uint], frame.row.height > 0 else { return nil }
        return DropHit(target: target["id"].uint, fraction: Double((point.y - frame.row.minY) / frame.row.height),
            surface: frame.content.contains(point) ? "thumbnail" : "row")
    }
    private func move(_ point: CGPoint, refresh: Bool = false) {
        guard var drag, let store else { return }
        drag.point = point
        let hit = hit(point, excluding: drag.id)
        if !refresh && drag.hit == hit { self.drag = drag; return }
        drag.hit = hit; drag.target = nil; drag.position = nil; drag.effectOwner = nil; self.drag = drag
        let request = UUID(), currentEpoch = epoch; dropRequest = request
        guard let hit else { return }
        let id = drag.id
        store.query(["type": "layer_drop", "epoch": currentEpoch, "id": id, "target": hit.target,
            "fraction": hit.fraction, "surface": hit.surface]) { [weak self] result in
            guard let self, dropRequest == request, epoch == currentEpoch, result["epoch"].uint == currentEpoch,
                  var current = self.drag, current.id == id, current.hit == hit,
                  layers.contains(where: { $0["id"].uint == hit.target }) else { return }
            if !result["position"].isNull && !result["target"].isNull {
                current.target = result["target"].uint; current.position = result["position"].string
                current.effectOwner = result["effect_owner"].isNull ? nil : result["effect_owner"].uint
            }
            self.drag = current
        }
    }
    private func finishDrop(_ point: CGPoint) {
        guard let drag, let store else { return }
        let hit = hit(point, excluding: drag.id), id = drag.id
        cancelDrop()
        guard let hit else { return }
        let currentEpoch = epoch
        store.query(["type": "layer_drop", "epoch": currentEpoch, "id": id, "target": hit.target,
            "fraction": hit.fraction, "surface": hit.surface]) { [weak self] result in
            guard let self, epoch == currentEpoch, result["epoch"].uint == currentEpoch,
                  !result["position"].isNull, !result["target"].isNull,
                  layers.contains(where: { $0["id"].uint == id && $0["can_drop_below"].bool }),
                  layers.contains(where: { $0["id"].uint == hit.target }) else { return }
            store.layer(["op": "drop", "id": id, "target": hit.target, "fraction": hit.fraction, "surface": hit.surface])
        }
    }
    private func cancelDrop() { dropRequest = UUID(); if drag != nil { drag = nil } }
    func acceptsContext(at point: CGPoint) -> Bool { row(at: point) != nil }
    func context(at point: CGPoint) {
        guard !contact.dragging else { return }
        guard let row = row(at: point) else { return }
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
           !layers.contains(where: { $0["id"].uint == swipe.layer && ($0["swipe_actions"].array.contains { $0["enabled"].bool } || !$0["right_swipe"].isNull) }) { swipe.close() }
        if let drag { move(drag.point, refresh: true) }
    }
    func cancel() {
        contact.cancel(); cancelDrop(); closeMenu()
        if store?.layerSwipe.owner == swipeOwner { store?.layerSwipe.close() }
    }
}
