import SwiftUI

/// Schema, values, validation and sampled curves are all supplied by Rust.
/// These views only present controls and translate native editing gestures.
struct LayerPropertiesPanel: View {
    @ObservedObject var store: EditorStore
    private var view: JSON { store.state["layer_properties"] }
    private var controls: [JSON] { view["controls"].array }
    var body: some View {
        let epoch = store.state["document_file"]["epoch"].uint, layer = view["layer"].uint, pages = view["pages"].array
        let actions = view["actions"].array, palette = EditorPalette(source: store.state["palette"])
        let tools = actions.filter { !["lookup_preset", "import_lookup"].contains($0["action"]["op"].string) }
        VStack(alignment: .leading, spacing: 6) {
            VStack(alignment: .leading, spacing: 6) {
                Text(view["title"].string).fontWeight(.bold).help(view["description"].string)
                if !view["resource_label"].isNull { lookup(actions, palette: palette) }
                if pages.count > 1 || !tools.isEmpty {
                    HStack(spacing: 4) {
                        if pages.count > 1 {
                            EditorChoice(label: view["title"].string, options: pages.map { $0["label"].string },
                                selected: pages.firstIndex { $0["id"].string == view["page"].string } ?? 0, identifier: "properties-page",
                                background: palette["input"]) {
                                store.dispatch(["type": "effect", "action": ["op": "select_page", "layer": layer, "page": pages[$0]["id"].string]])
                            }
                        }
                        ForEach(groups(tools), id: \.id) { group in action(group.id, group.items, palette: palette) }
                    }
                }
                if view["histogram"].bool { ScopeControl(store: store, kind: "tonal_histogram", tonal: true) }
                ForEach(controls.indices, id: \.self) { index in
                    let control = controls[index]
                    if index == 0 || control["section_id"].stableKey != controls[index - 1]["section_id"].stableKey {
                        if index > 0 { Divider().padding(.vertical, 3) }
                        if !control["section"].isNull { Text(control["section"].string).fontWeight(.bold).padding(.leading, 6) }
                    }
                    if control["kind"]["kind"].string == "curve" {
                        CurveProperty(store: store, layer: layer, control: control)
                            .id("\(epoch):\(layer):\(control["key"].string):\(control["curve"]["domain"].stableKey)")
                    } else {
                        PropertyField(store: store, layer: layer, epoch: epoch, control: control)
                            .id("\(epoch):\(layer):\(control["key"].string):\(control["kind"]["kind"].string)")
                    }
                }
            }.disabled(!view["enabled"].bool).opacity(view["enabled"].bool ? 1 : 0.4)
            if !view["add_filter"].isNull {
                EditorMenuButton(menu: { AppleContextMenu(view["add_filter"]) { store.dispatch($0) } }, identifier: "properties-filter-menu") {
                    HStack(spacing: 6) {
                        Text(view["add_filter"]["title"].string)
                        SharedIcon(name: "chevron-down").frame(width: 12, height: 12)
                    }.padding(.horizontal, 6).frame(height: 32)
                }.buttonStyle(EditorControlButtonStyle()).accessibilityIdentifier("properties-add-filter")
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("layer-properties")
    }
    private func effect(_ action: JSON) { store.dispatch(["type": "effect", "action": action.raw]) }
    private func groups(_ actions: [JSON]) -> [(id: String, items: [JSON])] {
        actions.reduce(into: []) { groups, action in
            let id = action["group"].isNull ? action["action"]["op"].string : action["group"]["id"].string
            if let index = groups.firstIndex(where: { $0.id == id }) { groups[index].items.append(action) }
            else { groups.append((id, [action])) }
        }
    }
    @ViewBuilder private func action(_ id: String, _ choices: [JSON], palette: EditorPalette) -> some View {
        let first = choices[0], group = first["group"], label = group.isNull ? first["label"].string : group["label"].string
        let face = Group {
            if first["icon"].isNull { Text(label).lineLimit(1).padding(.horizontal, 6).frame(height: 32) }
            else { SharedIcon(name: first["icon"].string).frame(width: 32, height: 32) }
        }.contentShape(Rectangle()).help(label).accessibilityLabel(label)
        if group.isNull {
            Button { effect(first["action"]) } label: { face }.buttonStyle(EditorControlButtonStyle())
                .accessibilityIdentifier("property-action-" + id)
        } else {
            EditorMenuButton(menu: {
                AppleContextMenu(JSON(["sections": [choices.map { ["label": $0["label"].raw, "enabled": true, "action": $0["action"].raw,
                    "identifier": "property-\($0["action"]["op"].string)-\($0["action"]["role"].string)"] }]])) { effect($0) }
            }, identifier: "property-action-menu-" + id) { face }.buttonStyle(EditorControlButtonStyle())
                .accessibilityIdentifier("property-action-group-" + id)
        }
    }
    private func lookup(_ actions: [JSON], palette: EditorPalette) -> some View {
        let indices = actions.indices.filter { actions[$0]["action"]["op"].string == "lookup_preset" }, presets = indices.map { actions[$0] }
        let selected = view["resource_selection"].isNull ? nil : indices.firstIndex(of: Int(view["resource_selection"].uint))
        let labels = presets.map { $0["label"].string } + (selected == nil ? [view["resource_name"].string] : [])
        let importer = actions.first { $0["action"]["op"].string == "import_lookup" }
        return HStack(spacing: 6) {
            EditorChoice(label: view["resource_label"].string, options: labels, selected: selected ?? presets.count,
                identifier: "property-resource-choice", background: palette["input"]) { index in
                if presets.indices.contains(index) { effect(presets[index]["action"]) }
            }.help(view["resource_name"].string)
            if let importer {
                IconTile(icon: importer["icon"].string, label: importer["label"].string) { effect(importer["action"]) }
                    .frame(width: 40, height: 36).accessibilityIdentifier("import-lookup")
            }
        }
    }
}

private struct PropertyField: View {
    @Environment(\.capyNativeCopy) private var nativeCopy
    @Environment(\.capyCommonCopy) private var commonCopy
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
        effect(kind == "number" ? ["op": "number", "operation": ["type": "value", "value": value]] : ["op": "set", "value": ["kind": kind, "value": value]],
            revision: revision, phase: phase, completion: completion ?? { if let error = $0 { store.failure = error } })
    }
    private func reset() {
        revision &+= 1
        store.effect(layer, epoch: epoch, key: key, action: ["op": "reset"])
    }
    var body: some View {
        field(revision: revision).id(revision).contextMenu { Button(commonCopy["reset"].string, action: reset) }
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
            HStack(spacing: 6) {
                ManagedColorButton(label: label, identifier: "property-" + key, value: value,
                    colors: store.displayColors, viewing: store.colorViewing, opaque: control["kind"]["opaque"].bool) { change($0.raw, revision: revision) }
                if !control["color_action"].isNull {
                    IconTile(icon: "fill", label: nativeCopy["color"]["use_selected"].string) { store.dispatch(control["color_action"]) }
                        .frame(width: 40, height: 36).accessibilityIdentifier("property-\(key)-bucket")
                }
            }
        case "gradient":
            GradientEditor(store: store, control: control) { action, completion in
                store.edit(action as? [String: Any] ?? [:], completion: completion)
            }
        default: EmptyView()
        }
    }
}

