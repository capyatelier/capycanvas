import SwiftUI

struct PaletteCells: Equatable {
    static let tile: CGFloat = 40, gap: CGFloat = 4, pitch: CGFloat = 44
    let width: CGFloat
    var columns: Int { max(1, Int((width + Self.gap) / Self.pitch)) }
    private var cell: CGFloat { (width + Self.gap) / CGFloat(columns) }
    func x(_ index: Int) -> CGFloat { (CGFloat(index % columns) * cell).rounded() }
    func width(_ index: Int) -> CGFloat { (CGFloat(index % columns + 1) * cell).rounded() - x(index) - Self.gap }
    func y(_ index: Int) -> CGFloat { CGFloat(index / columns) * Self.pitch }
    func frame(_ index: Int) -> CGRect { CGRect(x: x(index), y: y(index), width: width(index), height: Self.tile) }
    func rows(_ count: Int) -> Int { max(1, (count + columns - 1) / columns) }
    func height(rows: Int) -> CGFloat { CGFloat(rows) * Self.pitch - Self.gap }
    func viewport(_ count: Int) -> CGFloat { height(rows: min(4, max(2, rows(count)))) }
    func slot(_ point: CGPoint, count: Int) -> Int? {
        guard point.x >= 0, point.x < width, point.y >= 0 else { return nil }
        let index = Int(point.y / Self.pitch) * columns + Int(point.x * CGFloat(columns) / (width + Self.gap))
        return index < count ? index : nil
    }
}

@MainActor final class PaletteGridInteraction: ObservableObject, NativeReorderModel {
    struct Drag {
        let token = UUID()
        let id: UInt64
        let palette: UInt64
        let original: [UInt64]
        let grab: CGSize
        let size: CGSize
        var point: CGPoint
        var slot: Int?
        var requested = false
        var answered: Int?
        var order: [UInt64]?
        var action = JSON()
    }
    weak var store: EditorStore?
    let owner = UUID()
    let contact = ReorderContact()
    var viewport: CGRect = .zero
    var cells = PaletteCells(width: 256)
    var swatches: [JSON] = []
    var palette: UInt64 = 0
    var covered = false
    var origin: CGPoint = .zero
    var selected: UInt64?
    @Published private(set) var drag: Drag?
    var edgeScroll: ReorderEdgeScroll? { ReorderEdgeScroll() }
    var enabled: Bool { store != nil && !covered && !swatches.isEmpty }
    private var ids: [UInt64] { swatches.map { $0["id"].uint } }
    private var controller: PaletteController? { store?.palettes }

