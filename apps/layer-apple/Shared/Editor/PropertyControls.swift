import SwiftUI

/// Schema, values, validation and sampled curves are all supplied by Rust.
/// These views only present controls and translate native editing gestures.
struct LayerPropertiesPanel: View {
    @ObservedObject var store: EditorStore
    @State private var selectedCurve = ""
    private var view: JSON { store.state["layer_properties"] }
    private var controls: [JSON] { view["controls"].array }
    private var curves: [JSON] { controls.filter { $0["kind"]["kind"].string == "curve" } }
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(view["title"].string).fontWeight(.bold).help(view["description"].string)
            if let curve = curves.first(where: { $0["key"].string == selectedCurve }) ?? curves.first {
                PropertyChoice(label: "Channel", options: curves.map { $0["label"].string },
                    selected: curves.firstIndex { $0["key"].string == curve["key"].string } ?? 0, identifier: "property-channel",
                    background: EditorPalette(source: store.state["palette"])["input"]) {
                    selectedCurve = curves[$0]["key"].string
                }
                CurveProperty(store: store, layer: view["layer"].uint, control: curve)
                    .id("\(view["layer"].uint):\(curve["key"].string)")
            }
            ForEach(controls.indices, id: \.self) { index in
                let control = controls[index]
                if control["kind"]["kind"].string != "curve" {
                    if index == 0 || control["section"].string != controls[index - 1]["section"].string {
                        if index > 0 { Divider().padding(.vertical, 3) }
                        if !control["section"].isNull { Text(control["section"].string).fontWeight(.bold).padding(.leading, 6) }
                    }
                    PropertyField(store: store, layer: view["layer"].uint, control: control)
                        .id("\(view["layer"].uint):\(control["key"].string):\(control["kind"]["kind"].string)")
                }
            }
        }.disabled(!view["enabled"].bool).opacity(view["enabled"].bool ? 1 : 0.4)
            .accessibilityElement(children: .contain).accessibilityIdentifier("layer-properties")
            .onChange(of: view["layer"].uint) { _, _ in selectedCurve = "" }
    }
}

private struct PropertyField: View {
    @ObservedObject var store: EditorStore
    let layer: UInt64
    let control: JSON
    private var key: String { control["key"].string }
    private var kind: String { control["kind"]["kind"].string }
    private var label: String { control["label"].string }
    private var value: JSON { control["value"]["value"] }
    private func change(_ value: Any, completion: @escaping @MainActor (String?) -> Void) {
        store.edit(["type": "effect", "action": ["op": "set", "layer": layer, "key": key,
            "value": ["kind": kind, "value": value]]], completion: completion)
    }
    private func change(_ value: Any) { change(value) { if let error = $0 { store.failure = error } } }
    var body: some View {
        field.contextMenu {
            Button("Reset") { store.effect(layer, key: key, action: ["op": "reset"]) }
        }
    }
    @ViewBuilder private var field: some View {
        switch kind {
        case "number":
            NumberControl(store: store, label: label, value: value.number, control: control["kind"]["numeric"],
                identifier: "property-" + key) { change($0, completion: $1) }
                .id(control["kind"].stableKey + label)
        case "toggle":
            Toggle(label, isOn: Binding(get: { value.bool }, set: { change($0) }))
                .toggleStyle(.switch).controlSize(.small).accessibilityIdentifier("property-" + key)
        case "choice":
            VStack(alignment: .leading, spacing: 2) {
                Text(label)
                PropertyChoice(label: label, options: control["kind"]["options"].array.map(\.string),
                    selected: Int(value.uint), identifier: "property-" + key,
                    background: EditorPalette(source: store.state["palette"])["input"]) { change($0) }
            }
        case "color":
            PropertyColor(store: store, label: label, identifier: "property-" + key, value: value) { change($0, completion: $1) }
        case "gradient": GradientProperty(store: store, layer: layer, control: control)
        default: EmptyView()
        }
    }
}

private struct PropertyChoice: View {
    let label: String
    let options: [String]
    let selected: Int
    let identifier: String
    let background: Color
    let select: (Int) -> Void
    var body: some View {
        Menu {
            ForEach(options.indices, id: \.self) { index in
                Button { select(index) } label: {
                    if index == selected { Label(options[index], systemImage: "checkmark") }
                    else { Text(options[index]) }
                }.accessibilityIdentifier(identifier + "-option-\(index)")
            }
        } label: {
            HStack {
                Text(options.indices.contains(selected) ? options[selected] : label).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                SharedIcon(name: "chevron-down")
            }.padding(6).background(background, in: RoundedRectangle(cornerRadius: 6)).contentShape(RoundedRectangle(cornerRadius: 6))
        }.menuStyle(.borderlessButton).menuIndicator(.hidden)
            .accessibilityLabel(label).accessibilityValue(options.indices.contains(selected) ? options[selected] : "")
            .accessibilityIdentifier(identifier)
    }
}

