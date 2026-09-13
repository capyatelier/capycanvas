import SwiftUI
import UniformTypeIdentifiers

struct LayerPanel: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    @StateObject private var interaction = LayerRowInteraction()
    @State private var importing = false
    @State private var popupID = UUID()
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
                                LayerRow(store: store, layer: layer, previews: store.layerThumbnails, interaction: interaction)
                                    .modifier(LayerRowMeasurement(id: layer["id"].uint))
                                    .overlay { dropMark(layer) }
                                    .editorPopover(isPresented: menuPresented(at: .row(layer["id"].uint)), placement: .inward) { menuContent }
                                    .onAppear { store.layerThumbnails.show(layer["id"].uint) }
                                    .onDisappear {
                                        store.layerThumbnails.hide(layer["id"].uint)
                                        if interaction.menuSource == .row(layer["id"].uint) { closeMenu() }
                                    }
                            }
                        }.coordinateSpace(name: "layer-rows")
                            .background(NativeReorderInput(model: interaction))
                            .onPreferenceChange(LayerRowFrames.self) { interaction.frames = $0 }
                            .overlay(alignment: .topLeading) { dragPreview }
                            .modifier(PanelBodyMeasurement(panel: "layers", part: "rows"))
                    }.accessibilityIdentifier("layer-rows").modifier(LayerInputCheckOrder(layers: layers))
                } else { Spacer(minLength: 0) }
                if visible("layer_actions") { footer.modifier(PanelBodyMeasurement(panel: "layers", part: "footer")) }
            }
        }
            .onAppear { interaction.store = store }
            .onChange(of: interaction.menu.isNull) { _, empty in store.workspace.popover(popupID, open: !empty) }
            .onChange(of: store.state["revision"].uint) { _, _ in
                interaction.validate(); store.layerThumbnails.refresh()
            }
            .onChange(of: store.state["layer_tools"]["rename_layer"].uint) { _, _ in interaction.validate() }
            .onChange(of: store.state["document_file"]["epoch"].uint) { _, _ in interaction.cancel() }
            .onDisappear { interaction.cancel(); store.workspace.popover(popupID, open: false) }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.image]) { result in
                switch result {
                case .success(let url): store.importLayer(url)
                case .failure(let error): store.failure = error.localizedDescription
                }
            }
    }
    @ViewBuilder private var dragPreview: some View {
        if !interaction.nativeDragging, let drag = interaction.drag,
           let layer = layers.first(where: { $0["id"].uint == drag.id }) {
            LayerRow(store: store, layer: layer, previews: store.layerThumbnails, preview: true)
                .frame(width: drag.bounds.width)
                .background(palette["panel"]).opacity(0.7).allowsHitTesting(false).accessibilityHidden(true)
                .offset(x: drag.bounds.minX, y: drag.bounds.minY + drag.point.y - drag.origin.y)
        }
    }
    private var header: some View {
        VStack(spacing: 2) {
            HStack(spacing: 6) {
                EditorChoice(label: "Layer blend mode", options: store.catalog["layer_blends"].array.map(\.string),
                    selected: Int(current["blend"].uint), identifier: "layer-blend", background: palette["input"], compact: true) {
                    store.layer(["op": "blend", "id": current["id"].raw, "value": $0])
                }.disabled(!view["controls"]["blend"].bool).frame(maxWidth: .infinity)
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
            LayerButton(icon: "add-layer", label: "New layer") { store.layer(["op": "new", "group": false, "clipped": false]) }
            LayerButton(icon: "folder", label: "New group") { store.layer(["op": "new", "group": true, "clipped": false]) }
            LayerButton(icon: "mask", label: "Add layer mask", enabled: view["controls"]["mask"].bool) {
                store.layer(["op": "add_mask", "id": current["id"].raw, "replace": false])
            }
            LayerButton(icon: "image", label: "Import image as layer", enabled: store.snapshot["canvas_ready"].bool) { importing = true }
            LayerButton(icon: "delete", label: "Delete selected layers", enabled: view["can_delete"].bool) { store.layer(["op": "delete_selected"]) }
            Spacer(minLength: 0)
            LayerButton(icon: "more", label: "Layer actions", enabled: !current.isNull) {
                openMenu(current, mask: current["mask_selected"].bool, source: .footer)
            }.editorPopover(isPresented: menuPresented(at: .footer), placement: .inward) { menuContent }
        }.padding(.horizontal, 6).padding(.vertical, 4)
    }
    private func menuPresented(at source: LayerMenuSource) -> Binding<Bool> {
        Binding(get: { interaction.menuSource == source && !interaction.menu.isNull }, set: { presented in
            if !presented && interaction.menuSource == source { closeMenu() }
        })
    }
    private var menuContent: some View {
        EditorActionMenu(model: AppleContextMenu(interaction.menu) { store.dispatch($0) },
            identifier: "layer-context-menu", dismiss: closeMenu)
    }
    private func closeMenu() { interaction.closeMenu() }
    private func openMenu(_ layer: JSON, mask: Bool, source: LayerMenuSource? = nil) {
        interaction.openMenu(id: layer["id"].uint, mask: mask, source: source)
    }
    @ViewBuilder private func dropMark(_ layer: JSON) -> some View {
        if let drag = interaction.drag, drag.target == layer["id"].uint {
            if layer["group"].bool && (0.25..<0.75).contains(drag.fraction) {
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

/// The isolated UI workflow reads complete order, including unmounted rows.
/// Ordinary builds keep the native scroll view's accessibility value unchanged.
private struct LayerInputCheckOrder: ViewModifier {
    let layers: [JSON]
    @ViewBuilder func body(content: Content) -> some View {
        #if DEBUG
        if ProcessInfo.processInfo.environment["CAPY_LAYER_INPUT_PROBE"] == "1" {
            content.accessibilityValue(layers.map { String($0["id"].uint) }.joined(separator: ","))
        } else { content }
        #else
        content
        #endif
    }
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
    var interaction: LayerRowInteraction?
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
                perform { store.dispatch(["type": "set_layer_visibility", "id": id, "visible": !layer["visible"].bool]) }
            }
            LayerButton(icon: layer["selection_icon"].string, label: "Select layer without changing drawing target", height: 36) {
                perform { store.layer(["op": "toggle_selection", "id": id]) }
            }.accessibilityAddTraits(layer["selected"].bool ? .isSelected : [])
            HStack(spacing: 2) {
                RoundedRectangle(cornerRadius: 1).fill(Color(red: 233/255, green: 153/255, blue: 165/255))
                    .frame(width: 3, height: 28).opacity(layer["clipped"].bool ? 1 : 0)
                thumbnail(mask: false)
                if layer["has_mask"].bool {
                    LayerButton(icon: "link", label: layer["mask_linked"].bool ? "Unlink mask from layer" : "Link mask to layer", width: 12, size: 12) {
                        perform { store.layer(["op": "link_mask", "id": id, "value": !layer["mask_linked"].bool]) }
                    }.opacity(layer["mask_linked"].bool ? 1 : 0.35)
                    thumbnail(mask: true)
                }
            }.padding(.leading, min(CGFloat(layer["depth"].uint) * 8, 24))
            VStack(alignment: .leading, spacing: 0) {
                LayerName(store: store, layer: layer, preview: preview, allowsAction: { interaction?.contact.consumeClick() != true })
                if !metadata.isEmpty { Text(metadata).lineLimit(1).opacity(0.55) }
            }.frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 6)
                .modifier(LayerRowMeasurement(id: id, part: \.name, enabled: !preview))
                .contentShape(Rectangle()).onTapGesture {
                    if store.state["layer_tools"]["rename_layer"].uint != id {
                        perform { store.layer(["op": "select", "id": id, "mask": false]) }
                    }
                }
            SharedIcon(name: layer["locked"].bool ? "lock" : "alpha-lock", size: 12)
                .opacity(layer["locked"].bool || layer["alpha_locked"].bool ? 1 : 0)
            if layer["can_drop_below"].bool {
                SharedIcon(name: "grip", size: 12).opacity(0.6).frame(width: 12, height: 36)
                    .contentShape(Rectangle())
                    .modifier(LayerRowMeasurement(id: id, part: \.grip, enabled: !preview))
                    .accessibilityLabel("Move layer").accessibilityIdentifier("layer-grip-\(id)")
            }
        }.padding(.horizontal, 6).padding(.vertical, 2).frame(minHeight: 40)
            .background((layer["selected"].bool ? palette.active : Color.clear)
                .contentShape(Rectangle()).onTapGesture {
                    perform { store.layer(["op": "select", "id": id, "mask": false]) }
                })
            .accessibilityElement(children: .contain)
            .accessibilityValue(layer["mask_selected"].bool ? "Editing mask" : layer["editing"].bool ? "Drawing target" : layer["selected"].bool ? "Selected" : "")
            .accessibilityIdentifier("layer-row-\(id)")
    }
    private func thumbnail(mask: Bool) -> some View {
        Button {
            perform { store.layer(!mask && layer["group"].bool ? ["op": "collapse", "id": id] : ["op": "select", "id": id, "mask": mask]) }
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
            .accessibilityIdentifier("layer-thumbnail-\(id)-\(mask ? "mask" : "content")")
            .accessibilityValue(thumbnailCaptureStatus(mask: mask))
            .accessibilityAddTraits((mask ? layer["mask_selected"].bool : layer["editing"].bool && !layer["mask_selected"].bool) ? .isSelected : [])
            .modifier(LayerRowMeasurement(id: id, part: \.mask, enabled: mask && !preview))
    }
    private func perform(_ action: () -> Void) {
        guard !preview, interaction?.contact.consumeClick() != true else { return }
        action()
    }
    private func thumbnailCaptureStatus(mask: Bool) -> String {
        #if DEBUG
        if ProcessInfo.processInfo.environment["CAPY_CAPTURE_PROBE"] == "1" {
            let symbolic = !mask && (layer["group"].bool || !layer["content_icon"].isNull)
            return symbolic || previews.images[LayerThumbnails.key(id, mask)] != nil ? "Preview ready" : "Preview pending"
        }
        #endif
        return ""
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
    var allowsAction: () -> Bool = { true }
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
                        if allowsAction(), layer["editable"].bool && !layer["locked"].bool { store.layer(["op": "begin_rename", "id": layer["id"].raw]) }
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

struct LayerOpacityField: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        let layer = store.state["layer_tools"]["editing_layer"]
        let epoch = store.state["document_file"]["epoch"].uint
        NumberControl(store: store, label: "Layer opacity", value: layer["opacity"].number,
            control: store.catalog["layer_opacity"], identifier: "layer-opacity", inline: true) { value, completion in
            // A late focus callback from a removed field must not edit another
            // layer or a new document whose IDs happen to match the old one.
            guard store.state["document_file"]["epoch"].uint == epoch,
                  store.state["layer_tools"]["editing_layer"]["id"].uint == layer["id"].uint else {
                completion(nil); return
            }
            store.edit(["type": "set_layer_opacity", "id": layer["id"].raw, "opacity": value], completion: completion)
        }.id("\(epoch):\(layer["id"].uint)")
    }
}