    private func index(at point: CGPoint) -> Int? {
        guard let slot = cells.slot(point, count: swatches.count), cells.frame(slot).contains(point) else { return nil }
        return slot
    }
    func source(at point: CGPoint) -> ReorderTarget? {
        guard enabled, viewport.contains(point), let index = index(at: point) else { return nil }
        let id = ids[index], palette = self.palette, original = ids, frame = cells.frame(index)
        controller?.focused = true
        return ReorderTarget(id: "palette:\(palette):\(id)", surface: .swatch,
            valid: { [weak self] _ in
                guard let self else { return false }
                return !covered && self.palette == palette && ids == original
            },
            openContext: { [weak self] in guard let self else { return }; controller?.openMenu(.color(id), owner: owner) },
            closeContext: { [weak self] in guard let self else { return }; controller?.closeMenu(owner: owner) },
            begin: { [weak self] origin in self?.begin(id: id, palette: palette, original: original, frame: frame, at: origin) },
            move: { [weak self] point in self?.move(point) },
            finish: { [weak self] point in self?.drop(at: point) },
            cancel: { [weak self] in self?.cancel() })
    }
    private func begin(id: UInt64, palette: UInt64, original: [UInt64], frame: CGRect, at point: CGPoint) {
        guard let swatch = swatches.first(where: { $0["id"].uint == id }) else { return }
        drag = Drag(id: id, palette: palette, original: original,
            grab: CGSize(width: point.x - frame.minX, height: point.y - frame.minY), size: frame.size, point: point)
        controller?.lift = PaletteLift(owner: owner, id: id, frame: lifted(point), rgba: swatch["rgba"],
            color: swatch["color"], selected: selected == id)
    }
    private func lifted(_ point: CGPoint) -> CGRect {
        guard let drag else { return .zero }
        return CGRect(x: origin.x + point.x - drag.grab.width, y: origin.y + point.y - drag.grab.height,
            width: drag.size.width, height: drag.size.height)
    }
    private func move(_ point: CGPoint) {
        guard drag != nil else { return }
        drag?.point = point
        if var lift = controller?.lift, lift.owner == owner { lift.frame = lifted(point); controller?.lift = lift }
        retarget()
    }
    private func retarget() {
        guard let drag, let store else { return }
        let slot = covered || !viewport.contains(drag.point) ? nil : cells.slot(drag.point, count: drag.original.count + 1)
        if slot == drag.slot && drag.requested { return }
        self.drag?.slot = slot; self.drag?.requested = true
        let token = drag.token
        let target = slot ?? drag.original.firstIndex(of: drag.id) ?? 0
        store.query(["type": "palette_reorder_preview", "palette": drag.palette, "id": drag.id, "slot": target]) { [weak self] preview in
            guard let self, var current = self.drag, current.token == token, current.slot == slot, !preview.isNull else { return }
            current.answered = slot
            current.order = preview["order"].array.map(\.uint)
            current.action = slot == nil ? JSON() : preview["action"]
            self.drag = current
        }
    }
    private func drop(at point: CGPoint) {
        move(point)
        guard let drag else { return }
        self.drag = nil
        if controller?.lift?.owner == owner { controller?.lift = nil }
        guard let slot = drag.slot, let store else { return }
        let commit = { [weak self] (action: JSON, order: [UInt64]?) in
            guard let controller = self?.controller, !action.isNull else { return }
            controller.settle = order
            controller.apply(action.raw) { [weak controller] _ in controller?.settle = nil }
        }
        if drag.answered == slot { commit(drag.action, drag.order); return }
        store.query(["type": "palette_reorder_preview", "palette": drag.palette, "id": drag.id, "slot": slot]) { preview in
            commit(preview["action"], preview["order"].array.map(\.uint))
        }
    }
    func acceptsContext(at point: CGPoint) -> Bool { enabled && index(at: point) != nil }
    func context(at point: CGPoint) {
        guard !contact.dragging, enabled, let index = index(at: point) else { return }
        controller?.focused = true
        controller?.openMenu(.color(ids[index]), owner: owner)
    }
    func validate() {
        if contact.target != nil, !contact.validate() { cancel() }
    }
    func cancel() {
        contact.cancel()
        if drag != nil { drag = nil }
        if controller?.lift?.owner == owner { controller?.lift = nil }
    }
}

@MainActor final class PaletteRowInteraction: ObservableObject, NativeReorderModel {
    weak var store: EditorStore?
    var owner = UUID()
    let contact = ReorderContact()
    var viewport: CGRect = .zero
    var frames: [UInt64: CGRect] = [:]
    var enabled: Bool { store != nil && !frames.isEmpty }
    private func row(at point: CGPoint) -> UInt64? {
        guard viewport.contains(point) else { return nil }
        return frames.first { $0.value.contains(point) }?.key
    }
    func source(at point: CGPoint) -> ReorderTarget? {
        guard let id = row(at: point) else { return nil }
        return ReorderTarget(id: "palette-row:\(id)", surface: .row, canDrag: false,
            valid: { [weak self] _ in self?.frames[id] != nil },
            openContext: { [weak self] in guard let self else { return }; store?.palettes.openMenu(.palette(id), owner: owner) },
            closeContext: { [weak self] in guard let self else { return }; store?.palettes.closeMenu(owner: owner) },
            begin: { _ in }, move: { _ in }, finish: { _ in }, cancel: {})
    }
    func acceptsContext(at point: CGPoint) -> Bool { row(at: point) != nil }
    func context(at point: CGPoint) {
        guard let id = row(at: point) else { return }
        store?.palettes.openMenu(.palette(id), owner: owner)
    }
    func cancel() { contact.cancel() }
}

struct PaletteRowFrames: PreferenceKey {
    static var defaultValue: [UInt64: CGRect] { [:] }
    static func reduce(value: inout [UInt64: CGRect], nextValue: () -> [UInt64: CGRect]) {
        value.merge(nextValue()) { _, next in next }
    }
}
