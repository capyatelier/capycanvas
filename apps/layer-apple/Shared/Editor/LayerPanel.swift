import SwiftUI
import UniformTypeIdentifiers

struct LayerPanel: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    @State private var menu = JSON()
    @State private var importing = false
    @State private var choosingBlend = false
    @State private var bounds: [UInt64: CGRect] = [:]
    @State private var drag: LayerDrag?
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var view: JSON { store.state["layer_tools"] }
    private var current: JSON { view["editing_layer"] }
    private var layers: [JSON] { store.state["layers"].array }
    private func visible(_ control: String) -> Bool {
        panel["controls"].array.contains { $0["control"].string == control && $0["visible_in_panel"].bool }
    }
    var body: some View {
        ZStack(alignment: .topLeading) {
            VStack(spacing: 0) {
                if visible("layer_opacity") { header.modifier(PanelBodyMeasurement(panel: "layers", part: "header")) }
                if visible("layers") {
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(layers, id: \.id) { layer in
                                LayerRow(store: store, layer: layer, previews: store.layerThumbnails,
                                    context: { mask in openMenu(layer, mask: mask) },
                                    dragging: { event in updateDrag(layer, event) }, drop: finishDrag, cancelDrag: { drag = nil })
                                    .background(GeometryReader { proxy in
                                        Color.clear.preference(key: LayerBounds.self,
                                            value: [layer["id"].uint: proxy.frame(in: .named("layer-panel"))])
                                    })
                                    .overlay { dropMark(layer) }
                                    .onAppear { store.layerThumbnails.show(layer["id"].uint) }
                                    .onDisappear { store.layerThumbnails.hide(layer["id"].uint) }
                            }
                        }.modifier(PanelBodyMeasurement(panel: "layers", part: "rows"))
                    }.accessibilityIdentifier("layer-rows")
                } else { Spacer(minLength: 0) }
                if visible("layer_actions") { footer.modifier(PanelBodyMeasurement(panel: "layers", part: "footer")) }
            }
            if let drag, let layer = layers.first(where: { $0["id"].uint == drag.id }) {
                LayerRow(store: store, layer: layer, previews: store.layerThumbnails, preview: true)
                    .background(palette["panel"]).opacity(0.7).allowsHitTesting(false)
                    .accessibilityHidden(true)
                    .offset(y: drag.top + drag.translation)
            }
        }.coordinateSpace(name: "layer-panel")
            .onPreferenceChange(LayerBounds.self) { bounds = $0 }
            .onChange(of: store.state["revision"].uint) { _, _ in store.layerThumbnails.refresh() }
            .onDisappear { drag = nil }
            .popover(isPresented: Binding(get: { !menu.isNull }, set: { if !$0 { menu = JSON() } })) {
                LayerActionMenu(store: store, menu: menu) { menu = JSON() }
            }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.image]) { result in
                switch result {
                case .success(let url): store.importLayer(url)
                case .failure(let error): store.failure = error.localizedDescription
                }
            }
    }
    private var header: some View {
        VStack(spacing: 2) {
            HStack(spacing: 6) {
                Button { choosingBlend = true } label: {
                    HStack(spacing: 2) {
                        Text(current["blend_label"].string).fontWeight(.bold).lineLimit(1)
                        Spacer(minLength: 0)
                        SharedIcon(name: "chevron-down", size: 12)
                    }.padding(.horizontal, 6).frame(height: 24).background(palette["input"], in: RoundedRectangle(cornerRadius: 4))
                }.buttonStyle(.plain)
                    .disabled(!view["controls"]["blend"].bool).accessibilityLabel("Layer blend mode")
                    .accessibilityValue(current["blend_label"].string)
                    .frame(maxWidth: .infinity)
                    .popover(isPresented: $choosingBlend) {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 0) {
                                ForEach(store.catalog["layer_blends"].array.indices, id: \.self) { index in
                                    Button {
                                        choosingBlend = false
                                        store.layer(["op": "blend", "id": current["id"].raw, "value": index])
                                    } label: {
                                        HStack {
                                            Image(systemName: "checkmark").opacity(current["blend"].uint == UInt64(index) ? 1 : 0)
                                            Text(store.catalog["layer_blends"][index].string)
                                            Spacer(minLength: 0)
                                        }.padding(.horizontal, 8).frame(height: 28).contentShape(Rectangle())
                                    }.buttonStyle(.plain)
                                }
                            }.padding(6)
                        }.frame(width: 230, height: 400)
                    }
                LayerOpacityField(store: store).disabled(!view["controls"]["opacity"].bool).frame(maxWidth: .infinity)
            }
            HStack(spacing: 2) {
                flag("alpha-lock", "Alpha lock", "alpha_locked", "alpha_lock", "alpha_lock")
                flag("lock", "Lock editing", "locked", "lock", "edit_lock")
                flag("clip", "Clip to layer below", "clipped", "clip", "clip")
                LayerButton(icon: "reference", label: view["reference_action_label"].string,
                    enabled: view["can_reference"].bool, selected: view["references_selected"].bool) {
                    store.layer(["op": "reference_selection"])
                }
                Spacer(minLength: 0)
            }
        }.padding(.horizontal, 6).padding(.vertical, 4)
    }
    private func flag(_ icon: String, _ label: String, _ property: String, _ op: String, _ capability: String) -> some View {
        LayerButton(icon: icon, label: label, enabled: view["controls"][capability].bool, selected: current[property].bool) {
            store.layer(["op": op, "id": current["id"].raw, "value": !current[property].bool])
        }
    }
    private var footer: some View {
        HStack(spacing: 2) {
            LayerButton(icon: "plus", label: "New layer") { store.layer(["op": "new", "group": false, "clipped": false]) }
            LayerButton(icon: "folder", label: "New group") { store.layer(["op": "new", "group": true, "clipped": false]) }
            LayerButton(icon: "mask", label: "Add layer mask", enabled: view["controls"]["mask"].bool) {
                store.layer(["op": "add_mask", "id": current["id"].raw, "replace": false])
            }
            LayerButton(icon: "image", label: "Import image as layer", enabled: store.snapshot["canvas_ready"].bool) { importing = true }
            LayerButton(icon: "delete", label: "Delete selected layers", enabled: view["can_delete"].bool) { store.layer(["op": "delete_selected"]) }
            Spacer(minLength: 0)
            LayerButton(icon: "more", label: "Layer actions", enabled: !current.isNull) { openMenu(current, mask: current["mask_selected"].bool) }
        }.padding(.horizontal, 6).padding(.vertical, 4)
    }
    private func openMenu(_ layer: JSON, mask: Bool) {
        drag = nil
        store.layer(["op": "context", "id": layer["id"].raw, "mask": mask])
        store.query(["type": "layer_menu", "id": layer["id"].raw, "mask": mask]) { menu = $0 }
    }
    private func updateDrag(_ layer: JSON, _ event: DragGesture.Value) {
        let id = layer["id"].uint
        guard let origin = bounds[id] else { return }
        let target = layers.first { $0["id"].uint != id && bounds[$0["id"].uint]?.contains(event.location) == true }
        var fraction = 0.0
        if let target, target["can_drop_below"].bool, let rect = bounds[target["id"].uint], rect.height > 0 {
            fraction = (event.location.y - rect.minY) / rect.height
        }
        drag = LayerDrag(id: id, top: drag?.top ?? origin.minY, translation: event.translation.height,
            target: target?["id"].uint, fraction: fraction)
    }
    private func finishDrag() {
        defer { drag = nil }
        if let drag, let target = drag.target {
            store.layer(["op": "drop", "id": drag.id, "target": target, "fraction": drag.fraction])
        }
    }
    @ViewBuilder private func dropMark(_ layer: JSON) -> some View {
        if let drag, drag.target == layer["id"].uint {
            if layer["group"].bool && drag.fraction > 0.25 && drag.fraction < 0.75 {
                Rectangle().stroke(EditorPalette.sharedAccent, lineWidth: 2).allowsHitTesting(false)
            } else {
                VStack(spacing: 0) {
                    if drag.fraction >= 0.5 { Spacer(minLength: 0) }
                    Rectangle().fill(EditorPalette.sharedAccent).frame(height: 2)
                    if drag.fraction < 0.5 { Spacer(minLength: 0) }
                }.allowsHitTesting(false)
            }
        }
    }
}

