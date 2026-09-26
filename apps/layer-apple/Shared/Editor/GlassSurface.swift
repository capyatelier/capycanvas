import SwiftUI

@MainActor final class GlassRegistry {
    private weak var store: EditorStore?
    private var boxes: [UUID: [CGFloat]] = [:]
    private var connections: [String: JSON] = [:]
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
    private func schedule() {
        guard !scheduled else { return }
        scheduled = true
        DispatchQueue.main.async { [weak self] in self?.flush() }
    }
    private func flush() {
        scheduled = false
        let payload = JSON(["regions": boxes.sorted { $0.key.uuidString < $1.key.uuidString }.map { $0.value.map(Double.init) + [1] },
            "connections": connections.keys.sorted().compactMap { connections[$0]?.raw }])
        let key = payload.stableKey
        guard key != sent else { return }
        sent = key
        store?.native?.glassRegions(payload)
    }
}

private struct GlassRegistryKey: EnvironmentKey { static let defaultValue: GlassRegistry? = nil }
extension EnvironmentValues {
    var glassRegistry: GlassRegistry? {
        get { self[GlassRegistryKey.self] }
        set { self[GlassRegistryKey.self] = newValue }
    }
}

struct GlassRegistration: ViewModifier {
    let shape: SquircleShape
    var active = true
    @Environment(\.glassRegistry) private var registry
    @State private var id = UUID()
    func body(content: Content) -> some View {
        content.onGeometryChange(for: [CGFloat].self) { proxy in
            guard active else { return [] }
            let frame = proxy.frame(in: .named("editor-workspace"))
            return [frame.minX, frame.minY, frame.width, frame.height] + shape.radii(in: CGRect(origin: .zero, size: frame.size))
        } action: { box in registry?.box(id, box.isEmpty ? nil : box) }
            .onDisappear { registry?.box(id, nil) }
    }
}

struct GlassConnection: ViewModifier {
    let key: String
    let connection: JSON
    @Environment(\.glassRegistry) private var registry
    func body(content: Content) -> some View {
        content.onChange(of: connection.stableKey, initial: true) { _, _ in registry?.connection(key, connection.isNull ? nil : connection) }
            .onDisappear { registry?.connection(key, nil) }
    }
}

extension View {
    func glassSurface(_ shape: SquircleShape, fill: Color, register: Bool = true) -> some View {
        background { shape.fill(fill) }.modifier(GlassRegistration(shape: shape, active: register))
    }
}

struct OutsideShadow: View {
    let shape: SquircleShape
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
