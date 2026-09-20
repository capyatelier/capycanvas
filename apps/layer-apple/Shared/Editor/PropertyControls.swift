import SwiftUI

/// Schema, values, validation and sampled curves are all supplied by Rust.
/// These views only present controls and translate native editing gestures.
struct LayerPropertiesPanel: View {
    @ObservedObject var store: EditorStore
    @State private var selectedCurve = ""
    private var view: JSON { store.state["layer_properties"] }
    private var controls: [JSON] { view["controls"].array.filter { $0["section"].string != "Advanced" } }
    private var advanced: [JSON] { view["controls"].array.filter { $0["section"].string == "Advanced" } }
    private var curves: [JSON] { controls.filter { $0["kind"]["kind"].string == "curve" } }
    var body: some View {
        let epoch = store.state["document_file"]["epoch"].uint
        VStack(alignment: .leading, spacing: 6) {
            Text(view["title"].string).fontWeight(.bold).help(view["description"].string)
            if let curve = curves.first(where: { $0["key"].string == selectedCurve }) ?? curves.first {
                EditorChoice(label: "Channel", options: curves.map { $0["label"].string },
                    selected: curves.firstIndex { $0["key"].string == curve["key"].string } ?? 0, identifier: "property-channel",
                    background: EditorPalette(source: store.state["palette"])["input"]) {
                    selectedCurve = curves[$0]["key"].string
                }
                CurveProperty(store: store, layer: view["layer"].uint, epoch: epoch, control: curve,
                    maximum: view["curve_max"].isNull ? nil : view["curve_max"].number)
                    .id("\(epoch):\(view["layer"].uint):\(curve["key"].string)")
            }
            ForEach(controls.indices, id: \.self) { index in
                let control = controls[index]
                if control["kind"]["kind"].string != "curve" {
                    if index == 0 || control["section"].string != controls[index - 1]["section"].string {
                        if index > 0 { Divider().padding(.vertical, 3) }
                        if !control["section"].isNull { Text(control["section"].string).fontWeight(.bold).padding(.leading, 6) }
                    }
                    PropertyField(store: store, layer: view["layer"].uint, epoch: epoch, control: control)
                        .id("\(epoch):\(view["layer"].uint):\(control["key"].string):\(control["kind"]["kind"].string)")
                }
            }
            if !advanced.isEmpty {
                DisclosureGroup("Advanced") {
                    ForEach(advanced.indices, id: \.self) { index in
                        let control = advanced[index]
                        PropertyField(store: store, layer: view["layer"].uint, epoch: epoch, control: control)
                            .id("\(epoch):\(view["layer"].uint):\(control["key"].string):\(control["kind"]["kind"].string)")
                    }
                }
            }
        }.disabled(!view["enabled"].bool).opacity(view["enabled"].bool ? 1 : 0.4)
            .accessibilityElement(children: .contain).accessibilityIdentifier("layer-properties")
            .onChange(of: view["layer"].uint) { _, _ in selectedCurve = "" }
    }
}

