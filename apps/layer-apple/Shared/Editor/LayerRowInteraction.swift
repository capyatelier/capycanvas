import SwiftUI

struct LayerRowFrame: Equatable {
    var row: CGRect = .zero
    var grip: CGRect = .zero
    var name: CGRect = .zero
    var mask: CGRect = .zero
}
struct LayerRowFrames: PreferenceKey {
    static var defaultValue: [UInt64: LayerRowFrame] { [:] }
    static func reduce(value: inout [UInt64: LayerRowFrame], nextValue: () -> [UInt64: LayerRowFrame]) {
        value.merge(nextValue()) { old, next in
            LayerRowFrame(row: next.row == .zero ? old.row : next.row,
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
            let value = { var value = LayerRowFrame(); value[keyPath: part] = frame; return value }()
            Color.clear.preference(key: LayerRowFrames.self, value: [id: value])
        })
        } else { content }
    }
}
enum LayerMenuSource: Equatable { case row(UInt64), footer }

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
    var frames: [UInt64: LayerRowFrame] = [:]
    var viewport: CGRect = .zero
    @Published private(set) var drag: Drag?
    @Published private(set) var nativeDragging = false
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
    func nativeDragChanged(_ active: Bool) { if nativeDragging != active { nativeDragging = active } }
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
        let mask = row["has_mask"].bool && frame.mask.contains(point)
        return ReorderTarget(id: identity(id), surface: frame.grip.contains(point) ? .handle : .row,
            canDrag: row["can_drop_below"].bool,
            valid: { [weak self] _ in
                guard let self else { return false }
                return enabled && epoch == currentEpoch && renaming != id && layers.contains { $0["id"].uint == id }
            }, openContext: { [weak self] in self?.openMenu(id: id, mask: mask) },
            closeContext: { [weak self] in self?.closeMenu() },
            begin: { [weak self] origin in self?.drag = Drag(id: id, bounds: frame.row, origin: origin, point: origin) },
            move: { [weak self] point in self?.move(point) },
            finish: { [weak self] point in
                guard let self else { return }
                move(point); let completed = drag; drag = nil
                if let completed, let target = completed.target {
                    store?.layer(["op": "drop", "id": id, "target": target, "fraction": completed.fraction])
                }
            }, cancel: { [weak self] in self?.drag = nil; self?.nativeDragChanged(false) })
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
        menuSource = source ?? .row(id)
        loadMenu(id: id, mask: mask) { [weak self] result in self?.menu = result }
    }
    func nativeMenu(at point: CGPoint) -> NativeReorderMenu? {
        guard let row = row(at: point), let frame = frames[row["id"].uint] else { return nil }
        let id = row["id"].uint, currentEpoch = epoch
        let mask = row["has_mask"].bool && frame.mask.contains(point)
        return NativeReorderMenu(id: identity(id), bounds: frame.row) { [weak self] completion in
            guard let self, epoch == currentEpoch else { completion(nil); return }
            loadMenu(id: id, mask: mask) { [weak self] result in
                guard let self, !result.isNull else { completion(nil); return }
                completion(AppleContextMenu(result) { [weak self] action in
                    guard let self, epoch == currentEpoch, layers.contains(where: { $0["id"].uint == id }) else { return }
                    closeMenu(); store?.dispatch(action)
                })
            }
        }
    }
    private func loadMenu(id: UInt64, mask: Bool, completion: @escaping (JSON) -> Void) {
        guard let store, layers.contains(where: { $0["id"].uint == id }) else { completion(JSON()); return }
        let request = UUID(), currentEpoch = epoch; menuRequest = request
        store.layer(["op": "context", "id": id, "mask": mask])
        store.query(["type": "layer_menu", "id": id, "mask": mask]) { [weak self] result in
            guard let self, menuRequest == request, epoch == currentEpoch,
                  layers.contains(where: { $0["id"].uint == id }) else { completion(JSON()); return }
            completion(result)
        }
    }
    func closeMenu() { menuRequest = UUID(); if menuSource != nil { menuSource = nil }; if !menu.isNull { menu = JSON() } }
    func validate() {
        // A document edit can remove the source while the pointer is stationary.
        // Retire it on publication, without waiting for a native move or release.
        if contact.target != nil, !contact.validate() { cancel() }
    }
    func cancel() {
        contact.cancel(); if drag != nil { drag = nil }; nativeDragChanged(false); closeMenu()
    }
}