private struct CurveProperty: View {
    @ObservedObject var store: EditorStore
    let layer: UInt64
    let control: JSON
    @Environment(\.isEnabled) private var enabled
    @GestureState private var touching = false
    @FocusState private var focused: Bool
    @State private var contact: [String: Any]?
    @State private var held: (key: String, owner: [String: Any])?
    @State private var sequence: (time: Date, points: Int)?
    @State private var removal: [String: Any]?
    @State private var numberOwner: [String: Any]?
    private var key: String { control["key"].string }
    private var curve: JSON { control["curve"] }
    private var points: [JSON] { control["value"]["value"].array }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var owner: [String: Any] { ["layer": layer, "key": key, "epoch": curve["epoch"].uint] }
    private func send(_ owner: [String: Any], _ fields: [String: Any]) {
        store.dispatch(["type": "effect", "action": owner.merging(fields) { $1 }])
    }
    private func send(_ owner: [String: Any], contact phase: String, at point: CGPoint = .zero, in size: CGSize = CGSize(width: 1, height: 1)) {
        send(owner, ["op": "curve_contact", "phase": phase, "point": [point.x, point.y], "extent": [size.width, size.height]])
    }
    private func cancel() {
        let captured = contact ?? held?.owner
        contact = nil; held = nil
        if let captured { send(captured, contact: "cancel") }
    }
    private func number(_ axis: String, phase: String?, value: Double) {
        let request = (numberOwner ?? owner).merging(["op": "curve_number", "axis": axis, "operation": ["type": "value", "value": value]]) { $1 }
        guard let phase else { store.dispatch(["type": "effect", "action": request]); return }
        if phase == "down" { numberOwner = owner }
        store.dispatch(["type": "effect", "action": ["op": "gesture", "phase": phase, "action": request]])
        if phase != "down" { numberOwner = nil }
    }
    private func keyName(_ key: KeyEquivalent) -> String? {
        switch key {
        case .leftArrow: "ArrowLeft"
        case .rightArrow: "ArrowRight"
        case .upArrow: "ArrowUp"
        case .downArrow: "ArrowDown"
        case .delete: "Backspace"
        case .deleteForward: "Delete"
        case .escape: "Escape"
        default: key.character == "\u{7F}" ? "Backspace" : nil
        }
    }
    private func press(_ press: KeyPress) -> KeyPress.Result {
        guard let name = keyName(press.key), !NativeTextContext.composing else { return .ignored }
        let pressed = press.phase != .up
        if pressed && !press.modifiers.isDisjoint(with: [.command, .option, .control]) { return .ignored }
        let target = held?.key == name ? held!.owner : owner
        if pressed { held = (name, target) }
        send(target, ["op": "curve_key", "key_event": name, "pressed": pressed, "repeat": press.phase == .repeat,
            "modifiers": ["command": press.modifiers.contains(.command), "shift": press.modifiers.contains(.shift), "alt": press.modifiers.contains(.option)]])
        if (!pressed && held?.key == name) || name == "Escape" { held = nil }
        if name == "Escape" { contact = nil }
        return .handled
    }
    private func axisLabels(_ axis: JSON, reversed: Bool) -> [String] {
        let labels = [axis["minimum"].string, axis["label"].string, axis["maximum"].string]
        return reversed ? labels.reversed() : labels
    }
    var body: some View {
        let axes = curve["axes"].array
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 4) {
                VStack {
                    ForEach(Array(axisLabels(axes[1], reversed: true).enumerated()), id: \.offset) { index, text in
                        if index > 0 { Spacer(minLength: 0) }
                        Text(text)
                    }
                }.font(.caption).foregroundStyle(palette["text"].opacity(0.7)).frame(height: 200)
                VStack(spacing: 2) {
                    plot
                    HStack {
                        ForEach(Array(axisLabels(axes[0], reversed: false).enumerated()), id: \.offset) { index, text in
                            if index > 0 { Spacer(minLength: 0) }
                            Text(text)
                        }
                    }.font(.caption).foregroundStyle(palette["text"].opacity(0.7))
                }
            }
            HStack(alignment: .top, spacing: 6) {
                ForEach(["input", "output"].indices, id: \.self) { index in
                    let axis = index == 0 ? "input" : "output", coordinate = curve[axis]
                    VStack(alignment: .leading, spacing: 2) {
                        Text(axes[index]["label"].string).lineLimit(1)
                        NumberControl(store: store, label: axes[index]["label"].string, value: coordinate["value"].number,
                            control: curve["numeric"], identifier: "curve-" + axis, presentedText: coordinate.isNull ? "" : coordinate["text"].string,
                            valueOnly: true,
                            gestureChange: { phase, value, completion in number(axis, phase: phase, value: value); completion(nil) }) { value, completion in
                            number(axis, phase: nil, value: value); completion(nil)
                        }.disabled(coordinate.isNull || coordinate["read_only"].bool)
                        if curve["domain"]["kind"].string == "log_hdr" {
                            Text(coordinate["ev"].string).font(.caption).monospacedDigit().foregroundStyle(palette["text"].opacity(0.7))
                                .frame(maxWidth: .infinity, alignment: .trailing)
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            ScopeFooter(store: store, kind: "tonal_histogram")
        }.help(curve["help"].string)
            .onChange(of: focused) { _, now in if !now { cancel() } }
            .onDisappear(perform: cancel)
    }
    private var plot: some View {
        GeometryReader { geometry in
            Canvas(colorMode: .extendedLinear) { context, size in
                var grid = Path()
                for i in 1...3 {
                    let fraction = CGFloat(i) / 4
                    grid.move(to: CGPoint(x: size.width * fraction, y: 0)); grid.addLine(to: CGPoint(x: size.width * fraction, y: size.height))
                    grid.move(to: CGPoint(x: 0, y: size.height * fraction)); grid.addLine(to: CGPoint(x: size.width, y: size.height * fraction))
                }
                context.stroke(grid, with: .color(palette["text"].opacity(0.2)), lineWidth: 1)
                let whiteX = curve["axes"][0]["white"], whiteY = curve["axes"][1]["white"]
                if !whiteX.isNull {
                    var reference = Path()
                    reference.move(to: CGPoint(x: whiteX.number * size.width, y: 0))
                    reference.addLine(to: CGPoint(x: whiteX.number * size.width, y: size.height))
                    reference.move(to: CGPoint(x: 0, y: (1 - whiteY.number) * size.height))
                    reference.addLine(to: CGPoint(x: size.width, y: (1 - whiteY.number) * size.height))
                    context.stroke(reference, with: .color(palette["text"].opacity(0.7)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                }
                var line = Path()
                for (index, p) in control["plot"].array.enumerated() {
                    let point = CGPoint(x: p[0].number * size.width, y: (1 - p[1].number) * size.height)
                    if index == 0 { line.move(to: point) } else { line.addLine(to: point) }
                }
                context.stroke(line, with: .color(palette["text"]), lineWidth: 1.5)
                let selected = curve["selected"].isNull ? nil : Int(curve["selected"].uint)
                for (index, p) in points.enumerated() {
                    let radius: CGFloat = selected == index ? 5 : 3.5
                    let dot = Path(ellipseIn: CGRect(x: p[0].number * size.width - radius, y: (1 - p[1].number) * size.height - radius,
                        width: radius * 2, height: radius * 2))
                    if selected == index { context.stroke(dot, with: .color(palette["text"]), lineWidth: 1.5) }
                    else { context.fill(dot, with: .color(palette["text"])) }
                }
            }.background { ZStack { palette["text"].opacity(0.12); ScopeGraph(store: store, scopes: store.scopes, kind: "tonal_histogram") } }
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).updating($touching) { _, active, _ in active = true }.onChanged { event in
                    let size = geometry.size
                    guard size.width > 0, size.height > 0 else { return }
                    if let captured = contact { send(captured, contact: "move", at: event.location, in: size); return }
                    focused = true
                    cancel()
                    let now = Date(), captured = owner
                    if let last = sequence, now.timeIntervalSince(last.time) < 0.5 { sequence = (now, last.points) }
                    else { sequence = (now, points.count) }
                    contact = captured
                    send(captured, contact: "down", at: event.startLocation, in: size)
                    if event.location != event.startLocation { send(captured, contact: "move", at: event.location, in: size) }
                }.onEnded { event in
                    if let captured = contact { send(captured, contact: "up", at: event.location, in: geometry.size) }
                    contact = nil
                    if let removal { send(owner, removal); self.removal = nil }
                })
                .simultaneousGesture(SpatialTapGesture(count: 2).onEnded { tap in
                    let request: [String: Any] = ["op": "curve_remove_at", "point": [tap.location.x, tap.location.y],
                        "extent": [geometry.size.width, geometry.size.height], "point_count": sequence?.points ?? points.count]
                    if contact == nil { send(owner, request) } else { removal = request }
                })
                .allowsHitTesting(enabled)
                .focusable(enabled).focused($focused).focusEffectDisabled()
                .onKeyPress(phases: [.down, .repeat, .up], action: press)
                .onChange(of: touching) { _, active in if !active, contact != nil { cancel() } }
                .accessibilityLabel("\(control["label"].string), \(points.count) points")
                .accessibilityIdentifier("effect-curve")
        }.frame(height: 200)
            .overlay(alignment: .bottomTrailing) {
                if control["modified"].bool {
                    Button { store.effect(layer, epoch: store.state["document_file"]["epoch"].uint, key: key, action: ["op": "reset"]) } label: {
                        SharedIcon(name: "reset").frame(width: 28, height: 28).contentShape(Rectangle())
                    }.buttonStyle(.plain).foregroundColor(palette["text"].opacity(0.7)).padding(2)
                        .help(curve["reset_label"].string).accessibilityLabel(curve["reset_label"].string).accessibilityIdentifier("curve-reset")
                }
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