private struct PropertyField: View {
    @ObservedObject var store: EditorStore
    @State private var revision: UInt64 = 0
    let layer: UInt64
    let epoch: UInt64
    let control: JSON
    private var key: String { control["key"].string }
    private var kind: String { control["kind"]["kind"].string }
    private var label: String { control["label"].string }
    private var value: JSON { control["value"]["value"] }
    private func effect(_ action: [String: Any], revision: UInt64, phase: String? = nil,
        completion: (@MainActor (String?) -> Void)? = nil) {
        // Reset replaces this editor. Late callbacks must not restore its
        // discarded draft; cancellation still retires the original preview.
        guard phase == "cancel" || revision == self.revision else { completion?(nil); return }
        store.effect(layer, epoch: epoch, key: key, action: action, phase: phase, completion: completion)
    }
    private func change(_ value: Any, revision: UInt64, phase: String? = nil,
        completion: (@MainActor (String?) -> Void)? = nil) {
        effect(["op": "set", "value": ["kind": kind, "value": value]], revision: revision,
            phase: phase, completion: completion ?? { if let error = $0 { store.failure = error } })
    }
    private func reset() {
        revision &+= 1
        store.effect(layer, epoch: epoch, key: key, action: ["op": "reset"])
    }
    var body: some View {
        field(revision: revision).id(revision).contextMenu { Button("Reset", action: reset) }
    }
    @ViewBuilder private func field(revision: UInt64) -> some View {
        switch kind {
        case "number":
            NumberControl(store: store, label: label, value: value.number, control: control["kind"]["numeric"],
                identifier: "property-" + key,
                gestureChange: { change($1, revision: revision, phase: $0, completion: $2) }) {
                change($0, revision: revision, completion: $1)
            }.id(control["kind"].stableKey + label)
        case "toggle":
            Toggle(label, isOn: Binding(get: { value.bool }, set: { change($0, revision: revision) }))
                .toggleStyle(.switch).controlSize(.small).accessibilityIdentifier("property-" + key)
        case "choice":
            PropertyChoiceRow {
                Text(label).lineLimit(1)
                EditorChoice(label: label, options: control["kind"]["options"].array.map(\.string),
                    selected: Int(value.uint), identifier: "property-" + key,
                    background: EditorPalette(source: store.state["palette"])["input"]) { change($0, revision: revision) }
            }
        case "color":
            ManagedColorButton(label: label, identifier: "property-" + key, value: value,
                documentSpace: store.state["colors"]["rgb_space"].string) { change($0.raw, revision: revision) }
        case "gradient":
            GradientProperty(store: store, control: control, effect: {
                effect($0, revision: revision, phase: $1, completion: $2)
            }, reset: reset)
        default: EmptyView()
        }
    }
}

