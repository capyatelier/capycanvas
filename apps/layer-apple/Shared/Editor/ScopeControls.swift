import SwiftUI

struct ScopeGraph: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var scopes: ScopePlots
    let kind: String
    var body: some View {
        let view = store.state[kind], plot = scopes.plots[kind], colors = scopes.colors
        Canvas { graphics, size in
            if let image = plot?.image {
                graphics.withCGContext { context in
                    context.interpolationQuality = .none
                    context.translateBy(x: 0, y: size.height); context.scaleBy(x: 1, y: -1)
                    context.draw(image, in: CGRect(origin: .zero, size: size))
                }
            }
            for (channel, values) in plot?.bins ?? [] where colors.indices.contains(channel) {
                let width = size.width / CGFloat(max(1, values.count))
                var path = Path()
                for (x, value) in values.enumerated() {
                    path.addRect(CGRect(x: CGFloat(x) * width, y: size.height * (1 - value), width: width + 0.1, height: size.height * value))
                }
                graphics.fill(path, with: .color(colors[channel].opacity(0.55)))
            }
        }.accessibilityElement().accessibilityAddTraits(.isImage)
            .accessibilityLabel([view["description"].string, view["range"].string].filter { !$0.isEmpty }.joined(separator: "\n"))
            .accessibilityIdentifier("scope-\(kind)-chart")
    }
}

struct ScopeFooter: View {
    @ObservedObject var store: EditorStore
    let kind: String
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func send(_ type: String, _ enabled: Bool) {
        store.dispatch(["type": "histogram", "action": ["type": type, "enabled": enabled]])
    }
    var body: some View {
        let view = store.state[kind], histogram = store.state["histogram"]
        HStack(spacing: store.panelMetrics.spacing) {
            Text(view["status"].string).lineLimit(1).truncationMode(.tail).foregroundStyle(palette["text"].opacity(0.55))
                .frame(maxWidth: .infinity, alignment: .leading).help(view["status"].string)
                .accessibilityIdentifier("scope-\(kind)-status")
            ForEach(Array(["shadows", "highlights"].enumerated()), id: \.offset) { index, name in
                let on = histogram[name].bool
                Button { send(name, !on) } label: {
                    SharedIcon(name: "tonal-" + name).foregroundStyle(on ? palette.accent : palette["text"])
                        .frame(width: store.panelMetrics.height, height: store.panelMetrics.height).contentShape(Rectangle())
                }.buttonStyle(.plain).help(histogram["labels"][index + 1].string)
                    .accessibilityLabel(histogram["labels"][index + 1].string)
                    .accessibilityAddTraits(on ? .isSelected : []).accessibilityIdentifier("scope-\(kind)-\(name)")
            }
        }
    }
}

struct ScopeControl: View {
    @ObservedObject var store: EditorStore
    let kind: String
    var tonal = false
    var maximumHeight: CGFloat? = nil
    @Environment(\.measuresWorkspacePanel) private var measuresWorkspacePanel
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func select(_ type: String, _ index: Int) {
        store.dispatch(["type": "histogram", "action": ["type": type, "index": index]])
    }
    var body: some View {
        let view = store.state[kind], waveform = kind == "waveform", copy = store.catalog["native_copy"]
        ScopeLayout(graph: tonal ? 0 : 1, maximumHeight: maximumHeight, spacing: store.panelMetrics.spacing) {
            if !tonal {
                HStack(spacing: store.panelMetrics.spacing) {
                    EditorChoice(label: copy["sampler"]["source"].string, options: view["sources"].array.map(\.string),
                        selected: Int(view["source"].uint), identifier: "scope-\(kind)-source", background: palette.controlFill, height: store.panelMetrics.height) { select("source", $0) }
                    EditorChoice(label: copy["color"]["channel"].string, options: view["channels"].array.map(\.string),
                        selected: Int(view["channel"].uint), identifier: "scope-\(kind)-channel", background: palette.controlFill, height: store.panelMetrics.height) {
                        select(waveform ? "waveform_channel" : "channel", $0)
                    }
                }.modifier(PanelBodyMeasurement(panel: kind, part: "toolbar", spacing: store.panelMetrics.spacing))
            }
            ScopeGraph(store: store, scopes: store.scopes, kind: kind)
                .frame(minHeight: 0, idealHeight: tonal ? 120 : 160, maxHeight: tonal ? 120 : .infinity)
                .overlay(alignment: .leading) {
                    if waveform {
                        VStack(alignment: .leading) {
                            Text(view["axis"][1].string); Spacer(minLength: 0); Text(view["axis"][0].string)
                        }.padding(.leading, 3).foregroundStyle(palette["text"].opacity(0.55)).allowsHitTesting(false)
                    }
                }.clipped().modifier(PanelBodyMeasurement(panel: kind, part: "graph", intrinsicHeight: tonal ? 120 : 160,
                    spacing: store.panelMetrics.spacing))
            if !tonal && !waveform {
                HStack { Text(view["axis"][0].string); Spacer(minLength: 0); Text(view["axis"][1].string) }
                    .foregroundStyle(palette["text"].opacity(0.55))
                    .modifier(PanelBodyMeasurement(panel: kind, part: "axis", spacing: store.panelMetrics.spacing))
            }
            if !tonal {
                Toggle(view["labels"][0].string, isOn: Binding(get: { view["logarithmic"].bool },
                    set: { store.dispatch(["type": "histogram", "action": ["type": waveform ? "waveform_logarithmic" : "logarithmic", "enabled": $0]]) }))
                    .toggleStyle(PanelCheckStyle(minHeight: store.panelMetrics.height, lineLimit: nil))
                    .accessibilityIdentifier("scope-\(kind)-log")
                    .modifier(PanelBodyMeasurement(panel: kind, part: "log", spacing: store.panelMetrics.spacing))
            }
            ScopeFooter(store: store, kind: kind).modifier(PanelBodyMeasurement(panel: kind, part: "footer"))
        }.environment(\.measuresWorkspacePanel, measuresWorkspacePanel && !tonal)
            .accessibilityElement(children: .contain).accessibilityIdentifier("scope-" + kind)
    }
}

private struct ScopeLayout: Layout {
    let graph: Int
    let maximumHeight: CGFloat?
    let spacing: CGFloat
    private func sizes(_ proposal: ProposedViewSize, _ subviews: Subviews) -> [CGSize] {
        subviews.map { $0.sizeThatFits(ProposedViewSize(width: proposal.width, height: nil)) }
    }
    private func fixedHeight(_ sizes: [CGSize]) -> CGFloat {
        sizes.enumerated().filter { $0.offset != graph }.reduce(0) { $0 + $1.element.height }
            + CGFloat(max(0, sizes.count - 1)) * spacing
    }
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let sizes = sizes(proposal, subviews), fixed = fixedHeight(sizes), natural = fixed + sizes[graph].height
        return CGSize(width: proposal.width ?? sizes.map(\.width).max() ?? 0,
            height: max(fixed, maximumHeight ?? natural))
    }
    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let sizes = sizes(ProposedViewSize(width: bounds.width, height: nil), subviews)
        let graphHeight = max(0, bounds.height - fixedHeight(sizes))
        var y = bounds.minY
        for (index, view) in subviews.enumerated() {
            let height = index == graph ? graphHeight : sizes[index].height
            view.place(at: CGPoint(x: bounds.minX, y: y), anchor: .topLeading,
                proposal: ProposedViewSize(width: bounds.width, height: height))
            y += height + spacing
        }
    }
}
