import SwiftUI

@MainActor final class GlassRegistry {
    private weak var store: EditorStore?
    private var boxes: [UUID: [CGFloat]] = [:]
    private var connections: [String: JSON] = [:]
    private var arcs: [UUID: [CGFloat]] = [:]
    private var colorBubbles: [UUID: [CGFloat]] = [:]
    private var panelAllocations: [String: PanelBridgeAllocation] = [:]
    private var panelLayout = JSON()
    private var panelFrame = JSON()
    private var panelQueryKey = ""
    private var panelRevision: UInt64 = 0
    private var panelSurfaceColumns = Set<UInt64>()
    private var panelSurfaceGroups = Set<UInt64>()
    private var scheduled = false
    private var sent = ""
    init(store: EditorStore) { self.store = store }
    func box(_ id: UUID, _ value: [CGFloat]?) {
        guard boxes[id] != value else { return }
        boxes[id] = value; schedule()
    }
    func connection(_ key: String, _ value: JSON?) {
        guard connections[key]?.stableKey != value?.stableKey else { return }
        connections[key] = value; schedule()
    }
    func arc(_ id: UUID, _ value: [CGFloat]?) {
        guard arcs[id] != value else { return }; arcs[id] = value; schedule()
    }
    func colorBubble(_ id: UUID, _ value: [CGFloat]?) {
        guard colorBubbles[id] != value else { return }; colorBubbles[id] = value; schedule()
    }
    func panelMeasurements(_ allocations: [String: PanelBridgeAllocation], layout: JSON) {
        panelAllocations = allocations
        guard let store else { return }
        let groups = layout["groups"].array.compactMap { group -> [Any]? in
            guard let allocation = allocations["group:\(group["id"].uint)"], !allocation.bounds.isEmpty else { return nil }
            return [group["id"].uint, allocation.bounds.json]
        }
        let columns = layout["collapsed"].array.compactMap { column -> [String: Any]? in
            let id = column["id"].uint
            guard let bounds = allocations["column:\(id)"]?.bounds,
                let content = allocations["column-content:\(id)"]?.bounds,
                !bounds.isEmpty, !content.isEmpty else { return nil }
            let icons: [Any] = column["groups"].array.flatMap { $0["icons"].array }.compactMap { icon in
                guard let allocation = allocations["column-icon:\(id):\(icon["panel"].string)"], !allocation.bounds.isEmpty else { return nil }
                return [icon["panel"].string, allocation.bounds.json]
            }
            guard icons.count == column["groups"].array.flatMap({ $0["icons"].array }).count else { return nil }
            return ["column": id, "bounds": bounds.json, "content": content.json, "icons": icons]
        }
        let request = JSON(["type": "panel_bridges", "layout": layout.raw,
            "measure": ["groups": groups, "columns": columns]])
        let key = JSON([request.raw, layout.raw, store.state["document_file"]["epoch"].raw]).stableKey
        guard key != panelQueryKey else { flush(); return }
        panelQueryKey = key
        flush()
        store.query(request.object) { [weak self] frame in
            guard let self, self.panelQueryKey == key else { return }
            guard !frame.isNull else { self.panelQueryKey = ""; return }
            self.panelLayout = layout
            self.setPanelFrame(frame)
            self.flush()
            self.store?.objectWillChange.send()
        }
    }
    func panelTabsChanged() {
        flush()
        DispatchQueue.main.async { [weak store = store] in store?.objectWillChange.send() }
    }
    func panelSlideChanged() { flush() }
    func panelGroupConnected(_ id: UInt64) -> Bool {
        panelBridgesReady && panelSurfaceGroups.contains(id)
    }
    func panelColumnConnected(_ id: UInt64) -> Bool {
        panelBridgesReady && panelSurfaceColumns.contains(id)
    }
    func panelGroupRadii(_ id: UInt64) -> [CGFloat]? {
        guard panelGroupConnected(id) else { return nil }
        return panelFrame["groups"].array.first { $0["group"].uint == id }?["radii"].array.map { CGFloat($0.number) }
    }
    func panelColumnRadii(_ id: UInt64) -> [CGFloat]? {
        guard panelColumnConnected(id) else { return nil }
        return panelFrame["columns"].array.first { $0["column"].uint == id }?["radii"].array.map { CGFloat($0.number) }
    }
    private var panelBridgesReady: Bool {
        guard let store else { return false }
        return !panelFrame["bridges"].array.isEmpty
            && store.snapshot["display_status"]["panel_revision"].optionalUInt == panelRevision
    }
    private func setPanelFrame(_ frame: JSON) {
        guard frame.stableKey != panelFrame.stableKey else { return }
        panelFrame = frame
        panelRevision &+= 1
    }
    private func schedule() {
        guard !scheduled else { return }
        scheduled = true
        DispatchQueue.main.async { [weak self] in self?.flush() }
    }
    private func flush() {
        scheduled = false
        let panels = panelSurfaces()
        let payload = JSON(["regions": boxes.sorted { $0.key.uuidString < $1.key.uuidString }.map { $0.value.map(Double.init) },
            "connections": connections.keys.sorted().compactMap { connections[$0]?.raw },
            "arcs": arcs.sorted { $0.key.uuidString < $1.key.uuidString }.map { $0.value.map(Double.init) },
            "color_bubbles": colorBubbles.sorted { $0.key.uuidString < $1.key.uuidString }.map { $0.value.map(Double.init) },
            "panels": panels, "panel_revision": panelRevision])
        let key = payload.stableKey
        guard key != sent else { return }
        sent = key
        store?.native?.glassRegions(payload)
    }
    private func panelSurfaces() -> [Any] {
        panelSurfaceColumns.removeAll(keepingCapacity: true)
        panelSurfaceGroups.removeAll(keepingCapacity: true)
        guard let store else { return [] }
        let panel = store.state["palette"]["glass"]["panel"].array.map(\.number)
        let strip = store.state["palette"]["glass"]["strip"].array.map(\.number)
        guard panel.count == 4, strip.count == 4 else { return [] }
        return panelFrame["columns"].array.compactMap { column in
            let id = column["column"].uint
            let bridges = panelFrame["bridges"].array.filter { $0[0].uint == id }.map { $0[2] }
            guard let first = bridges.first else { return nil }
            let columnOpacity = panelAllocations["column:\(id)"]?.opacity ?? 0
            let receivers: [[String: Any]] = first["receivers"].array.compactMap { receiver in
                let group = receiver["group"].uint
                let bounds = panelFrame["groups"].array.first { $0["group"].uint == group }?["bounds"].rect ?? .zero
                guard !bounds.isEmpty else { return nil }
                let opacity = panelAllocations["group:\(group)"]?.opacity ?? 0
                let tabbed = panelLayout["groups"].array.first { $0["id"].uint == group }?["tabs_visible"].bool == true
                let active = panelLayout["groups"].array.first { $0["id"].uint == group }?["active"].string ?? ""
                let panels = panelLayout["groups"].array.first { $0["id"].uint == group }?["panels"].array.map(\.string) ?? []
                let index = panels.firstIndex(of: active)
                let tab = index.flatMap { store.workspace.tabFrames["\(group):\($0)"] }
                let moving = store.workspace.tabSlide.grab.flatMap { grab -> (WorkspaceTabFrame, CGFloat)? in
                    guard grab.group == group, !store.workspace.tabSlide.preview.isNull,
                        let index = grab.panels.firstIndex(of: active), grab.frames.indices.contains(index) else { return nil }
                    return (grab.frames[index], CGFloat(store.workspace.tabSlide.offset(group: group, index: index)))
                }
                let tabBar = tabbed ? CGRect(x: bounds.minX, y: bounds.minY,
                    width: bounds.width, height: min(36, bounds.height)) : .zero
                return ["group": group, "tab_bar": tabBar.values,
                    "active_tab": (tabbed ? moving.map { $0.0.bounds.offsetBy(dx: $0.1, dy: 0) }
                        ?? tab?.bounds : nil)?.values ?? CGRect.zero.values,
                    "tab_clip": (tabbed ? tabBar.intersection(moving?.0.clip ?? tab?.clip ?? tabBar) : .zero).values,
                    "color": panel.withOpacity(opacity), "strip_color": strip.withOpacity(opacity)]
            }
            guard receivers.count == first["receivers"].array.count else { return nil }
            panelSurfaceColumns.insert(id)
            panelSurfaceGroups.formUnion(first["receivers"].array.map { $0["group"].uint })
            return ["offset": [0, 0], "sidebar": column["bounds"].rect.values,
                "radii": column["radii"].raw, "color": panel.withOpacity(columnOpacity),
                "bridges": bridges.map(\.raw), "groups": receivers]
        }
    }
}