private struct CurveProperty: View {
    @ObservedObject var store: EditorStore
    let layer: UInt64
    let epoch: UInt64
    let control: JSON
    let maximum: Double?
    @Environment(\.isEnabled) private var enabled
    @GestureState private var contact = false
    @State private var selected: Int?
    @State private var dragging = false
    @State private var dragPoint: (index: Int, point: CGPoint)?
    private var key: String { control["key"].string }
    private var points: [JSON] { control["value"]["value"].array }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var removable: Bool { selected.map { $0 > 0 && $0 < points.count - 1 } ?? false }
    private func change(_ point: [Double], index: Int?, remove: Bool = false, phase: String? = nil) {
        store.effect(layer, epoch: epoch, key: key, action: ["op": "curve_point", "index": index as Any? ?? NSNull(), "point": point, "remove": remove], phase: phase)
    }
    private func cancelDrag() {
        if let point = dragPoint { change([0, 0], index: point.index, phase: "cancel") }
        dragging = false; dragPoint = nil
    }
    var body: some View {
        VStack(spacing: 6) {
            GeometryReader { geometry in
                Canvas(colorMode: .extendedLinear) { context, size in
                    var grid = Path()
                    for i in 1...3 {
                        let fraction = CGFloat(i) / 4
                        grid.move(to: CGPoint(x: size.width * fraction, y: 0)); grid.addLine(to: CGPoint(x: size.width * fraction, y: size.height))
                        grid.move(to: CGPoint(x: 0, y: size.height * fraction)); grid.addLine(to: CGPoint(x: size.width, y: size.height * fraction))
                    }
                    context.stroke(grid, with: .color(palette["text"].opacity(0.2)), lineWidth: 1)
                    let ink = palette["text"].opacity(0.7)
                    if let maximum, maximum > 0 {
                        let white = 1 / maximum
                        var reference = Path()
                        reference.move(to: CGPoint(x: white * size.width, y: 0))
                        reference.addLine(to: CGPoint(x: white * size.width, y: size.height))
                        reference.move(to: CGPoint(x: 0, y: (1 - white) * size.height))
                        reference.addLine(to: CGPoint(x: size.width, y: (1 - white) * size.height))
                        context.stroke(reference, with: .color(ink), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                        context.draw(Text("SDR white · 0 EV").font(.system(size: 11)).foregroundColor(ink),
                            at: CGPoint(x: 5, y: 5), anchor: .topLeading)
                        context.draw(Text(String(format: "%.0f · %+.0f EV", maximum, log2(maximum))).font(.system(size: 11)).foregroundColor(ink),
                            at: CGPoint(x: size.width - 5, y: size.height - 5), anchor: .bottomTrailing)
                    } else {
                        context.draw(Text("Output").font(.system(size: 11)).foregroundColor(ink), at: CGPoint(x: 5, y: 5), anchor: .topLeading)
                        context.draw(Text("Input").font(.system(size: 11)).foregroundColor(ink), at: CGPoint(x: size.width - 5, y: size.height - 5), anchor: .bottomTrailing)
                    }
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
                }.background(palette["text"].opacity(0.12)).contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0).updating($contact) { _, active, _ in active = true }.onChanged { event in
                        let size = geometry.size
                        guard size.width > 0, size.height > 0 else { return }
                        if !dragging {
                            dragging = true
                            selected = nil
                            let nearest = points.indices.min { a, b in distance(points[a], event.startLocation, size) < distance(points[b], event.startLocation, size) }
                            if let index = nearest, distance(points[index], event.startLocation, size) <= 12 {
                                selected = index
                                let point = CGPoint(x: points[index][0].number, y: points[index][1].number)
                                dragPoint = (index, point)
                                change([point.x, point.y], index: index, phase: "down")
                            }
                        }
                        if let point = dragPoint, event.translation != .zero {
                            change([point.point.x + event.translation.width / size.width,
                                point.point.y - event.translation.height / size.height], index: point.index, phase: "move")
                        }
                    }.onEnded { event in
                        guard dragging else { return }
                        guard geometry.size.width > 0, geometry.size.height > 0 else { cancelDrag(); return }
                        if let point = dragPoint {
                            change([point.point.x + event.translation.width / geometry.size.width,
                                point.point.y - event.translation.height / geometry.size.height], index: point.index, phase: "up")
                        } else {
                            // Commit insertion once. Subsequent drags address the
                            // actual returned model, never a guessed insertion index.
                            change([event.location.x / geometry.size.width, 1 - event.location.y / geometry.size.height], index: nil)
                        }
                        dragging = false; dragPoint = nil
                    })
                    .allowsHitTesting(enabled)
                    .onChange(of: contact) { _, active in if !active { cancelDrag() } }
                    .onDisappear(perform: cancelDrag)
                    .accessibilityLabel("\(control["label"].string), \(points.count) points")
                    .accessibilityIdentifier("effect-curve")
            }.frame(height: 200)
            HStack {
                Button("Remove point") { change([0, 0], index: selected, remove: true); selected = nil }
                    .disabled(!removable).accessibilityIdentifier("curve-remove")
                Spacer(minLength: 0)
                Button("Reset") { selected = nil; store.effect(layer, epoch: epoch, key: key, action: ["op": "reset"]) }
                    .accessibilityIdentifier("curve-reset")
            }.buttonStyle(.plain)
        }.onChange(of: points.map { $0[0].number }) { previous, current in
            guard current.count == previous.count + 1,
                let inserted = current.firstIndex(where: { !previous.contains($0) }) else { return }
            selected = inserted
        }
    }
    private func distance(_ point: JSON, _ location: CGPoint, _ size: CGSize) -> CGFloat {
        hypot(point[0].number * size.width - location.x, (1 - point[1].number) * size.height - location.y)
    }
}

