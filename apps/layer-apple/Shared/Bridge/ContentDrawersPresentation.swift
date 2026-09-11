import SwiftUI

/// Each projection has one cancellable geometry query/animation. It retains
/// its last model only through closing, without copying document ownership.
@MainActor final class ContentDrawerPresentation: ObservableObject, Identifiable {
    let id: String
    @Published private(set) var model = JSON()
    @Published private(set) var geometry = JSON()
    private weak var store: EditorStore?
    private var current = JSON()
    private var key = ""
    private var heights: [Int: CGFloat] = [:]
    private var task: Task<Void, Never>?
    private struct Request {
        let token = UUID()
        let from: JSON
        let heights: [CGFloat]
        let closing: Bool
        let animate: Bool
        let start = ProcessInfo.processInfo.systemUptime
    }
    private var latest: Request?
    private var bodies: [String: JSON] = [:]
    init(id: String, store: EditorStore) { self.id = id; self.store = store }
    func panel(_ id: JSON) -> JSON {
        store?.snapshot["panels"].array.first { $0["id"].string == id.string } ?? bodies[id.string] ?? JSON()
    }
    func refresh(_ next: JSON) {
        guard let store else { return }
        let changed = current.stableKey != next.stableKey
        current = next
        if !next.isNull {
            if model.stableKey != next.stableKey { model = next; heights = [:] }
            for panel in store.snapshot["panels"].array { bodies[panel["id"].string] = panel }
        }
        let measured = model["columns"].array.indices.map { heights[$0] ?? 0 }
        let nextKey = JSON([next.raw, store.snapshot["layout"].raw, store.snapshot["partial_zen"].raw,
            measured, id == "tool" ? store.contentDrawers.tileRevision : 0]).stableKey
        guard key != nextKey else { return }; key = nextKey
        latest = Request(from: geometry["placement"], heights: measured, closing: next.isNull, animate: changed)
        // Coalesce changes while Rust owns a query. Cancelling a Swift task
        // cannot cancel an already-queued native request, so do not enqueue a
        // replacement until the current reply returns.
        guard task == nil else { return }
        task = Task { [weak self] in
            let duration = max(0.001, store.catalog["panel_expansion_ms"].number / 1000)
            while !Task.isCancelled {
                guard let self, let request = self.latest else { return }
                let progress = request.animate ? min(1, (ProcessInfo.processInfo.systemUptime - request.start) / duration) : 1
                let result: JSON = await withCheckedContinuation { continuation in
                    store.query(["type": "drawer", "column": UInt64(self.id) as Any? ?? NSNull(),
                        "heights": request.heights, "progress": progress, "from": request.from.raw, "closing": request.closing]) {
                        continuation.resume(returning: $0)
                    }
                }
                guard !Task.isCancelled else { return }
                guard self.latest?.token == request.token else { continue }
                self.geometry = result; store.workspace.refreshChrome()
                if progress == 1 || result.isNull {
                    self.task = nil
                    if request.closing { store.contentDrawers.remove(self.id) }
                    return
                }
                do { try await Task.sleep(for: .milliseconds(16)) } catch { return }
            }
        }
    }
    func measure(_ height: CGFloat, column: Int) {
        guard height.isFinite, abs((heights[column] ?? 0) - height) > 0.5 else { return }
        heights[column] = height; refresh(current)
    }
    func stop() { latest = nil; task?.cancel(); task = nil }
}

@MainActor final class ContentDrawersPresentation: ObservableObject {
    @Published private(set) var items: [String: ContentDrawerPresentation] = [:]
    private weak var store: EditorStore?
    private var tilesKey = ""
    private(set) var tileRevision = 0
    init(store: EditorStore) { self.store = store }
    func refresh() {
        guard let store else { return }
        var models: [String: JSON] = [:]
        for model in store.state["customization"]["column_drawers"].array {
            models[String(model["anchor"]["column"].uint)] = model
        }
        let tool = store.state["customization"]["drawer"]
        if !tool.isNull { models["tool"] = tool }
        for id in models.keys where items[id] == nil { items[id] = ContentDrawerPresentation(id: id, store: store) }
        for (id, item) in items { item.refresh(models[id] ?? JSON()) }
    }
    func remove(_ id: String) {
        items.removeValue(forKey: id)?.stop()
        store?.workspace.refreshChrome()
    }
    func measureTiles(_ measurements: [String: DrawerTileBounds]) {
        let values = measurements.keys.sorted().compactMap { key -> Any? in
            guard let tile = measurements[key], !tile.bounds.isEmpty else { return nil }
            return ["column": tile.column, "anchor": ["panel": tile.panel, "tile": tile.tile], "bounds": JSON(tile.bounds).raw]
        }
        let next = JSON(values).stableKey
        guard next != tilesKey else { return }; tilesKey = next
        store?.dispatch(["type": "measure_drawer_tiles", "measurements": values])
        tileRevision &+= 1
        refresh()
    }
}

struct DrawerTileBounds: Equatable {
    let column: UInt64
    let panel: String
    let tile: UInt64
    let bounds: CGRect
}
struct DrawerTileMeasurements: PreferenceKey {
    static var defaultValue: [String: DrawerTileBounds] { [:] }
    static func reduce(value: inout [String: DrawerTileBounds], nextValue: () -> [String: DrawerTileBounds]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}
struct DrawerTileMeasurement: ViewModifier {
    let panel: String
    let tile: UInt64
    @Environment(\.drawerColumn) private var column
    @Environment(\.workspaceClip) private var clip
    func body(content: Content) -> some View {
        content.background(GeometryReader { geometry in
            if let column {
                let bounds = geometry.frame(in: .named("editor-workspace")).intersection(clip)
                Color.clear.preference(key: DrawerTileMeasurements.self, value: ["\(column):\(panel):\(tile)":
                    DrawerTileBounds(column: column, panel: panel, tile: tile, bounds: bounds)])
            }
        })
    }
}
private struct DrawerColumn: EnvironmentKey { static let defaultValue: UInt64? = nil }
extension EnvironmentValues {
    var drawerColumn: UInt64? { get { self[DrawerColumn.self] } set { self[DrawerColumn.self] = newValue } }
}
extension JSON {
    init(_ bounds: CGRect) { self.init(["x": bounds.minX, "y": bounds.minY, "width": bounds.width, "height": bounds.height]) }
    func relative(to parent: JSON) -> JSON { JSON(rect.offsetBy(dx: -parent.rect.minX, dy: -parent.rect.minY)) }
}