struct PanelBridgeAllocation: Equatable {
    let bounds: CGRect
    let opacity: Double
}

struct PanelBridgeAllocations: PreferenceKey {
    static var defaultValue: [String: PanelBridgeAllocation] { [:] }
    static func reduce(value: inout [String: PanelBridgeAllocation], nextValue: () -> [String: PanelBridgeAllocation]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}

struct PanelBridgeMeasurement: ViewModifier {
    let key: String
    var active = true
    @Environment(\.workspaceChromeOpacity) private var opacity
    func body(content: Content) -> some View {
        content.background(GeometryReader { geometry in
            let bounds = geometry.frame(in: .named("editor-workspace"))
            Color.clear.preference(key: PanelBridgeAllocations.self,
                value: !active || opacity <= 0 || bounds.isEmpty ? [:]
                    : [key: PanelBridgeAllocation(bounds: bounds, opacity: opacity)])
        })
    }
}

private extension CGRect {
    var json: [String: Double] { ["x": Double(minX), "y": Double(minY), "width": Double(width), "height": Double(height)] }
    var values: [Double] { isNull ? [0, 0, 0, 0] : [Double(minX), Double(minY), Double(width), Double(height)] }
}
private extension Array where Element == Double {
    func withOpacity(_ opacity: Double) -> [Double] {
        indices.contains(3) ? enumerated().map { $0.offset == 3 ? $0.element * opacity : $0.element } : self
    }
}

private struct GlassRegistryKey: EnvironmentKey { static let defaultValue: GlassRegistry? = nil }
extension EnvironmentValues {
    var glassRegistry: GlassRegistry? {
        get { self[GlassRegistryKey.self] }
        set { self[GlassRegistryKey.self] = newValue }
    }
}

private struct GlassGeometry: Equatable {
    var bounds = CGRect.zero
    var size = CGSize.zero
    init() {}
    init(_ proxy: GeometryProxy) { bounds = proxy.frame(in: .named("editor-workspace")); size = proxy.size }
    var scale: CGFloat { bounds.width / max(1, size.width) }
}

struct GlassRegistration: ViewModifier {
    let shape: SquircleShape
    var active = true
    var circular = false
    var rounded = false
    var strength: Double = 1
    @Environment(\.glassRegistry) private var registry
    @Environment(\.workspaceChromeOpacity) private var opacity
    @State private var id = UUID()
    @State private var geometry = GlassGeometry()
    private var region: [CGFloat] {
        let bounds = geometry.bounds
        guard active && opacity > 0 && strength > 0 && !bounds.isEmpty else { return [] }
        let radii = circular ? Array(repeating: min(bounds.width, bounds.height) / 2, count: 4)
            : shape.radii(in: CGRect(origin: .zero, size: geometry.size)).map { $0 * geometry.scale }
        return [bounds.minX, bounds.minY, bounds.width, bounds.height] + radii + [circular || rounded ? 0 : 1, strength * opacity]
    }
    func body(content: Content) -> some View {
        content.onGeometryChange(for: GlassGeometry.self) { GlassGeometry($0) } action: { geometry = $0 }
            .onChange(of: region, initial: true) { _, box in registry?.box(id, box.isEmpty ? nil : box) }
            .onDisappear { registry?.box(id, nil) }
    }
}

struct GlassArcRegistration: ViewModifier {
    let arc: JSON
    let strength: Double
    @Environment(\.glassRegistry) private var registry
    @Environment(\.workspaceChromeOpacity) private var opacity
    @State private var id = UUID()
    @State private var geometry = GlassGeometry()
    private var region: [CGFloat] {
        region(in: geometry.bounds, scale: geometry.scale, opacity: opacity)
    }
    func region(in bounds: CGRect, scale: CGFloat, opacity: Double) -> [CGFloat] {
        guard opacity > 0, strength > 0, !bounds.isEmpty, arc["radius"].number > 0 else { return [] }
        let radius = arc["radius"].number
        let extra = 21 / radius * 180 / .pi, direction = arc["sweep"].number < 0 ? -1.0 : 1.0
        return [bounds.midX, bounds.midY, radius * scale, arc["band"].number * scale,
            (arc["start"].number - direction * extra) * .pi / 180,
            (arc["sweep"].number + direction * extra) * .pi / 180, strength * opacity]
    }
    func body(content: Content) -> some View {
        content.onGeometryChange(for: GlassGeometry.self) { GlassGeometry($0) } action: { geometry = $0 }
            .onChange(of: region, initial: true) { _, value in registry?.arc(id, value.isEmpty ? nil : value) }
            .onDisappear { registry?.arc(id, nil) }
    }
}

struct GlassColorBubbleRegistration: ViewModifier {
    let color: JSON
    let strength: Double
    @Environment(\.glassRegistry) private var registry
    @Environment(\.workspaceChromeOpacity) private var opacity
    @State private var id = UUID()
    @State private var geometry = GlassGeometry()
    private var region: [CGFloat] {
        let bounds = geometry.bounds, source = color["bounds"], shape = color["glass"], scale = geometry.scale
        guard opacity > 0, strength > 0, !bounds.isEmpty, !shape.isNull else { return [] }
        return [bounds.minX - source["x"].number * scale, bounds.minY - source["y"].number * scale,
            shape[0].number * scale, shape[1].number * scale, shape[2].number * scale, shape[3].number, strength * opacity]
    }
    func body(content: Content) -> some View {
        content.onGeometryChange(for: GlassGeometry.self) { GlassGeometry($0) } action: { geometry = $0 }
            .onChange(of: region, initial: true) { _, value in registry?.colorBubble(id, value.isEmpty ? nil : value) }
            .onDisappear { registry?.colorBubble(id, nil) }
    }
}

struct GlassConnection: ViewModifier {
    let key: String
    let connection: JSON
    @Environment(\.glassRegistry) private var registry
    @Environment(\.workspaceChromeOpacity) private var opacity
    private var projected: JSON { connection.replacing("strength", with: JSON(opacity)) }
    func body(content: Content) -> some View {
        content.onChange(of: projected.stableKey, initial: true) { _, _ in registry?.connection(key, connection.isNull || opacity <= 0 ? nil : projected) }
            .onDisappear { registry?.connection(key, nil) }
    }
}

extension View {
    func glassSurface(_ shape: SquircleShape, fill: Color, register: Bool = true) -> some View {
        background { shape.fill(fill) }.modifier(GlassRegistration(shape: shape, active: register))
    }
}

enum DrawerShadow {
    static func cuts(placement: JSON, connection: JSON) -> [CGRect] {
        connection.isNull ? [] : [connection["bounds"].rect, placement["anchor"].rect]
    }
}

struct OutsideShadow<S: Shape>: View {
    let shape: S
    let opacity: Double
    let radius: CGFloat
    let y: CGFloat
    var cuts: [CGRect] = []
    var body: some View {
        GeometryReader { geometry in
            let margin = ceil(radius * 3 + abs(y))
            Canvas { context, size in
                let rect = CGRect(x: margin, y: margin, width: size.width - margin * 2, height: size.height - margin * 2)
                let path = shape.path(in: rect)
                context.drawLayer { layer in
                    layer.addFilter(.shadow(color: .black.opacity(opacity), radius: radius, x: 0, y: y, options: .shadowOnly))
                    layer.fill(path, with: .color(.black))
                }
                context.blendMode = .clear
                context.fill(path, with: .color(.black))
                for cut in cuts { context.fill(Path(cut.offsetBy(dx: margin, dy: margin)), with: .color(.black)) }
            }.frame(width: geometry.size.width + margin * 2, height: geometry.size.height + margin * 2)
                .offset(x: -margin, y: -margin)
        }.allowsHitTesting(false).accessibilityHidden(true)
    }
}