private struct GradientProperty: View {
    @ObservedObject var store: EditorStore
    let control: JSON
    let effect: ([String: Any], String?, (@MainActor (String?) -> Void)?) -> Void
    let reset: () -> Void
    @Environment(\.isEnabled) private var enabled
    @GestureState private var contact = false
    @State private var selected = 0
    @State private var fieldRevision: UInt64 = 0
    @State private var dragging = false
    @State private var dragStop: (index: Int, position: Double)?
    @State private var ramp = JSON()
    @State private var previews = JSON()
    private var stops: [JSON] { control["value"]["value"].array }
    private var index: Int { max(0, min(selected, stops.count - 1)) }
    private var removable: Bool { index > 0 && index < stops.count - 1 }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func change(_ position: Double, index: Int?, color: Any = NSNull(), remove: Bool = false,
        revision: UInt64? = nil, phase: String? = nil,
        completion: (@MainActor (String?) -> Void)? = nil) {
        // A retired field keeps its original stop. Do not retarget a delayed
        // edit when selection or the stop list changes, even if an index is
        // reused. Cancellation still retires the original preview.
        if phase != "cancel" {
            guard index == nil || index == self.index,
                revision == nil || revision == fieldRevision else { completion?(nil); return }
        }
        effect(["op": "gradient_stop", "index": index as Any? ?? NSNull(),
            "position": position, "color": color, "remove": remove], phase, completion)
    }
    private func opacity(_ value: Double, index: Int, revision: UInt64, phase: String? = nil,
        completion: @escaping @MainActor (String?) -> Void) {
        var color = stops[index]["color"]["rgba"].array.map(\.number)
        guard color.count == 4 else { completion("Invalid color"); return }
        color[3] = value
        change(stops[index]["position"].number, index: index, color: stops[index]["color"].replacing("rgba", with: JSON(color)).raw, revision: revision, phase: phase, completion: completion)
    }
    private func cancelDrag() {
        if let stop = dragStop { change(0, index: stop.index, phase: "cancel") }
        dragging = false; dragStop = nil
    }
    var body: some View {
        let index = self.index
        let revision = fieldRevision
        VStack(alignment: .leading, spacing: 6) {
            GeometryReader { geometry in
                Canvas(colorMode: .extendedLinear) { context, size in
                    let width = max(1, size.width - 12)
                    let samples = ramp.array
                    let gradient = Gradient(stops: samples.enumerated().map { Gradient.Stop(color: $0.element["rgba"].paintColor, location: Double($0.offset) / Double(max(1, samples.count - 1))) })
                    context.fill(Path(roundedRect: CGRect(x: 6, y: 0, width: width, height: 32), cornerRadius: 4), with: .linearGradient(gradient,
                        startPoint: CGPoint(x: 6, y: 0), endPoint: CGPoint(x: size.width - 6, y: 0)))
                    for (i, stop) in stops.enumerated() {
                        let x = 6 + stop["position"].number * width
                        let marker = Path(ellipseIn: CGRect(x: x - 4.5, y: 38.5, width: 9, height: 9))
                        context.fill(marker, with: .color(previews[i]["rgba"].paintColor))
                        context.stroke(marker, with: .color(palette["text"]), lineWidth: 1)
                        if i == index {
                            context.stroke(Path(ellipseIn: CGRect(x: x - 7, y: 36, width: 14, height: 14)),
                                with: .color(palette["text"]), lineWidth: 2)
                        }
                    }
                }.contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 0).updating($contact) { _, active, _ in active = true }.onChanged { event in
                    let width = geometry.size.width - 12
                    guard width > 0 else { return }
                    if !dragging {
                        dragging = true
                        let nearest = stops.indices.min { abs(6 + stops[$0]["position"].number * width - event.startLocation.x) < abs(6 + stops[$1]["position"].number * width - event.startLocation.x) }
                        if let found = nearest, abs(6 + stops[found]["position"].number * width - event.startLocation.x) <= 12 {
                            selected = found
                            dragStop = (found, stops[found]["position"].number)
                            change(stops[found]["position"].number, index: found, phase: "down")
                        }
                    }
                    if let stop = dragStop, event.translation.width != 0 {
                        change(stop.position + event.translation.width / width, index: stop.index, phase: "move")
                    }
                }.onEnded { event in
                    guard dragging else { return }
                    let width = geometry.size.width - 12
                    guard width > 0 else { cancelDrag(); return }
                    if let stop = dragStop {
                        change(stop.position + event.translation.width / width, index: stop.index, phase: "up")
                    } else {
                        change((event.location.x - 6) / width, index: nil)
                    }
                    dragging = false; dragStop = nil
                }).allowsHitTesting(enabled)
                    .onChange(of: contact) { _, active in if !active { cancelDrag() } }
                    .onDisappear(perform: cancelDrag)
                    .accessibilityIdentifier("effect-gradient")
                    .accessibilityLabel("\(control["label"].string), \(stops.count) stops")
            }.frame(height: 52)
            if !stops.isEmpty {
                Group {
                    NumberControl(store: store, label: "Position", value: stops[index]["position"].number,
                        control: store.catalog["opacity"], identifier: "gradient-position",
                        gestureChange: { change($1, index: index, revision: revision, phase: $0, completion: $2) }) {
                        change($0, index: index, revision: revision, completion: $1)
                    }.disabled(!removable)
                    ManagedColorButton(label: "Color", identifier: "gradient-stop", value: stops[index]["color"],
                        documentSpace: store.state["colors"]["rgb_space"].string) {
                        change(stops[index]["position"].number, index: index, color: $0.raw, revision: revision)
                    }
                    NumberControl(store: store, label: "Opacity", value: stops[index]["color"]["rgba"][3].number,
                        control: store.catalog["opacity"], identifier: "gradient-opacity",
                        gestureChange: { opacity($1, index: index, revision: revision, phase: $0, completion: $2) }) {
                        opacity($0, index: index, revision: revision, completion: $1)
                    }
                }.id("\(index):\(revision)")
            }
            HStack {
                Button("Remove stop") { change(0, index: index, remove: true); selected = max(0, index - 1) }
                    .disabled(!removable).accessibilityIdentifier("gradient-remove")
                Spacer(minLength: 0)
                Button("Reset", action: reset)
                    .accessibilityIdentifier("gradient-reset")
            }.buttonStyle(.plain)
        }.task(id: JSON([stops.map(\.raw), store.state["colors"]["rgb_space"].raw]).stableKey) {
            ramp = ColorUI.resolve(["type": "gradient", "stops": stops.map(\.raw),
                "document_space": store.state["colors"]["rgb_space"].raw, "display_space": "DisplayP3"])
            previews = ColorUI.resolve(["type": "preview", "colors": stops.map { $0["color"].raw }, "display_space": "DisplayP3"])
            if !ramp["error"].isNull { store.failure = ramp["error"].string }
            if !previews["error"].isNull { store.failure = previews["error"].string }
        }.onChange(of: stops.map { $0["position"].number }) { previous, current in
            if current.count != previous.count { fieldRevision &+= 1 }
            // Select the stop Rust actually inserted, including history restoration.
            guard current.count == previous.count + 1,
                let inserted = current.firstIndex(where: { !previous.contains($0) }) else { return }
            selected = inserted
        }
    }
}

extension EditorStore {
    func effect(_ layer: UInt64, epoch: UInt64, key: String, action: [String: Any], phase: String? = nil,
        completion: (@MainActor (String?) -> Void)? = nil) {
        // Retired native fields must not edit their former layer or a new
        // document reusing its ID. Rust still needs a former layer's cancel
        // event to restore an interrupted preview in the same document.
        guard state["document_file"]["epoch"].uint == epoch,
            phase == "cancel" || state["layer_tools"]["editing_layer"]["id"].uint == layer else {
            completion?(nil); return
        }
        var action = action; action["layer"] = layer; action["key"] = key
        if let phase { action = ["op": "gesture", "phase": phase, "action": action] }
        let message: [String: Any] = ["type": "effect", "action": action]
        if let completion { edit(message, completion: completion) } else { dispatch(message) }
    }
}