private struct PropertyColor: View {
    @ObservedObject var store: EditorStore
    let label: String
    let identifier: String
    let value: JSON
    let change: (Any, @escaping @MainActor (String?) -> Void) -> Void
    @State private var expanded = false
    var body: some View {
        VStack(spacing: 4) {
            HStack {
                Text(label).frame(maxWidth: .infinity, alignment: .leading)
                Button { expanded.toggle() } label: {
                    RoundedRectangle(cornerRadius: 6).fill(value.effectColor).frame(width: 48, height: 28)
                        .overlay(RoundedRectangle(cornerRadius: 6).stroke(.primary.opacity(0.3), lineWidth: 1))
                }.buttonStyle(.plain).accessibilityLabel(label).accessibilityIdentifier(identifier + "-color")
            }
            if expanded {
                ForEach(0..<4, id: \.self) { index in
                    NumberControl(store: store, label: ["Red", "Green", "Blue", "Alpha"][index],
                        value: value[index].number, control: store.catalog["opacity"], identifier: identifier + "-rgba-\(index)") { next, completion in
                        var color = value.array.map(\.number)
                        guard color.count == 4 else { return }
                        color[index] = next; change(color, completion)
                    }
                }
            }
        }
    }
}

private struct CurveProperty: View {
    @ObservedObject var store: EditorStore
    let layer: UInt64
    let control: JSON
    @Environment(\.isEnabled) private var enabled
    @GestureState private var contact = false
    @State private var selected: Int?
    @State private var dragging = false
    @State private var dragIndex: Int?
    private var key: String { control["key"].string }
    private var points: [JSON] { control["value"]["value"].array }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var removable: Bool { selected.map { $0 > 0 && $0 < points.count - 1 } ?? false }
    private func change(_ point: [Double], index: Int?, remove: Bool = false) {
        store.effect(layer, key: key, action: ["op": "curve_point", "index": index as Any? ?? NSNull(), "point": point, "remove": remove])
    }
    var body: some View {
        VStack(spacing: 6) {
            GeometryReader { geometry in
                Canvas { context, size in
                    var grid = Path()
                    for i in 1...3 {
                        let fraction = CGFloat(i) / 4
                        grid.move(to: CGPoint(x: size.width * fraction, y: 0)); grid.addLine(to: CGPoint(x: size.width * fraction, y: size.height))
                        grid.move(to: CGPoint(x: 0, y: size.height * fraction)); grid.addLine(to: CGPoint(x: size.width, y: size.height * fraction))
                    }
                    context.stroke(grid, with: .color(palette["text"].opacity(0.2)), lineWidth: 1)
                    var curve = Path()
                    for (index, p) in control["plot"].array.enumerated() {
                        let point = CGPoint(x: p[0].number * size.width, y: (1 - p[1].number) * size.height)
                        if index == 0 { curve.move(to: point) } else { curve.addLine(to: point) }
                    }
                    context.stroke(curve, with: .color(palette["text"]), lineWidth: 1.5)
                    for (index, p) in points.enumerated() {
                        let radius: CGFloat = selected == index ? 5 : 3.5
                        context.fill(Path(ellipseIn: CGRect(x: p[0].number * size.width - radius,
                            y: (1 - p[1].number) * size.height - radius, width: radius * 2, height: radius * 2)), with: .color(palette["text"]))
                    }
                }.background(palette["input"], in: RoundedRectangle(cornerRadius: 6)).contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0).updating($contact) { _, active, _ in active = true }.onChanged { event in
                        let size = geometry.size
                        guard size.width > 0, size.height > 0 else { return }
                        if !dragging {
                            dragging = true
                            dragIndex = points.indices.min { a, b in distance(points[a], event.startLocation, size) < distance(points[b], event.startLocation, size) }
                            if let index = dragIndex, distance(points[index], event.startLocation, size) > 12 { dragIndex = nil }
                            selected = dragIndex
                        }
                        if let index = dragIndex { change([event.location.x / size.width, 1 - event.location.y / size.height], index: index) }
                    }.onEnded { event in
                        if dragIndex == nil, geometry.size.width > 0, geometry.size.height > 0 {
                            // Commit insertion once. Subsequent drags address the
                            // actual returned model, never a guessed insertion index.
                            change([event.location.x / geometry.size.width, 1 - event.location.y / geometry.size.height], index: nil)
                        }
                        dragging = false; dragIndex = nil
                    })
                    .allowsHitTesting(enabled)
                    .onChange(of: contact) { _, active in if !active { dragging = false; dragIndex = nil } }
                    .accessibilityLabel(control["label"].string).accessibilityValue("\(points.count) points")
                    .accessibilityIdentifier("effect-curve")
            }.frame(height: 200)
            HStack {
                Button("Remove point") { change([0, 0], index: selected, remove: true); selected = nil }
                    .disabled(!removable).accessibilityIdentifier("curve-remove")
                Spacer(minLength: 0)
                Button("Reset") { selected = nil; store.effect(layer, key: key, action: ["op": "reset"]) }
                    .accessibilityIdentifier("curve-reset")
            }.buttonStyle(.plain)
        }
    }
    private func distance(_ point: JSON, _ location: CGPoint, _ size: CGSize) -> CGFloat {
        hypot(point[0].number * size.width - location.x, (1 - point[1].number) * size.height - location.y)
    }
}

