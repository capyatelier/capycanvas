import SwiftUI

struct PanelMeasurementOrigin: Hashable {
    let request: JSON
    let context: JSON
    let key: String
    init(request: JSON, context: JSON) {
        self.request = request; self.context = context
        key = JSON([request["tab"].raw, request["body"].raw, request["width"].raw, request["split_filters"].raw, context.raw]).stableKey
    }
    static func ==(lhs: Self, rhs: Self) -> Bool { lhs.key == rhs.key }
    func hash(into hasher: inout Hasher) { hasher.combine(key) }
}

@MainActor final class PanelMeasurements {
    private weak var store: EditorStore?
    private struct Tab { let origin: PanelMeasurementOrigin; let width: CGFloat }
    private struct Body {
        let origin: PanelMeasurementOrigin
        var width: CGFloat = 0
        var height: CGFloat = 0
        var scrollHeight: CGFloat?
        var unitHeight: CGFloat = 0
        var hasContent = false
    }
    private var widths: [String: Tab] = [:]
    private var bodies: [String: Body] = [:]
    private var current: [String: PanelMeasurementOrigin] = [:]
    private var context = JSON()
    private var scheduled = false
    private var sending = false
    private var lastReport: String?

    init(store: EditorStore) { self.store = store }

    func origins(scale: CGFloat, style: String) -> [String: PanelMeasurementOrigin] {
        guard let store else { return [:] }
        if context["scale"].number != Double(scale) || context["style"].string != style {
            context = JSON(["generation": context["generation"].uint + 1, "scale": scale, "text_scale": 1, "style": style])
        }
        current = Dictionary(uniqueKeysWithValues: store.snapshot["layout"]["measurement_requests"].array.map { request in
            let panel = request["panel"].string
            return (panel, PanelMeasurementOrigin(request: request, context: context))
        })
        return current
    }

    private func accepts(_ origin: PanelMeasurementOrigin, tab: Bool) -> Bool {
        let panel = origin.request["panel"].string
        guard let wanted = current[panel], wanted.context.stableKey == origin.context.stableKey else { return false }
        let key = tab ? "tab" : "body"
        return wanted.request[key].string == origin.request[key].string
    }

    func receive(_ facts: [PanelSizeKey: CGFloat]) {
        let previous = bodies
        bodies = [:]
        for (key, value) in facts where value.isFinite && value >= 0 {
            guard let origin = key.origin, accepts(origin, tab: key.kind == .tab) else { continue }
            if key.kind == .tab {
                widths[key.panel] = Tab(origin: origin, width: value)
                continue
            }
            let token = origin.key
            var body = bodies[token] ?? Body(origin: origin)
            switch key.kind {
            case .tab: break
            case .width: body.width = value
            case .fixed: body.height += value; body.hasContent = true
            case .scroll:
                body.height += value
                body.scrollHeight = (body.scrollHeight ?? 0) + value
                body.hasContent = true
            case .unit: body.unitHeight = max(body.unitHeight, value)
            }
            bodies[token] = body
        }
        for token in bodies.keys where bodies[token]?.unitHeight == 0 {
            bodies[token]?.unitHeight = previous[token]?.unitHeight ?? 0
        }
        reconcile()
    }

    func reconcile() {
        guard !scheduled && !sending else { return }
        scheduled = true
        Task { @MainActor [weak self] in
            await Task.yield()
            guard let self else { return }
            scheduled = false
            publish()
        }
    }

    private func publish() {
        guard let store, !context.isNull else { return }
        widths = widths.filter { accepts($0.value.origin, tab: true) }
        let quantize: (CGFloat) -> Double = { Double(ceil($0 * 64) / 64) }
        let reports = current.keys.sorted().compactMap { panel -> [String: Any]? in
            guard let wanted = current[panel] else { return nil }
            let tab = widths[panel]
            let candidates = bodies.values.filter { body in
                body.origin.request["panel"].string == panel && body.hasContent && body.width > 0 && accepts(body.origin, tab: false)
                    && abs(Double(body.width) - body.origin.request["width"].number) < 0.25
            }.sorted { $0.width < $1.width }
            let body = candidates.first { body in
                abs(Double(body.width) - wanted.request["width"].number) < 0.25
                    && body.origin.request["split_filters"].bool == wanted.request["split_filters"].bool
            } ?? candidates.first
            guard tab != nil || body != nil else { return nil }
            let origin = body?.origin ?? tab!.origin
            let measuredTab = tab.flatMap { tab in tab.origin.request["tab"].string == origin.request["tab"].string ? quantize(tab.width) : nil }
            var report: [String: Any] = ["request": origin.request.raw, "context": origin.context.raw,
                "tab_width": measuredTab as Any? ?? NSNull(), "body": NSNull()]
            if let body {
                var measured: [String: Any] = ["actual_width": quantize(body.width), "content_height": quantize(body.height)]
                if let scroll = body.scrollHeight {
                    measured["scroll"] = ["fixed_height": quantize(max(0, body.height - scroll)), "unit_height": quantize(body.unitHeight)]
                }
                report["body"] = measured
            }
            return report
        }
        let batch = JSON(["context": context.raw, "reports": reports])
        guard batch.stableKey != lastReport else { return }
        lastReport = batch.stableKey
        sending = true
        store.edit(["type": "measure_panels", "measurements": batch.raw]) { [weak self] error in
            guard let self else { return }
            sending = false
            if let error { store.failure = error }
            else { reconcile() }
        }
    }
}

