import SwiftUI

/// Each projection has one cancellable geometry query/animation. It retains
/// its last model only through closing, without copying document ownership.
@MainActor final class ContentDrawerPresentation: ObservableObject, Identifiable {
    let id: String
    @Published private(set) var model = JSON()
    @Published private(set) var geometry = JSON()
    @Published private(set) var interactive = false
    @Published private(set) var pagePosition: CGFloat = 0
    @Published private(set) var layerSizes = JSON()
    private weak var store: EditorStore?
    private var current = JSON()
    private var key = ""
    var sizingContext: String {
        guard let store else { return "" }
        return JSON([model["anchor"].raw, store.snapshot["layout"].raw, id == "tool" ? store.contentDrawers.tileRevision : 0]).stableKey
    }
    private var heights: [Int: CGFloat] = [:]
    private var task: Task<Void, Never>?
    var reduceMotion = false
    private struct Request {
        let token = UUID()
        let from: JSON
        let heights: [CGFloat]
        let closing: Bool
        let animate: Bool
        let pageFrom: CGFloat?
        let pageTo: CGFloat
    }
    private var latest: Request?
    private var bodies: [String: JSON] = [:]
    init(id: String, store: EditorStore) { self.id = id; self.store = store }
    func panel(_ id: JSON) -> JSON {
        if let panel = store?.panel(id.string), !panel.isNull { return panel }
        return bodies[id.string] ?? JSON()
    }
    func refresh(_ next: JSON) {
        guard let store else { return }
        let changed = current.stableKey != next.stableKey
        current = next
        if interactive != !next.isNull { interactive = !next.isNull }
        if !next.isNull {
            if model.stableKey != next.stableKey {
                if model["columns"].stableKey != next["columns"].stableKey || model["tabs"].stableKey != next["tabs"].stableKey { heights = [:]; layerSizes = JSON() }
                model = next
            }
            for panel in store.snapshot["panels"].array { bodies[panel["id"].string] = panel }
        }
        let measured = model["columns"].array.indices.map { heights[$0] ?? 0 }
        let nextKey = JSON([next.raw, store.snapshot["layout"].raw,
            measured, layerSizes.raw, id == "tool" ? store.contentDrawers.tileRevision : 0]).stableKey
        guard key != nextKey else { return }; key = nextKey
        let pageTo: CGFloat = model["layer_properties"].bool ? 1 : 0
        let pageChange = !model["layer_properties"].isNull && !next.isNull && pagePosition != pageTo
        latest = Request(from: geometry["placement"], heights: measured, closing: next.isNull,
            animate: changed || pageChange, pageFrom: pageChange ? pagePosition : nil, pageTo: pageTo)
        guard task == nil else { return }
        task = Task { [weak self] in
            let duration = max(0.001, store.catalog["panel_expansion_ms"].number / 1000)
            while !Task.isCancelled {
                guard let self, let request = self.latest else { return }
                let target: JSON
                if request.closing { target = request.from }
                else {
                    let page = request.pageTo == 1 ? 1 : 0
                    let heights = !self.model["layer_properties"].isNull && self.layerSizes["heights"].array.count == 2
                        ? [self.layerSizes["heights"][page].number] : request.heights
                    target = await withCheckedContinuation { continuation in
                        store.query(["type": "drawer", "column": UInt64(self.id) as Any? ?? NSNull(),
                            "heights": heights, "progress": 1, "from": NSNull(), "closing": false]) {
                            continuation.resume(returning: $0["placement"])
                        }
                    }
                }
                guard !Task.isCancelled else { return }
                guard self.latest?.token == request.token else { continue }
                guard !target.isNull else { self.task = nil; if request.closing { store.contentDrawers.remove(self.id) }; return }
                let start = ProcessInfo.processInfo.systemUptime
                while !Task.isCancelled && self.latest?.token == request.token {
                    let progress = request.animate && !self.reduceMotion ? min(1, (ProcessInfo.processInfo.systemUptime - start) / duration) : 1
                    let eased = CGFloat(1 - pow(1 - progress, 3))
                    let position = request.pageFrom.map { $0 + (request.pageTo - $0) * eased } ?? request.pageTo
                    let result = Self.frame(["from": request.from.raw, "to": target.raw, "progress": eased, "closing": request.closing])
                    self.geometry = result; self.pagePosition = position; store.workspace.refreshChrome()
                    store.contentDrawers.source(self.id, geometry: result, anchor: self.model["anchor"])
                    if progress == 1 {
                        self.task = nil
                        if request.closing { store.contentDrawers.remove(self.id) }
                        return
                    }
                    do { try await Task.sleep(for: .milliseconds(8)) } catch { return }
                }
            }
        }
    }
    private static func frame(_ request: [String: Any]) -> JSON {
        do {
            let text = try JSON(request).encoded()
            guard let response = text.withCString({ takeNativeText(capy_apple_drawer_frame($0)) }) else { return JSON() }

            return try JSON.decode(response)
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    func measure(_ height: CGFloat, column: Int) {
        guard height.isFinite, abs((heights[column] ?? 0) - height) > 0.5 else { return }
        heights[column] = height; refresh(current)
    }
    func measureLayerPages(_ next: JSON) {
        guard next.stableKey != layerSizes.stableKey else { return }
        layerSizes = next; refresh(current)
    }
    func toggleLayerProperties() { store?.customize(["type": "toggle_layer_drawer_properties"]) }
    func stop() { latest = nil; task?.cancel(); task = nil }
}

@MainActor final class ContentDrawersPresentation: ObservableObject {
    @Published private(set) var items: [String: ContentDrawerPresentation] = [:]
    @Published private(set) var sources: [String: DrawerSource] = [:]
    private weak var store: EditorStore?
    private var tilesKey = ""
    private var columnsKey = ""
    private var columns: [UInt64: CGRect] = [:]
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
    func source(_ id: String, geometry: JSON, anchor: JSON = JSON()) {
        let next = geometry["connection"].isNull ? nil : DrawerSource(placement: geometry["placement"], anchor: anchor)
        guard sources[id] != next else { return }
        store?.objectWillChange.send()
        sources[id] = next
    }
    func remove(_ id: String) {
        items.removeValue(forKey: id)?.stop()
        source(id, geometry: JSON())
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
    func measureColumns(_ measurements: [UInt64: CGRect], force: Bool = false) {
        columns = measurements
        let values: [Any] = measurements.keys.sorted().compactMap { group in
            guard let bounds = measurements[group], !bounds.isEmpty,
                [bounds.minX, bounds.minY, bounds.width, bounds.height].allSatisfy(\.isFinite) else { return nil }
            return ["group": group, "bounds": JSON(bounds).raw]
        }
        let next = JSON(values).stableKey
        guard force || next != columnsKey else { return }; columnsKey = next
        store?.dispatch(["type": "measure_column_drawers", "measurements": values])
    }
    func prepareDrag() {
        // Closing/reopening a drawer or workspace undo can clear Rust's transient
        // measurements even when SwiftUI coalesces back to identical geometry.
        measureColumns(columns, force: true)
    }
}

struct ColumnDrawerMeasurements: PreferenceKey {
    static var defaultValue: [UInt64: CGRect] { [:] }
    static func reduce(value: inout [UInt64: CGRect], nextValue: () -> [UInt64: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
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
