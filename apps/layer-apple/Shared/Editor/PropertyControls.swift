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
        VStack(alignment: .leading, spacing: store.panelMetrics.spacing) {
            Text(view["name"].string).fontWeight(.bold).lineLimit(1).help(view["title"].string)
                .accessibilityIdentifier("properties-layer-name")
            if !view["layer_type"].string.isEmpty || !view["add_filter"].isNull {
                HStack(spacing: store.panelMetrics.spacing) {
                    Text(view["layer_type"].string).font(.system(size: store.catalog["text_size_pt"].number * 4 / 3 * 0.9)).foregroundStyle(palette["text"].opacity(0.55))
                        .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading).help(view["layer_type"].string)
                        .accessibilityIdentifier("properties-layer-type")
                    if !view["add_filter"].isNull {
                        EditorMenuButton(menu: { AppleContextMenu(view["add_filter"]) { store.dispatch($0) } }, identifier: "properties-filter-menu") {
                            HStack(spacing: store.panelMetrics.spacing) {
                                SharedIcon(name: "add-filter")
                                Text(view["add_filter"]["title"].string).fontWeight(.bold)
                                SharedIcon(name: "chevron-down")
                            }.padding(.horizontal, 6).frame(height: store.panelMetrics.height)
                        }.fixedSize(horizontal: true, vertical: false)
                            .buttonStyle(EditorControlButtonStyle()).accessibilityIdentifier("properties-add-filter")
                    }
                }
            }
            VStack(alignment: .leading, spacing: store.panelMetrics.spacing) {
                if !view["resource_label"].isNull { lookup(actions, palette: palette) }
                if pages.count > 1 || !tools.isEmpty {
                    HStack(spacing: 4) {
                        if pages.count > 1 {
                            EditorChoice(label: view["title"].string, options: pages.map { $0["label"].string },
                                selected: pages.firstIndex { $0["id"].string == view["page"].string } ?? 0, identifier: "properties-page",
                                background: palette.controlFill, height: store.panelMetrics.height) {
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
                        if !control["section"].isNull { Text(control["section"].string).fontWeight(.bold).padding(.top, index > 0 ? 2 : 0) }
                    }
                    Group {
                        if control["kind"]["kind"].string == "curve" {
                            CurveProperty(store: store, layer: layer, control: control)
                                .id("\(epoch):\(layer):\(control["key"].string):\(control["curve"]["domain"].stableKey)")
                        } else {
                            PropertyField(store: store, layer: layer, epoch: epoch, control: control, labelWidth: store.panelLabelWidth(controls.filter { $0["kind"]["kind"].string == "choice" }.map { $0["label"].string }))
                                .id("\(epoch):\(layer):\(control["key"].string):\(control["kind"]["kind"].string)")
                        }
                    }.disabled(control["disabled"].bool).opacity(control["disabled"].bool && view["enabled"].bool ? 0.4 : 1)
                }
            }.disabled(!view["enabled"].bool).opacity(view["enabled"].bool ? 1 : 0.4)
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
            if first["icon"].isNull { Text(label).lineLimit(1).padding(.horizontal, 6).frame(height: store.panelMetrics.height) }
            else { SharedIcon(name: first["icon"].string).frame(width: store.panelMetrics.height, height: store.panelMetrics.height) }
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
        return HStack(spacing: store.panelMetrics.spacing) {
            EditorChoice(label: view["resource_label"].string, options: labels, selected: selected ?? presets.count,
                identifier: "property-resource-choice", background: palette.controlFill, ellipsis: true, height: store.panelMetrics.height) { index in
                if presets.indices.contains(index) { effect(presets[index]["action"]) }
            }.help(view["resource_name"].string)
            if let importer {
                IconTile(icon: importer["icon"].string, label: importer["label"].string) { effect(importer["action"]) }
                    .frame(width: store.panelMetrics.height, height: store.panelMetrics.height).accessibilityIdentifier("import-lookup")
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
    var labelWidth: CGFloat = 0
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
                gestureChange: { change($1, revision: revision, phase: $0, completion: $2) }, reset: reset) {
                change($0, revision: revision, completion: $1)
            }.id(control["kind"].stableKey + label)
        case "toggle":
            Toggle(label, isOn: Binding(get: { value.bool }, set: { change($0, revision: revision) }))
                .toggleStyle(PanelCheckStyle(minHeight: store.panelMetrics.height)).accessibilityIdentifier("property-" + key)
        case "choice" where !control["paper"].isNull:
            PropertyChoiceRow(labelWidth: labelWidth) {
                Text(label).lineLimit(1)
                PaperTextureButton(store: store, layer: layer, control: control)
            }
        case "choice":
            PropertyChoiceRow(labelWidth: labelWidth) {
                Text(label).lineLimit(1)
                EditorChoice(label: label, options: control["kind"]["options"].array.map(\.string),
                    selected: Int(value.uint), identifier: "property-" + key,
                    background: EditorPalette(source: store.state["palette"])["input"], height: store.panelMetrics.height) { change($0, revision: revision) }
            }
        case "color":
            HStack(spacing: store.panelMetrics.spacing) {
                ManagedColorButton(label: label, identifier: "property-" + key, value: value,
                    colors: store.displayColors, viewing: store.colorViewing, opaque: control["kind"]["opaque"].bool) { change($0.raw, revision: revision) }
                if !control["color_action"].isNull {
                    IconTile(icon: "fill", label: nativeCopy["color"]["use_selected"].string) { store.dispatch(control["color_action"]) }
                        .frame(width: store.panelMetrics.height, height: store.panelMetrics.height).accessibilityIdentifier("property-\(key)-bucket")
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
    var body: some View {
        CurveEditor(store: store, control: JSON(["controls": control["curve"].raw, "points": control["value"]["value"].raw,
            "plot": control["plot"].raw, "marker": NSNull(), "modified": control["modified"].bool]),
            target: JSON(["kind": "effect", "layer": layer, "key": control["key"].string]), label: control["label"].string, histogram: true)
    }
}

struct CurveEditor: View {
    @ObservedObject var store: EditorStore
    let control: JSON
    let target: JSON
    let label: String
    var histogram = false
    var axisGutter: String?
    @Environment(\.isEnabled) private var enabled
    @FocusState private var focused: Bool
    @State private var contact: [String: Any]?
    @State private var held: (key: String, owner: [String: Any])?
    @State private var sequence: (time: Date, points: Int)?
    @State private var numberOwner: [String: Any]?
    @State private var plotHeight: CGFloat = 200
    private var key: String { target["key"].string }
    private var curve: JSON { control["controls"] }
    private var points: [JSON] { control["points"].array }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var owner: [String: Any] { ["epoch": curve["epoch"].uint] }
    private func send(_ owner: [String: Any], _ fields: [String: Any]) {
        store.dispatch(["type": "curve_editor", "target": target.raw, "action": owner.merging(fields) { $1 }])
    }
    private func send(_ owner: [String: Any], contact phase: String, at point: CGPoint = .zero, in size: CGSize = CGSize(width: 1, height: 1)) {
        let inset = curve["inset"].number
        send(owner, ["kind": "contact", "phase": phase, "point": [point.x - inset, point.y - inset], "extent": [max(1, size.width - 2 * inset), max(1, size.height - 2 * inset)]])
    }
    private func cancel() {
        let captured = contact ?? held?.owner
        contact = nil; held = nil
        if let captured { send(captured, contact: "cancel") }
    }
    private func number(_ axis: String, phase: String?, value: Double) {
        let request = (numberOwner ?? owner).merging(["kind": "number", "axis": axis, "operation": ["type": "value", "value": value]]) { $1 }
        guard let phase else { store.dispatch(["type": "curve_editor", "target": target.raw, "action": request]); return }
        if phase == "down" { numberOwner = owner }
        store.dispatch(["type": "curve_editor", "target": target.raw, "action": ["kind": "gesture", "phase": phase, "action": request]])
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
        guard !NativeTextContext.composing else { return .ignored }
        let pressed = press.phase != .up
        guard let name = keyName(press.key) else {
            guard press.key.character != "\t" else { return .ignored }
            store.input(["type": "key", "key": String(press.key.character), "pressed": pressed, "repeat": press.phase == .repeat,
                "modifiers": ["command": !press.modifiers.isDisjoint(with: [.command, .control]),
                    "shift": press.modifiers.contains(.shift), "alt": press.modifiers.contains(.option)]])
            return .handled
        }
        if pressed && !press.modifiers.isDisjoint(with: [.command, .option, .control]) { return .ignored }
        let target = held?.key == name ? held!.owner : owner
        if pressed { held = (name, target) }
        send(target, ["kind": "key", "key_event": name, "pressed": pressed, "repeat": press.phase == .repeat,
            "modifiers": ["command": press.modifiers.contains(.command), "shift": press.modifiers.contains(.shift), "alt": press.modifiers.contains(.option)]])
        if (!pressed && held?.key == name) || name == "Escape" { held = nil }
        if name == "Escape" { contact = nil }
        return .handled
    }
    private func axisLabels(_ axis: JSON, reversed: Bool) -> [String] {
        let labels = [axis["minimum"].string, axis["label"].string, axis["maximum"].string]
        return reversed ? labels.reversed() : labels
    }
    @ViewBuilder private func verticalAxis(_ axis: JSON) -> some View {
        let labels = VStack {
            ForEach(Array(axisLabels(axis, reversed: true).enumerated()), id: \.offset) { index, text in
                if index > 0 { Spacer(minLength: 0) }
                Text(text).fixedSize().rotationEffect(.degrees(index == 1 && !curve["coordinate_readouts"].bool ? -90 : 0))
                    .frame(width: curve["coordinate_readouts"].bool || axisGutter != nil ? nil : 36)
            }
        }
        if let axisGutter { Text(axisGutter).hidden().frame(height: plotHeight).overlay { labels }.font(.caption).foregroundStyle(palette["text"].opacity(0.7)) }
        else { labels.font(.caption).foregroundStyle(palette["text"].opacity(0.7)).frame(height: 200) }
    }
    var body: some View {
        let axes = curve["axes"]
        VStack(alignment: .leading, spacing: store.panelMetrics.spacing) {
            HStack(alignment: axisGutter == nil ? .center : .top, spacing: 4) {
                verticalAxis(axes[1])
                VStack(spacing: 2) {
                    plot
                    ZStack {
                        HStack { Text(axes[0]["minimum"].string); Spacer(); Text(axes[0]["maximum"].string) }
                        Text(axes[0]["label"].string)
                    }.font(.caption).foregroundStyle(palette["text"].opacity(0.7))
                }
                if let axisGutter { Text(axisGutter).font(.caption).hidden().accessibilityHidden(true) }
            }
            if curve["coordinate_readouts"].bool { HStack(alignment: .top, spacing: store.panelMetrics.spacing) {
                ForEach(["input", "output"].indices, id: \.self) { index in
                    let axis = index == 0 ? "input" : "output", coordinate = curve[axis]
                    VStack(alignment: .leading, spacing: store.panelMetrics.spacing) {
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
            }
            if histogram { ScopeFooter(store: store, kind: "tonal_histogram") }
        }.help(curve["help"].string)
            .onChange(of: focused) { _, now in if !now { cancel() } }
            .onDisappear(perform: cancel)
            .modifier(BubbleNativeControl(cancel: cancel))
    }
    @ViewBuilder private var plot: some View {
        if axisGutter != nil {
            plotContent.aspectRatio(1, contentMode: .fit)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { plotHeight = $0 }
        } else { plotContent.frame(height: 200) }
    }
    private var plotContent: some View {
        GeometryReader { geometry in
            Canvas(colorMode: .extendedLinear) { context, size in
                let inset = curve["inset"].number
                context.translateBy(x: inset, y: inset)
                let size = CGSize(width: max(1, size.width - 2 * inset), height: max(1, size.height - 2 * inset))
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
                if curve["control_polygon"].bool {
                    var polygon = Path()
                    for (index, p) in points.enumerated() {
                        let point = CGPoint(x: p[0].number * size.width, y: (1 - p[1].number) * size.height)
                        if index == 0 { polygon.move(to: point) } else { polygon.addLine(to: point) }
                    }
                    context.stroke(polygon, with: .color(palette["text"].opacity(0.35)), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                }
                if !control["marker"].isNull {
                    let p = control["marker"]
                    context.fill(Path(ellipseIn: CGRect(x: p[0].number * size.width - 4, y: (1 - p[1].number) * size.height - 4, width: 8, height: 8)), with: .color(palette["accent"]))
                }
                let selected = curve["selected"].isNull ? nil : Int(curve["selected"].uint)
                for (index, p) in points.enumerated() {
                    let radius: CGFloat = selected == index ? 5 : 3.5
                    let dot = Path(ellipseIn: CGRect(x: p[0].number * size.width - radius, y: (1 - p[1].number) * size.height - radius,
                        width: radius * 2, height: radius * 2))
                    if selected == index { context.stroke(dot, with: .color(palette["text"]), lineWidth: 1.5) }
                    else { context.fill(dot, with: .color(palette["text"])) }
                }
            }.background { ZStack { palette["text"].opacity(0.12); if histogram { ScopeGraph(store: store, scopes: store.scopes, kind: "tonal_histogram") } } }
                .contentShape(Rectangle())
                .overlay {
                    ControlInput(identity: target.stableKey + String(curve["epoch"].uint), contact: { event in
                        switch event.phase {
                        case "down":
                            focused = true; cancel()
                            sequence = (Date(), points.count); contact = owner
                            send(owner, contact: "down", at: event.point, in: geometry.size)
                        case "move":
                            if let contact { send(contact, contact: "move", at: event.point, in: geometry.size) }
                        case "up":
                            if let contact { send(contact, contact: "up", at: event.point, in: geometry.size) }
                            contact = nil
                        case "cancel": cancel()
                        default: break
                        }
                    }, secondary: { point, size in
                        cancel(); focused = true
                        let inset = curve["inset"].number
                        send(owner, ["kind": "remove_at", "point": [point.x - inset, point.y - inset],
                            "extent": [size.width - 2 * inset, size.height - 2 * inset]])
                    }, doublePress: { point, size in
                        cancel(); focused = true
                        let inset = curve["inset"].number
                        send(owner, ["kind": "remove_at", "point": [point.x - inset, point.y - inset],
                            "extent": [size.width - 2 * inset, size.height - 2 * inset], "point_count": sequence?.points ?? points.count])
                    }).accessibilityHidden(true)
                }
                .allowsHitTesting(enabled)
                .focusable(enabled).focused($focused).focusEffectDisabled()
                .onKeyPress(phases: [.down, .repeat, .up], action: press)
                .accessibilityRepresentation {
                    Button(label) { focused = true }.accessibilityValue(String(points.count))
                        .accessibilityIdentifier("effect-curve")
                }
        }.overlay(alignment: .bottomTrailing) {
                if histogram && control["modified"].bool {
                    Button { store.dispatch(["type": "curve_editor", "target": target.raw, "action": ["kind": "reset"]]) } label: {
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