struct PanelSizeKey: Hashable {
    enum Kind: Hashable { case tab, fixed, scroll, unit, width }
    let panel: String
    let part: String
    var kind: Kind = .fixed
    var origin: PanelMeasurementOrigin? = nil
}
struct PanelSizeFacts: PreferenceKey {
    static var defaultValue: [PanelSizeKey: CGFloat] { [:] }
    static func reduce(value: inout [PanelSizeKey: CGFloat], nextValue: () -> [PanelSizeKey: CGFloat]) {
        value.merge(nextValue(), uniquingKeysWith: max)
    }
}
private struct PanelMeasurementOrigins: EnvironmentKey { static let defaultValue: [String: PanelMeasurementOrigin] = [:] }
private struct PanelMeasurementEnabled: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var workspacePanelMeasurementOrigins: [String: PanelMeasurementOrigin] {
        get { self[PanelMeasurementOrigins.self] }
        set { self[PanelMeasurementOrigins.self] = newValue }
    }
    var measuresWorkspacePanel: Bool {
        get { self[PanelMeasurementEnabled.self] }
        set { self[PanelMeasurementEnabled.self] = newValue }
    }
}

/// Measures content before its enclosing ScrollView applies viewport clipping.
struct PanelBodyMeasurement: ViewModifier {
    let panel: String
    var part = "body"
    var intrinsicHeight: CGFloat? = nil
    var kind: PanelSizeKey.Kind = .fixed
    var spacing: CGFloat = 0
    var naturalHeight: ((CGFloat) -> CGFloat)? = nil
    @Environment(\.measuresWorkspacePanel) private var enabled
    @Environment(\.workspacePanelMeasurementOrigins) private var origins
    func body(content: Content) -> some View {
        content.background(GeometryReader { allocation in
            Color.clear.preference(key: PanelSizeFacts.self, value: enabled
                ? [PanelSizeKey(panel: panel, part: part, kind: kind, origin: origins[panel]):
                    (naturalHeight?(allocation.size.width) ?? intrinsicHeight ?? allocation.size.height) + spacing] : [:])
        })
    }
}

struct PanelMeasurementScope: ViewModifier {
    let panel: String
    var enabled = true
    var splitFilters = false
    @Environment(\.workspacePanelMeasurementOrigins) private var origins
    @State private var width: CGFloat = 0
    private var scoped: [String: PanelMeasurementOrigin] {
        var result = origins
        if enabled, width > 0, let origin = origins[panel] {
            let request = origin.request.replacing("width", with: JSON(Double(width)))
                .replacing("split_filters", with: JSON(splitFilters))
            result[panel] = PanelMeasurementOrigin(request: request, context: origin.context)
        } else { result.removeValue(forKey: panel) }
        return result
    }
    func body(content: Content) -> some View {
        let scoped = scoped
        content.background(GeometryReader { allocation in
            Color.clear.preference(key: PanelSizeFacts.self, value:
                enabled ? [PanelSizeKey(panel: panel, part: "width", kind: .width, origin: scoped[panel]): allocation.size.width] : [:])
        }).environment(\.workspacePanelMeasurementOrigins, scoped)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
    }
}

/// Labels are lightweight, inert copies. No extra panel, thumbnail, filter or
/// Navigator content is mounted to measure an inactive tab.
struct WorkspacePanelLabelMeasurements: View {
    @ObservedObject var store: EditorStore
    @Environment(\.workspacePanelMeasurementOrigins) private var origins
    var body: some View {
        ZStack {
            ForEach(store.snapshot["panels"].array, id: \.measurementPanelID) { panel in
                WorkspaceTabLabel(tab: panel, selected: false, palette: EditorPalette(source: store.state["palette"]))
                    .background(GeometryReader { allocation in
                        Color.clear.preference(key: PanelSizeFacts.self,
                            value: [PanelSizeKey(panel: panel["id"].string, part: "tab", kind: .tab, origin: origins[panel["id"].string]): allocation.size.width])
                    })
            }
        }.hidden().allowsHitTesting(false).accessibilityHidden(true)
    }
}

private extension JSON { var measurementPanelID: String { self["id"].string } }
