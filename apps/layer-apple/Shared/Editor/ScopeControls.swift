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
    var logarithmic = false
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func send(_ type: String, _ enabled: Bool) {
        store.dispatch(["type": "histogram", "action": ["type": type, "enabled": enabled]])
    }
    var body: some View {
        let view = store.state[kind], histogram = store.state["histogram"]
        HStack(spacing: 4) {
            Text(view["status"].string).lineLimit(1).truncationMode(.tail).foregroundStyle(palette["text"].opacity(0.7))
                .frame(maxWidth: .infinity, alignment: .leading).help(view["status"].string)
                .accessibilityIdentifier("scope-\(kind)-status")
            if logarithmic {
                Toggle(view["labels"][0].string, isOn: Binding(get: { view["logarithmic"].bool },
                    set: { send(kind == "waveform" ? "waveform_logarithmic" : "logarithmic", $0) }))
                    #if os(macOS)
                    .toggleStyle(.checkbox)
                    #else
                    .toggleStyle(.switch).controlSize(.mini)
                    #endif
                    .lineLimit(1).fixedSize().accessibilityIdentifier("scope-\(kind)-log")
            }
            ForEach(Array(["shadows", "highlights"].enumerated()), id: \.offset) { index, name in
                let on = histogram[name].bool
                Button { send(name, !on) } label: {
                    SharedIcon(name: "tonal-" + name).foregroundStyle(on ? palette.accent : palette["text"])
                        .frame(width: 28, height: 28).contentShape(Rectangle())
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
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func select(_ type: String, _ index: Int) {
        store.dispatch(["type": "histogram", "action": ["type": type, "index": index]])
    }
    var body: some View {
        let view = store.state[kind], waveform = kind == "waveform", copy = store.catalog["native_copy"]
        VStack(alignment: .leading, spacing: 4) {
            if !tonal {
                HStack(spacing: 4) {
                    EditorChoice(label: copy["sampler"]["source"].string, options: view["sources"].array.map(\.string),
                        selected: Int(view["source"].uint), identifier: "scope-\(kind)-source", background: palette["input"]) { select("source", $0) }
                    EditorChoice(label: copy["color"]["channel"].string, options: view["channels"].array.map(\.string),
                        selected: Int(view["channel"].uint), identifier: "scope-\(kind)-channel", background: palette["input"]) {
                        select(waveform ? "waveform_channel" : "channel", $0)
                    }
                }
            }
            ScopeGraph(store: store, scopes: store.scopes, kind: kind).frame(height: tonal ? 120 : 160)
                .overlay(alignment: .leading) {
                    if waveform {
                        VStack(alignment: .leading) {
                            Text(view["axis"][1].string); Spacer(minLength: 0); Text(view["axis"][0].string)
                        }.padding(.leading, 3).foregroundStyle(palette["text"].opacity(0.7)).allowsHitTesting(false)
                    }
                }
            if !tonal && !waveform {
                HStack { Text(view["axis"][0].string); Spacer(minLength: 0); Text(view["axis"][1].string) }
                    .foregroundStyle(palette["text"].opacity(0.7))
            }
            ScopeFooter(store: store, kind: kind, logarithmic: !tonal)
        }.accessibilityElement(children: .contain).accessibilityIdentifier("scope-" + kind)
    }
}