private struct LayerDrag { let id: UInt64; let top: CGFloat; let translation: CGFloat; let target: UInt64?; let fraction: Double }
private struct LayerBounds: PreferenceKey {
    static var defaultValue: [UInt64: CGRect] = [:]
    static func reduce(value: inout [UInt64: CGRect], nextValue: () -> [UInt64: CGRect]) { value.merge(nextValue(), uniquingKeysWith: { _, new in new }) }
}
private extension JSON { var id: UInt64 { self["id"].uint } }

private struct LayerButton: View {
    let icon: String
    let label: String
    var enabled = true
    var selected = false
    var width: CGFloat = 24
    var height: CGFloat = 24
    var size: CGFloat = 16
    let action: () -> Void
    var body: some View {
        IconTile(icon: icon, label: label, selected: selected, enabled: enabled, size: size, action: action)
            .frame(width: width, height: height).accessibilityIdentifier("layer-" + label)
    }
}

private struct LayerRow: View {
    @ObservedObject var store: EditorStore
    let layer: JSON
    @ObservedObject var previews: LayerThumbnails
    var preview = false
    var context: (Bool) -> Void = { _ in }
    var dragging: (DragGesture.Value) -> Void = { _ in }
    var drop: () -> Void = {}
    var cancelDrag: () -> Void = {}
    @GestureState private var dragActive = false
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var id: UInt64 { layer["id"].uint }
    private var metadata: String {
        var parts: [String] = []
        if layer["blend"].uint != 0 { parts.append(layer["blend_label"].string) }
        if layer["opacity"].number < 1 { parts.append("\(Int((layer["opacity"].number * 100).rounded()))%") }
        return parts.joined(separator: " · ")
    }
    var body: some View {
        HStack(spacing: 2) {
            LayerButton(icon: layer["visible"].bool ? "eye" : "eye-hidden", label: layer["visible"].bool ? "Hide layer" : "Show layer", height: 36) {
                store.dispatch(["type": "set_layer_visibility", "id": id, "visible": !layer["visible"].bool])
            }.editorContextAction { context(false) }
            LayerButton(icon: layer["selection_icon"].string, label: "Select layer without changing drawing target", height: 36) {
                store.layer(["op": "toggle_selection", "id": id])
            }.accessibilityAddTraits(layer["selected"].bool ? .isSelected : [])
                .editorContextAction { context(false) }
            HStack(spacing: 2) {
                RoundedRectangle(cornerRadius: 1).fill(Color(red: 233/255, green: 153/255, blue: 165/255))
                    .frame(width: 3, height: 28).opacity(layer["clipped"].bool ? 1 : 0)
                thumbnail(mask: false)
                if layer["has_mask"].bool {
                    LayerButton(icon: "link", label: layer["mask_linked"].bool ? "Unlink mask from layer" : "Link mask to layer", width: 12, size: 12) {
                        store.layer(["op": "link_mask", "id": id, "value": !layer["mask_linked"].bool])
                    }.opacity(layer["mask_linked"].bool ? 1 : 0.35)
                    thumbnail(mask: true)
                }
            }.padding(.leading, min(CGFloat(layer["depth"].uint) * 8, 24))
            VStack(alignment: .leading, spacing: 0) {
                LayerName(store: store, layer: layer, preview: preview)
                if !metadata.isEmpty { Text(metadata).lineLimit(1).opacity(0.55) }
            }.frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 6)
                .contentShape(Rectangle()).onTapGesture {
                    if store.state["layer_tools"]["rename_layer"].uint != id {
                        store.layer(["op": "select", "id": id, "mask": false])
                    }
                }
                .editorContextAction { context(false) }
            SharedIcon(name: layer["locked"].bool ? "lock" : "alpha-lock", size: 12)
                .opacity(layer["locked"].bool || layer["alpha_locked"].bool ? 1 : 0)
            if layer["can_drop_below"].bool {
                SharedIcon(name: "grip", size: 12).opacity(0.6).frame(width: 12, height: 36)
                    .contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 6, coordinateSpace: .named("layer-panel"))
                        .updating($dragActive) { _, active, _ in active = true }
                        .onChanged(dragging).onEnded { _ in drop() })
                    .accessibilityLabel("Move layer")
            }
        }.padding(.horizontal, 6).padding(.vertical, 2).frame(minHeight: 40)
            .background((layer["selected"].bool ? palette.active : Color.clear)
                .contentShape(Rectangle()).editorContextAction { context(false) })
            .onChange(of: dragActive) { wasActive, active in
                if wasActive && !active {
                    DispatchQueue.main.async { if !dragActive { cancelDrag() } }
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityValue(layer["mask_selected"].bool ? "Editing mask" : layer["editing"].bool ? "Drawing target" : layer["selected"].bool ? "Selected" : "")
            .accessibilityIdentifier("layer-row-\(id)")
    }
    private func thumbnail(mask: Bool) -> some View {
        Button {
            store.layer(!mask && layer["group"].bool ? ["op": "collapse", "id": id] : ["op": "select", "id": id, "mask": mask])
        } label: {
            ZStack {
                if !mask && layer["group"].bool { SharedIcon(name: layer["collapsed"].bool ? "folder" : "folder-open", size: 28) }
                else if !mask && !layer["content_icon"].isNull { SharedIcon(name: layer["content_icon"].string, size: 28) }
                else if let image = previews.images[LayerThumbnails.key(id, mask)] {
                    Image(decorative: image, scale: 1).resizable().frame(width: 28, height: 28)
                        .opacity(mask && !layer["mask_enabled"].bool ? 0.4 : 1)
                }
            }.frame(width: 30, height: 30)
                .background(!mask && layer["group"].bool ? Color.clear : palette["input"], in: RoundedRectangle(cornerRadius: 3))
                .overlay {
                    if mask ? layer["mask_selected"].bool : layer["editing"].bool && !layer["mask_selected"].bool {
                        TargetCorners().stroke(.white, lineWidth: 1).shadow(color: .black, radius: 1).allowsHitTesting(false)
                    }
                }
                // Bound the hit region as well as the drawing. Without this,
                // iPad thumbnail hits can consume the adjacent checkbox tap.
                .contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityLabel(mask ? "Edit layer mask" : layer["group"].bool ? "Collapse or expand group" : "Edit layer content")
            .accessibilityAddTraits((mask ? layer["mask_selected"].bool : layer["editing"].bool && !layer["mask_selected"].bool) ? .isSelected : [])
            .editorContextAction { context(mask) }
    }
}

private struct TargetCorners: Shape {
    func path(in r: CGRect) -> Path {
        var p = Path()
        for (x, y, dx, dy) in [(r.minX,r.minY,1.0,1.0),(r.maxX,r.minY,-1.0,1.0),(r.minX,r.maxY,1.0,-1.0),(r.maxX,r.maxY,-1.0,-1.0)] {
            p.move(to: CGPoint(x: x + dx * 7, y: y)); p.addLine(to: CGPoint(x: x, y: y)); p.addLine(to: CGPoint(x: x, y: y + dy * 7))
        }
        return p
    }
}

private struct LayerName: View {
    @ObservedObject var store: EditorStore
    let layer: JSON
    let preview: Bool
    @State private var name = ""
    @State private var finished = false
    @FocusState private var focused: Bool
    private var renaming: Bool { !preview && store.state["layer_tools"]["rename_layer"].uint == layer["id"].uint }
    var body: some View {
        Group {
            if renaming {
                TextField("Layer name", text: $name).textFieldStyle(.plain).focused($focused)
                    .onSubmit { finish() }.onKeyPress(.escape) { finish(cancel: true); return .handled }
            } else {
                Text(layer["label"].string).lineLimit(1).help(layer["label"].string)
                    .onTapGesture(count: 2) {
                        if layer["editable"].bool && !layer["locked"].bool { store.layer(["op": "begin_rename", "id": layer["id"].raw]) }
                    }
            }
        }.onChange(of: renaming, initial: true) { _, active in
            if active { name = layer["label"].string; finished = false; focused = true }
        }.onChange(of: focused) { old, next in if old && !next && renaming { finish() } }
    }
    private func finish(cancel: Bool = false) {
        guard !finished else { return }; finished = true
        if cancel || name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { store.layer(["op": "cancel_rename"]) }
        else { store.layer(["op": "rename", "id": layer["id"].raw, "name": String(name.prefix(128))]) }
    }
}

private struct LayerOpacityField: View {
    @ObservedObject var store: EditorStore
    @State private var text = ""
    @State private var fill = 0.0
    @State private var localValue: Double?
    @State private var pendingValue: Double?
    @State private var canceled = false
    @State private var error: String?
    @FocusState private var editing: Bool
    private var value: Double { store.state["layer_tools"]["editing_layer"]["opacity"].number }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        HStack(spacing: 4) {
            GeometryReader { geometry in
                ZStack(alignment: .leading) {
                    Capsule().fill(palette["input"])
                    Rectangle().fill(palette["text"].opacity(0.5)).frame(width: max(0, geometry.size.width * fill))
                }.frame(height: 4).clipShape(Capsule()).frame(height: 24).contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0).onChanged { event in
                        guard geometry.size.width > 0 else { return }
                        resolve(["type": "position", "position": min(1, max(0, event.location.x / geometry.size.width))])
                    })
            }.frame(height: 24).accessibilityLabel("Layer opacity")
                .accessibilityValue(text).accessibilityAdjustableAction { direction in resolve(["type": "step", "steps": direction == .increment ? 1 : -1]) }
            TextField("Layer opacity", text: $text).textFieldStyle(.plain).multilineTextAlignment(.trailing)
                .monospacedDigit().frame(width: 32).focused($editing).onSubmit { resolve(["type": "expression", "text": text]) }
                .onKeyPress(.escape) { canceled = true; editing = false; format(); return .handled }
        }.frame(height: 24).onAppear { localValue = value; format() }
            .overlay(RoundedRectangle(cornerRadius: 4).stroke(error == nil ? Color.clear : Color.red, lineWidth: 1))
            .accessibilityHint(error ?? "")
            .onChange(of: value) { _, next in
                if let pendingValue, abs(next - pendingValue) > 0.00001 { return }
                pendingValue = nil; localValue = next
                if !editing { format() }
            }
            .onChange(of: store.state["layer_tools"]["editing_layer"]["id"].uint) { _, _ in
                canceled = true; editing = false; pendingValue = nil; localValue = value; format()
            }
            .onChange(of: editing) { old, next in
                if next { canceled = false }
                if old && !next && !canceled { resolve(["type": "expression", "text": text]) }
            }
    }
    private func format() {
        do {
            let result = try store.resolveNumber(store.catalog["layer_opacity"], value: localValue ?? value, operation: ["type": "format"])
            if !editing { text = result["edit"].string }; fill = result["fill"].number; error = nil
        } catch { self.error = error.localizedDescription }
    }
    private func resolve(_ operation: [String: Any]) {
        do {
            let result = try store.resolveNumber(store.catalog["layer_opacity"], value: localValue ?? value, operation: operation)
            fill = result["fill"].number; text = result["edit"].string
            localValue = result["value"].number; pendingValue = localValue; error = nil
            store.dispatch(["type": "set_layer_opacity", "opacity": localValue!])
        } catch { self.error = error.localizedDescription }
    }
}