private struct GradientProperty: View {
    @ObservedObject var store: EditorStore
    let layer: UInt64
    let control: JSON
    @Environment(\.isEnabled) private var enabled
    @GestureState private var contact = false
    @State private var selected = 0
    @State private var dragging = false
    @State private var dragIndex: Int?
    private var key: String { control["key"].string }
    private var stops: [JSON] { control["value"]["value"].array }
    private var index: Int { max(0, min(selected, stops.count - 1)) }
    private var removable: Bool { index > 0 && index < stops.count - 1 }
    private func change(_ position: Double, index: Int?, color: Any = NSNull(), remove: Bool = false,
        completion: (@MainActor (String?) -> Void)? = nil) {
        store.edit(["type": "effect", "action": ["op": "gradient_stop", "layer": layer, "key": key,
            "index": index as Any? ?? NSNull(), "position": position, "color": color, "remove": remove]]) { error in
                if let completion { completion(error) } else if let error { store.failure = error }
            }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            GeometryReader { geometry in
                Canvas { context, size in
                    let width = max(1, size.width - 12)
                    let gradient = Gradient(stops: stops.map { Gradient.Stop(color: $0["color"].effectColor, location: $0["position"].number) })
                    context.fill(Path(CGRect(x: 6, y: 0, width: width, height: 32)), with: .linearGradient(gradient,
                        startPoint: CGPoint(x: 6, y: 0), endPoint: CGPoint(x: size.width - 6, y: 0)))
                    for (i, stop) in stops.enumerated() {
                        let radius: CGFloat = i == index ? 4 : 2.5
                        context.fill(Path(ellipseIn: CGRect(x: 6 + stop["position"].number * width - radius, y: 39 - radius,
                            width: radius * 2, height: radius * 2)), with: .foreground)
                    }
                }.contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 0).updating($contact) { _, active, _ in active = true }.onChanged { event in
                    let width = max(1, geometry.size.width - 12)
                    if !dragging {
                        dragging = true
                        dragIndex = stops.indices.min { abs(6 + stops[$0]["position"].number * width - event.startLocation.x) < abs(6 + stops[$1]["position"].number * width - event.startLocation.x) }
                        if let found = dragIndex, abs(6 + stops[found]["position"].number * width - event.startLocation.x) > 12 { dragIndex = nil }
                        if let found = dragIndex { selected = found }
                    }
                    if let found = dragIndex, event.translation.width != 0 { change((event.location.x - 6) / width, index: found) }
                }.onEnded { event in
                    if dragIndex == nil { change((event.location.x - 6) / max(1, geometry.size.width - 12), index: nil) }
                    dragging = false; dragIndex = nil
                }).allowsHitTesting(enabled)
                    .onChange(of: contact) { _, active in if !active { dragging = false; dragIndex = nil } }
                    .accessibilityIdentifier("effect-gradient").accessibilityLabel(control["label"].string)
                    .accessibilityValue("\(stops.count) stops")
            }.frame(height: 44)
            if !stops.isEmpty {
                NumberControl(store: store, label: "Position", value: stops[index]["position"].number,
                    control: store.catalog["opacity"], identifier: "gradient-position") { change($0, index: index, completion: $1) }
                    .disabled(!removable).id(index)
                PropertyColor(store: store, label: "Color", identifier: "gradient-stop", value: stops[index]["color"]) {
                    change(stops[index]["position"].number, index: index, color: $0, completion: $1)
                }.id(index)
                NumberControl(store: store, label: "Opacity", value: stops[index]["color"][3].number,
                    control: store.catalog["opacity"], identifier: "gradient-opacity") { value, completion in
                    var color = stops[index]["color"].array.map(\.number)
                    guard color.count == 4 else { return }
                    color[3] = value
                    change(stops[index]["position"].number, index: index, color: color, completion: completion)
                }.id(index)
            }
            HStack {
                Button("Remove stop") { change(0, index: index, remove: true); selected = max(0, index - 1) }
                    .disabled(!removable).accessibilityIdentifier("gradient-remove")
                Spacer(minLength: 0)
                Button("Reset") { selected = 0; store.effect(layer, key: key, action: ["op": "reset"]) }
                    .accessibilityIdentifier("gradient-reset")
            }.buttonStyle(.plain)
        }
    }
}

private extension JSON {
    var effectColor: Color { Color(.sRGB, red: self[0].number, green: self[1].number, blue: self[2].number, opacity: self[3].number) }
}
private extension EditorStore {
    func effect(_ layer: UInt64, key: String, action: [String: Any]) {
        var action = action; action["layer"] = layer; action["key"] = key
        dispatch(["type": "effect", "action": action])
    }
}
