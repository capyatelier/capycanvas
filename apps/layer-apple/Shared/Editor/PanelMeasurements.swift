import SwiftUI

/// Transient widget facts. Rust owns fitted widths/heights and workspace history.
/// Only mounted panel bodies contribute; inactive bodies retain their last size.
@MainActor final class PanelMeasurements {
    private weak var store: EditorStore?
    private var widths: [String: CGFloat] = [:]
    private var heights: [String: CGFloat] = [:]
    private var scheduled = false
    private var sending = false

    init(store: EditorStore) { self.store = store }

    func receive(_ facts: [PanelSizeKey: CGFloat]) {
        let bodies = Set(facts.keys.filter { $0.part != "tab" }.map(\.panel))
        for panel in bodies { heights[panel] = 0 }
        for (key, value) in facts where value.isFinite && value >= 0 {
            if key.part == "tab" { widths[key.panel] = value }
            else { heights[key.panel, default: 0] += value }
        }
        reconcile()
    }

    func reconcile() {
        guard !scheduled && !sending && !widths.isEmpty else { return }
        scheduled = true
        Task { @MainActor [weak self] in
            // Geometry preferences can arrive in several batches in one layout.
            await Task.yield()
            guard let self else { return }
            scheduled = false
            publish()
        }
    }

    private func publish() {
        guard let store else { return }
        let ids = store.snapshot["panels"].array.map { $0["id"].string }
        let live = Set(ids)
        widths = widths.filter { live.contains($0.key) }
        heights = heights.filter { live.contains($0.key) }
        let values = ids.compactMap { id -> [String: Any]? in
            guard let width = widths[id] else { return nil }
            // Round upward by at most 1/64 point, avoiding f32/CGFloat feedback
            // while keeping a fitted panel large enough for its actual content.
            let quantize: (CGFloat) -> Double = { Double(ceil($0 * 64) / 64) }
            return ["panel": id, "tab_width": quantize(width), "content_height": quantize(heights[id] ?? 0)]
        }
        let desired = JSON(values)
        guard !SnapshotProjection.equal(desired.raw, store.snapshot["panel_measurements"].raw) else { return }
        sending = true
        store.edit(["type": "measure_panels", "measurements": values]) { [weak self] error in
            guard let self else { return }
            sending = false
            if let error { store.failure = error }
            else { reconcile() }
        }
    }
}

struct PanelSizeKey: Hashable {
    let panel: String
    let part: String
}
struct PanelSizeFacts: PreferenceKey {
    static var defaultValue: [PanelSizeKey: CGFloat] { [:] }
    static func reduce(value: inout [PanelSizeKey: CGFloat], nextValue: () -> [PanelSizeKey: CGFloat]) {
        value.merge(nextValue(), uniquingKeysWith: max)
    }
}
private struct PanelMeasurementEnabled: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
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
    @Environment(\.measuresWorkspacePanel) private var enabled
    func body(content: Content) -> some View {
        content.background(GeometryReader { allocation in
            Color.clear.preference(key: PanelSizeFacts.self, value: enabled
                ? [PanelSizeKey(panel: panel, part: part): intrinsicHeight ?? allocation.size.height] : [:])
        })
    }
}

/// Labels are lightweight, inert copies. No extra panel, thumbnail, filter or
/// Navigator content is mounted to measure an inactive tab.
struct WorkspacePanelLabelMeasurements: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        ZStack {
            ForEach(store.snapshot["panels"].array, id: \.measurementPanelID) { panel in
                WorkspaceTabLabel(tab: panel, selected: false, palette: EditorPalette(source: store.state["palette"]))
                    .background(GeometryReader { allocation in
                        Color.clear.preference(key: PanelSizeFacts.self,
                            value: [PanelSizeKey(panel: panel["id"].string, part: "tab"): allocation.size.width])
                    })
            }
        }.hidden().allowsHitTesting(false).accessibilityHidden(true)
    }
}

private extension JSON { var measurementPanelID: String { self["id"].string } }