private struct LayerActionMenu: View {
    @ObservedObject var store: EditorStore
    let menu: JSON
    let dismiss: () -> Void
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(menu["sections"].array.indices, id: \.self) { section in
                    if section > 0 { Divider().padding(.vertical, 4) }
                    ForEach(menu["sections"][section].array.indices, id: \.self) { index in
                        let item = menu["sections"][section][index]
                        if item["sections"].array.isEmpty {
                            Button { store.dispatch(item["action"]); dismiss() } label: { label(item) }.buttonStyle(.plain).disabled(!item["enabled"].bool)
                        } else {
                            Menu { MenuItems(store: store, sections: item["sections"], didInvoke: dismiss) } label: { label(item) }
                                .menuStyle(.borderlessButton).disabled(!item["enabled"].bool)
                        }
                    }
                }
            }.padding(6)
        }.frame(width: 340).frame(maxHeight: 560).font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
    }
    private func label(_ item: JSON) -> some View {
        HStack(spacing: 6) {
            Image(systemName: "checkmark").opacity(item["selected"].bool ? 1 : 0).frame(width: 16)
            Text(item["label"].string).lineLimit(1)
            Spacer(minLength: 4)
            Text(item["hint"].string).opacity(0.55)
        }.padding(.horizontal, 4).frame(height: 28).contentShape(Rectangle())
    }
}
