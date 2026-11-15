import SwiftUI

/// Each projection has one cancellable geometry query/animation. It retains
/// its last model only through closing, without copying document ownership.
@MainActor final class ContentDrawerPresentation: ObservableObject, Identifiable {
    let id: String
    @Published private(set) var model = JSON()
    @Published private(set) var geometry = JSON()
    @Published private(set) var interactive = false
    @Published private(set) var glass = JSON()
    @Published private(set) var glassShown = false
    private let glassID = UUID()
    private var glassOpacity = 1.0
    private var glassWait: (@MainActor @Sendable () -> Void)?
    var glassShape: SquircleShape {
        SquircleShape(SquircleShape.surfaceRadius, square: DrawerSource.square(geometry["placement"]["bounds"].rect,
            sources: store?.contentDrawers.sources.filter { $0.key != id }.map(\.value) ?? [], joined: geometry["connection"]["square_corners"]))
    }
    @Published private(set) var pagePosition: CGFloat = 0
    @Published private(set) var layerSizes = JSON()
    @Published private(set) var measurement = JSON()
    @Published private(set) var colorFit = JSON()
    private var measurementKey = ""
    private var wheelHeight: CGFloat = 0
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
        let placement: JSON?
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
        let nextKey = JSON([next.raw, store.snapshot["layout"].raw, store.contentDrawers.pixelScale,
            measured, layerSizes.raw, wheelHeight, store.snapshot["color_panel"]["hdr"].raw, id == "tool" ? store.contentDrawers.tileRevision : 0]).stableKey
        guard key != nextKey else { return }; key = nextKey
        let pageTo: CGFloat = model["layer_properties"].bool ? 1 : 0
        let pageChange = !model["layer_properties"].isNull && !next.isNull && pagePosition != pageTo
        latest = Request(from: geometry["placement"], placement: nil, heights: measured, closing: next.isNull,
            animate: changed || pageChange, pageFrom: pageChange ? pagePosition : nil, pageTo: pageTo)
        start()
    }
    func present(_ next: JSON, placement: JSON) {
        guard let store else { return }
        let closing = next.isNull
        if interactive != !closing { interactive = !closing }
        if !closing, model.stableKey != next.stableKey { model = next }
        let target = closing ? geometry["placement"] : placement
        guard !target.isNull else { if closing { store.contentDrawers.remove(id) }; return }
        let nextKey = JSON([closing ? JSON().raw : target.raw, closing, store.contentDrawers.pixelScale]).stableKey
        guard key != nextKey else { return }; key = nextKey
        latest = Request(from: geometry["placement"], placement: target, heights: [], closing: closing,
            animate: true, pageFrom: nil, pageTo: 0)
        start()
    }
    func refreshScale() {
        guard latest != nil else { return }
        if let placement = latest?.placement { present(interactive ? model : JSON(), placement: placement) }
        else { refresh(current) }
    }
    private func start() {
        guard let store else { return }
        guard task == nil else { return }
        task = Task { [weak self] in
            let duration = max(0.001, store.catalog["panel_expansion_ms"].number / 1000)
            while !Task.isCancelled {
                guard let self, let request = self.latest else { return }
                let phone = !store.snapshot["layout"]["phone"].isNull
                let target: JSON
                if request.closing { target = request.from }
                else if let placement = request.placement { target = placement }
                else {
                    let page = request.pageTo == 1 ? 1 : 0
                    let heights = !self.model["layer_properties"].isNull && self.layerSizes["heights"].array.count == 2
                        ? [self.layerSizes["heights"][page].number] : request.heights
                    if phone {
                        let context = self.sizingContext + self.model["columns"].stableKey + self.model["tool_set"].stableKey
                        if self.measurementKey != context {
                            let next: JSON = await withCheckedContinuation { continuation in
                                store.query(["type": "drawer_measure", "column": UInt64(self.id) as Any? ?? NSNull()]) {
                                    continuation.resume(returning: $0)
                                }
                            }
                            guard !Task.isCancelled else { return }
                            guard self.latest?.token == request.token else { continue }
                            self.measurement = next; self.measurementKey = context
                        }
                        if !self.model["layer_properties"].isNull {
                            guard self.layerSizes["placements"].array.count == 2 else { self.task = nil; return }
                            target = self.layerSizes["placements"][page]
                        } else {
                            guard heights.count == self.measurement["measure"]["placement"]["columns"].array.count,
                                heights.allSatisfy({ $0 > 0 }) else { self.task = nil; return }
                            var query: [String: Any] = ["measure": self.measurement["measure"].raw, "heights": heights, "progress": 1]
                            if self.wheelHeight > 0, let column = self.model["columns"].array.firstIndex(where: { $0.array.contains { $0.string == "color" } }) {
                                query["color"] = ["column": column, "fixed_height": max(0, heights[column] - self.wheelHeight),
                                    "hdr": store.snapshot["color_panel"]["hdr"].bool]
                            }
                            let frame = Self.frame(query, pixelScale: store.contentDrawers.pixelScale)
                            self.colorFit = frame["color_fit"]
                            target = frame["placement"]
                        }
                    } else {
                        target = await withCheckedContinuation { continuation in
                            store.query(["type": "drawer", "column": UInt64(self.id) as Any? ?? NSNull(),
                                "heights": heights, "progress": 1, "from": NSNull(), "closing": false]) {
                                continuation.resume(returning: $0["placement"])
                            }
                        }
                    }
                }
                guard !Task.isCancelled else { return }
                guard self.latest?.token == request.token else { continue }
                guard !target.isNull else { self.task = nil; if request.closing { store.contentDrawers.remove(self.id) }; return }
                if !self.glass.isNull {
                    await self.handoff(JSON())
                    guard !Task.isCancelled else { return }
                    guard self.latest?.token == request.token else { continue }
                }
                let start = ProcessInfo.processInfo.systemUptime
                while !Task.isCancelled && self.latest?.token == request.token {
                    let progress = request.animate && !self.reduceMotion ? min(1, (ProcessInfo.processInfo.systemUptime - start) / duration) : 1
                    let eased = CGFloat(1 - pow(1 - progress, 3))
                    let position = request.pageFrom.map { $0 + (request.pageTo - $0) * eased } ?? request.pageTo
                    let result = Self.frame(["from": request.from.raw, "to": target.raw, "progress": eased, "closing": request.closing, "clip_contents": phone],
                        pixelScale: store.contentDrawers.pixelScale)
                    self.geometry = result; self.pagePosition = position; store.workspace.refreshChrome()
                    store.contentDrawers.source(self.id, geometry: result, anchor: self.model["anchor"])
                    if progress == 1 {
                        if !request.closing { await self.handoff(result["glass"]) }
                        guard !Task.isCancelled else { return }
                        guard self.latest?.token == request.token else { continue }
                        self.task = nil
                        if request.closing { store.contentDrawers.remove(self.id) }
                        return
                    }
                    do { try await Task.sleep(for: .milliseconds(8)) } catch { return }
                }
            }
        }
    }
    static func frame(_ request: [String: Any], pixelScale: CGFloat) -> JSON {
        do {
            var request = request
            request["pixel_scale"] = pixelScale
            let text = try JSON(request).encoded()
            guard let response = text.withCString({ takeNativeText(capy_apple_drawer_frame($0)) }) else { return JSON() }

            return try JSON.decode(response)
        } catch { return JSON(["error": error.localizedDescription]) }
    }
    func measure(_ height: CGFloat, column: Int) {
        guard height.isFinite, abs((heights[column] ?? 0) - height) > 0.5 else { return }
        heights[column] = height; refresh(current)
    }
    func measureColor(_ height: CGFloat) {
        guard height.isFinite, abs(wheelHeight - height) > 0.5 else { return }
        wheelHeight = height; refresh(current)
    }
    func measureLayerPages(_ next: JSON) {
        guard next.stableKey != layerSizes.stableKey else { return }
        layerSizes = next; refresh(current)
    }
    func toggleLayerProperties() { store?.customize(["type": "toggle_layer_drawer_properties"]) }
    func registerGlass(opacity: Double) {
        glassOpacity = opacity
        let bounds = glass.rect, active = !glass.isNull && opacity > 0
        store?.glass.box(glassID, active ? [bounds.minX, bounds.minY, bounds.width, bounds.height]
            + glassShape.radii(in: CGRect(origin: .zero, size: bounds.size)) + [1, opacity] : nil)
        store?.glass.connection("drawer:" + id, active && !geometry["connection"].isNull
            ? geometry["connection"].replacing("strength", with: JSON(opacity)) : nil)
    }
    private func handoff(_ bounds: JSON) async {
        guard let store else { return }
        let request = latest?.token
        glassShown = false; glass = bounds; registerGlass(opacity: glassOpacity)
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            var pending: CheckedContinuation<Void, Never>? = continuation
            let resume: @MainActor @Sendable () -> Void = { let waiter = pending; pending = nil; waiter?.resume() }
            glassWait = resume
            store.glass.flush { DispatchQueue.main.async { resume() } }
        }
        guard !Task.isCancelled, latest?.token == request else { return }
        glassShown = !bounds.isNull && store.canvasReady
    }
    func clearGlass() { store?.glass.box(glassID, nil); store?.glass.connection("drawer:" + id, nil) }
    func stop() {
        latest = nil; task?.cancel(); task = nil; glassWait?(); glassWait = nil
        glass = JSON(); glassShown = false; clearGlass()
    }
    func retire() { stop(); key = ""; model = JSON(); geometry = JSON(); interactive = false }
}

@MainActor final class ContentDrawersPresentation: ObservableObject {
    @Published private(set) var items: [String: ContentDrawerPresentation] = [:]
    @Published private(set) var sources: [String: DrawerSource] = [:]
    let menu: ContentDrawerPresentation
    private(set) var pixelScale: CGFloat = 1
    private weak var store: EditorStore?
    private var tilesKey = ""
    private var columnsKey = ""
    private var columns: [UInt64: CGRect] = [:]
    private(set) var tileRevision = 0
    init(store: EditorStore) { self.store = store; menu = ContentDrawerPresentation(id: "menu", store: store) }
    func updateScale(_ scale: CGFloat) {
        guard pixelScale != scale else { return }
        pixelScale = scale
        for drawer in Array(items.values) + [menu] { drawer.refreshScale() }
    }
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
        if id == menu.id { menu.retire() }
        else { items.removeValue(forKey: id)?.stop() }
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
