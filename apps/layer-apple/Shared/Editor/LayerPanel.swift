import SwiftUI

struct LayerPanel: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    var drawerPagePosition: CGFloat? = nil
    var simple = false
    @StateObject private var interaction = LayerRowInteraction()
    @State private var popupID = UUID()
    @State private var filterMenu = JSON()
    @StateObject private var scrollPosition = EditorScrollPosition()
    @State private var retainedOffset: CGFloat?
    @State private var paperHover = false
    @State private var footerWidth: CGFloat = 0
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var view: JSON { store.state["layer_tools"] }
    private var current: JSON { view["editing_layer"] }
    private var editingTarget: String { JSON([store.state["document_file"]["epoch"].raw, current["id"].raw]).stableKey }
    private var layers: [JSON] { store.state["layers"].array }
    private var rows: [LayerPanelRow] {
        layers.map { LayerPanelRow(layer: $0) }
    }
    private func visible(_ control: String) -> Bool {
        panel["controls"].array.contains { $0["control"].string == control && $0["visible_in_panel"].bool }
    }
    var body: some View {
        ZStack(alignment: .topLeading) {
            VStack(spacing: 0) {
                if !simple && visible("layer_opacity") { header.modifier(PanelBodyMeasurement(panel: "layers", part: "header")) }
                if visible("layers") {
                    GeometryReader { viewport in
                    EditorPositionedScrollView(position: scrollPosition) {
                        LazyVStack(spacing: 0) {
                            ForEach(rows) { row in
                                let thumbnailToken = "\(popupID):\(row.id)"
                                Group {
                                    LayerSwipeRow(store: store, swipe: store.layerSwipe, owner: interaction.swipeOwner, layer: row.layer) {
                                        LayerRow(store: store, swipe: store.layerSwipe, layer: row.layer, previews: store.layerThumbnails, simple: simple, interaction: interaction)
                                    }.modifier(PhotoDropTarget(store: store, row: row.id))
                                }
                                    .modifier(LayerRowMeasurement(id: row.id))
                                    .modifier(PanelBodyMeasurement(panel: "layers", part: "row-unit", kind: .unit))
                                    .overlay(alignment: .topLeading) { dropMark(row.layer) }
                                    .editorPopover(isPresented: menuPresented(at: .row(row.id)), placement: .inward) { menuContent }
                                    .editorPopover(isPresented: Binding(get: {
                                        view["opacity_popup"].optionalUInt == row.id && view["opacity_surface"].uint == interaction.touchSurface
                                    }, set: { shown in if !shown && view["opacity_surface"].uint == interaction.touchSurface {
                                        store.layer(["op": "close_opacity"])
                                    } }), placement: .inward) {
                                        LayerOpacityField(store: store, inline: false).frame(width: 240).padding(12)
                                            .accessibilityIdentifier("layer-gesture-opacity")
                                    }
                                    .onAppear { store.layerThumbnails.show(token: thumbnailToken, id: row.id) }
                                    .onDisappear {
                                        store.layerThumbnails.hide(thumbnailToken)
                                        if interaction.menuSource == .row(row.id) { closeMenu() }
                                    }
                            }
                            if !view["add_paper"].isNull {
                                Button {
                                    let action = view["add_paper"]["action"]
                                    if !action.isNull { store.dispatch(["type": "effect", "action": action.raw]) }
                                } label: {
                                    HStack(spacing: store.panelMetrics.group) {
                                        SharedIcon(name: view["add_paper"]["icon"].string)
                                        Text(view["add_paper"]["label"].string).fontWeight(.regular).lineLimit(1)
                                    }.padding(.horizontal, 6).padding(.vertical, 2)
                                        .frame(maxWidth: .infinity, minHeight: 40).contentShape(Rectangle())
                                }.buttonStyle(EditorControlButtonStyle(active: paperHover, corner: .radius(0)))
                                    .onHover { paperHover = $0 }
                                    .onDisappear { paperHover = false }
                                    .accessibilityIdentifier("layer-add-paper")
                                    .modifier(PanelBodyMeasurement(panel: "layers", part: "row-unit", kind: .unit))
                            }
                        }.scrollTargetLayout().coordinateSpace(name: "layer-rows")
                            .background(NativeReorderInput(model: interaction))
                            .onPreferenceChange(LayerRowFrames.self) { interaction.frames = $0 }
                            .overlay(alignment: .topLeading) { dragPreview }
                            .modifier(PanelBodyMeasurement(panel: "layers", part: "rows", kind: .scroll))
                            .modifier(BubbleNativeControl(viewport: viewport.frame(in: .named("editor-workspace")), cancel: { interaction.cancel() }))
                    }.overlayPreferenceValue(LayerRowFrames.self) { frames in
                        LayerConnections(store: store, frames: frames)
                    }.accessibilityIdentifier("layer-rows").modifier(LayerInputCheckOrder(layers: layers))
                    }
                } else { Spacer(minLength: 0) }
                if !simple && visible("layer_actions") { footer.modifier(PanelBodyMeasurement(panel: "layers", part: "footer")) }
            }
        }
            .onAppear { interaction.store = store; interaction.scrollPosition = scrollPosition }
            .onChange(of: drawerPagePosition) { before, next in
                if before == 0 && next != 0 { retainedOffset = scrollPosition.offset }
                if next == 0, let offset = retainedOffset { scrollPosition.scrollTo(y: offset); retainedOffset = nil }
            }
            .onChange(of: editingTarget) { _, _ in
                retainedOffset = nil
                if !current["id"].isNull { scrollPosition.scrollTo(id: current["id"].uint) }
            }
            .onChange(of: store.state["revision"].uint) { _, _ in
                interaction.validate(); store.layerThumbnails.refresh()
            }
            .onChange(of: store.state["layer_tools"]["rename_layer"].optionalUInt) { _, _ in interaction.validate() }
            .onChange(of: store.state["document_file"]["epoch"].uint) { _, _ in
                interaction.cancel(); filterMenu = JSON()
            }
            .onDisappear { interaction.cancel() }

    }
    @ViewBuilder private var dragPreview: some View {
        if let drag = interaction.drag,
           let row = rows.first(where: { $0.id == drag.id }) {
            Group {
                LayerRow(store: store, swipe: store.layerSwipe, layer: row.layer, previews: store.layerThumbnails, preview: true)
            }
                .frame(width: drag.bounds.width)
                .background(palette["panel"]).opacity(0.7).allowsHitTesting(false).accessibilityHidden(true)
                .offset(x: drag.bounds.minX, y: drag.bounds.minY + drag.point.y - drag.origin.y)
        }
    }
    private var header: some View {
        VStack(spacing: store.panelMetrics.spacing) {
            HStack(spacing: store.panelMetrics.spacing) {
                let blends = store.catalog["layer_blends"].array.map(\.string)
                EditorChoice(label: store.catalog["native_copy"]["layers"]["blend"].string, options: blends,
                    selected: blends.firstIndex(of: current["blend_label"].string) ?? -1, identifier: "layer-blend", background: palette.controlFill, presentation: .layerMenu, height: store.panelMetrics.height,
                    menu: { [store, id = current["id"].raw] show in
                        store.query(["type": "layer_blend_menu", "id": id]) { menu in
                            if !menu.isNull { show(AppleContextMenu(menu) { store.dispatch($0) }) }
                        }
                    }) { _ in }.disabled(!view["controls"]["blend"].bool).frame(maxWidth: .infinity)
                LayerOpacityField(store: store).disabled(!view["controls"]["opacity"].bool).frame(maxWidth: .infinity)
            }
            HStack(spacing: store.panelMetrics.compound) {
                flag("alpha-lock", store.catalog["native_copy"]["layers"]["alpha_lock"].string, "alpha_locked", "alpha_lock", "alpha_lock")
                flag("lock", store.catalog["native_copy"]["layers"]["lock_editing"].string, "locked", "lock", "edit_lock")
                let attachment = view["attachment"]
                LayerButton(icon: attachment["icon"].string, label: attachment["label"].string, side: store.panelMetrics.height,
                    enabled: !attachment["action"].isNull, selected: attachment["checked"].bool) {
                    let action = store.state["layer_tools"]["attachment"]["action"]
                    if !action.isNull { store.layer(action.object) }
                }.accessibilityHint(attachment["description"].string).help(attachment["description"].string)
                    .accessibilityIdentifier("layer-attachment")
                LayerButton(icon: "reference", label: view["reference_action_label"].string, side: store.panelMetrics.height,
                    enabled: view["can_reference"].bool, selected: view["references_selected"].bool) {
                    store.layer(["op": "reference_selection"])
                }
                Spacer(minLength: 0)
            }
        }.padding(.horizontal, layerEdgeInset).padding(.vertical, 4)
    }
    private func flag(_ icon: String, _ label: String, _ property: String, _ op: String, _ capability: String) -> some View {
        LayerButton(icon: icon, label: label, side: store.panelMetrics.height, enabled: view["controls"][capability].bool, selected: current[property].bool) {
            store.layer(["op": op, "id": current["id"].raw, "value": !current[property].bool])
        }
    }
    private var footerOverflow: [JSON] {
        view["footer"].array.filter { footerWidth + 0.5 < $0["min_width"].number }.map { $0["item"] }
    }
    private var footer: some View {
        HStack(spacing: store.panelMetrics.compound) {
            ForEach(view["footer"].array.indices, id: \.self) { index in
                let slot = view["footer"][index]
                if footerWidth + 0.5 >= slot["min_width"].number { footerButton(slot) }
            }
            Spacer(minLength: 0)
            LayerButton(icon: "more-small", label: store.catalog["native_copy"]["layers"]["actions"].string, side: store.panelMetrics.height, enabled: !current.isNull) {
                openMenu(current, mask: current["mask_selected"].bool, source: .footer)
            }.editorPopover(isPresented: menuPresented(at: .footer), placement: .inward) { menuContent }
        }.padding(.horizontal, layerEdgeInset).padding(.vertical, 4)
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { footerWidth = $0 }
    }
    @ViewBuilder private func footerButton(_ slot: JSON) -> some View {
        let item = slot["item"], kind = slot["button"].string
        let button = LayerButton(icon: item["icon"].string, label: item["label"].string, side: store.panelMetrics.height,
            enabled: item["enabled"].bool, selected: item["selected"].bool) {
                if kind == "add_filter" { filterMenu = view["add_filter"] }
                else if !item["action"].isNull { store.dispatch(item["action"]) }
            }
        if kind == "add_filter" {
            button.accessibilityIdentifier("layer-add-filter")
                .editorPopover(isPresented: Binding(get: { !filterMenu.isNull }, set: { if !$0 { filterMenu = JSON() } }), placement: .inward) {
                    EditorActionMenu(model: AppleContextMenu(filterMenu) { store.dispatch($0) }, identifier: "layer-filter-menu", dismiss: { filterMenu = JSON() })
                }
        } else {
            button.accessibilityIdentifier(kind == "new_vector_layer" ? "layer-new-vector" : "layer-" + item["label"].string)
        }
    }
    private func menuPresented(at source: LayerMenuSource) -> Binding<Bool> {
        Binding(get: { interaction.menuSource == source && !interaction.menu.isNull }, set: { presented in
            if !presented && interaction.menuSource == source { closeMenu() }
        })
    }
    private var menuContent: some View {
        let extra = interaction.menuSource == .footer ? footerOverflow : []
        let menu = extra.isEmpty ? interaction.menu : interaction.menu.replacing("sections",
            with: JSON([extra.map(\.raw)] + interaction.menu["sections"].array.map(\.raw)))
        return EditorActionMenu(model: AppleContextMenu(menu) { store.dispatch($0) },
            identifier: "layer-context-menu", dismiss: closeMenu)
    }
    private func closeMenu() { interaction.closeMenu() }
    private func openMenu(_ layer: JSON, mask: Bool, source: LayerMenuSource? = nil) {
        interaction.openMenu(id: layer["id"].uint, mask: mask, source: source)
    }
    @ViewBuilder private func dropMark(_ layer: JSON) -> some View {
        if let drag = interaction.drag {
            if drag.effectOwner == layer["id"].uint, let frame = interaction.frames[layer["id"].uint] {
                Rectangle().stroke(palette.accent, lineWidth: 2)
                    .frame(width: frame.content.width, height: frame.content.height)
                    .offset(x: frame.content.minX - frame.row.minX, y: frame.content.minY - frame.row.minY)
                    .allowsHitTesting(false)
            }
            if drag.target == layer["id"].uint, let position = drag.position {
                if position == "into" {
                    Rectangle().stroke(palette.accent, lineWidth: 2).allowsHitTesting(false)
                } else if position == "above" || position == "below" {
                    VStack(spacing: 0) {
                        if position == "below" { Spacer(minLength: 0) }
                        Rectangle().fill(palette.accent).frame(height: 2)
                        if position == "above" { Spacer(minLength: 0) }
                    }.allowsHitTesting(false)
                }
            }
        }
    }
}

private struct LayerConnections: View {
    @ObservedObject var store: EditorStore
    let frames: [UInt64: LayerRowFrame]
    var body: some View {
        GeometryReader { allocation in
            let rows = store.state["layers"].array
            let connections = store.state["layer_tools"]["connections"].array
            let palette = EditorPalette(source: store.state["palette"])
            let order = Dictionary(uniqueKeysWithValues: rows.enumerated().map { ($0.element["id"].uint, $0.offset) })
            let bounds = allocation.frame(in: .named("editor-workspace"))
            let geometry = frames.compactMapValues { frame -> CGRect? in
                guard frame.row.height > 0, frame.content.width > 0 else { return nil }
                return frame.content.offsetBy(dx: frame.root.minX - frame.row.minX - bounds.minX,
                    dy: frame.root.minY - frame.row.minY - bounds.minY)
            }
            Canvas { context, size in
                let viewport = CGRect(origin: .zero, size: size)
                let visible = rows.enumerated().filter { geometry[$0.element["id"].uint]?.intersects(viewport) == true }
                guard let first = visible.first, let last = visible.last,
                      let anchor = frames[first.element["id"].uint], anchor.swipe.width > 0 else { return }
                let column = anchor.content.minX - anchor.swipe.minX + anchor.root.minX - bounds.minX
                    - min(CGFloat(first.element["depth"].uint) * 8, 24)
                let endpoint = { (id: UInt64, bottom: Bool) -> CGFloat? in
                    if let frame = geometry[id] { return bottom ? frame.maxY : frame.minY }
                    guard let index = order[id] else { return nil }
                    if index < first.offset { return 0 }
                    if index > last.offset { return size.height }
                    return nil
                }
                let glyph = context.resolveSymbol(id: 0)
                for connection in connections {
                    let effect = connection["kind"].string == "effect"
                    guard let top = endpoint(connection["from"].uint, effect),
                          let bottom = endpoint(connection["to"].uint, !effect),
                          bottom > top, bottom > 0, top < size.height else { continue }
                    let x = column + min(CGFloat(connection["depth"].uint) * 8, 24)
                    var path = Path()
                    if effect {
                        let center = x + 15, y = (top + bottom) / 2
                        if y - top > 6 {
                            path.move(to: CGPoint(x: center, y: top)); path.addLine(to: CGPoint(x: center, y: y - 6))
                        }
                        if bottom - y > 6 {
                            path.move(to: CGPoint(x: center, y: y + 6)); path.addLine(to: CGPoint(x: center, y: bottom))
                        }
                        if let glyph { context.draw(glyph, at: CGPoint(x: center, y: y)) }
                    } else {
                        path.move(to: CGPoint(x: x - 3.5, y: top)); path.addLine(to: CGPoint(x: x - 3.5, y: bottom))
                    }
                    context.stroke(path, with: .color(palette[effect ? "text" : "relationship"]),
                        style: StrokeStyle(lineWidth: effect ? 1 : 2, lineCap: .round))
                }
            } symbols: {
                SharedIcon(name: "layer-effect-link-symbolic", size: 12).foregroundStyle(palette["text"]).tag(0)
            }
        }.clipped().allowsHitTesting(false).accessibilityHidden(true)
    }
}

private struct LayerSwipeRow<Content: View>: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var swipe: LayerSwipe
    let owner: UUID
    let layer: JSON
    @ViewBuilder var content: () -> Content
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var width: CGFloat = 216
    private var offset: CGFloat { swipe.owner == owner && swipe.layer == layer["id"].uint ? swipe.offset : 0 }
    var body: some View {
        content().offset(x: -offset)
            .background(alignment: .trailing) {
                if offset > 0 {
                    let actions = layer["swipe_actions"].array
                    let palette = EditorPalette(source: store.state["palette"])
                    HStack(spacing: 0) {
                        ForEach(actions.indices, id: \.self) { index in
                            let model = actions[index], destructive = model["id"].string == "delete"
                            Button { swipe.close(); store.layer(model["action"].object) } label: {
                                Text(model["label"].string).lineLimit(1).foregroundStyle(destructive ? .white : palette["text"])
                                    .padding(.horizontal, 4).frame(maxWidth: .infinity, maxHeight: .infinity)
                                    .background(destructive ? Color(red: 0.78, green: 0.16, blue: 0.16) : palette["text"].opacity(model["id"].string == "duplicate" ? 0.24 : 0.16))
                                    .contentShape(Rectangle())
                            }.buttonStyle(.plain).disabled(!model["enabled"].bool).help(model["label"].string)
                                .accessibilityIdentifier("layer-\(model["id"].string)-\(layer["id"].uint)")
                        }
                    }.frame(width: min(width, CGFloat(actions.count) * 72))
                        .mask(alignment: .trailing) { Rectangle().frame(width: max(0, offset)) }
                }
            }.clipped()
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
            .animation(swipe.tracking || reduceMotion ? nil : .easeOut(duration: 0.15), value: offset)
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

private struct LayerPanelRow: Identifiable {
    let layer: JSON
    var id: UInt64 { layer.id }
}

private struct LayerButton: View {
    let icon: String
    let label: String
    let side: CGFloat
    var enabled = true
    var selected = false
    var height: CGFloat? = nil
    var size: CGFloat = 16
    let action: () -> Void
    var body: some View {
        IconTile(icon: icon, label: label, selected: selected, enabled: enabled, size: size, action: action)
            .frame(width: side, height: height ?? side).accessibilityIdentifier("layer-" + label)
    }
}

let layerEdgeInset = CGFloat(ToolbarUI.metrics["layer_edge_inset"].number)

private struct LayerRow: View {
    @State private var rowCaption = ""
    @Environment(\.editorPalette) private var surface
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ObservedObject var store: EditorStore
    @ObservedObject var swipe: LayerSwipe
    let layer: JSON
    @ObservedObject var previews: LayerThumbnails
    var preview = false
    var simple = false
    var interaction: LayerRowInteraction?
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var id: UInt64 { layer["id"].uint }
    private var gripOpacity: Double {
        guard !preview, let interaction else { return 0.6 }
        let width = min(CGFloat(layer["swipe_actions"].array.count) * 72, interaction.frames[id]?.root.width ?? 216)
        return 0.6 * Double(1 - swipe.progress(owner: interaction.swipeOwner, layer: id, revealWidth: width))
    }
    var body: some View {
        HStack(spacing: 2) {
            LayerButton(icon: layer["visible"].bool && !layer["visibility_blocked"].bool ? "eye" : "eye-hidden", label: visibilityLabel, side: store.panelMetrics.height, height: 36) {
                perform { store.dispatch(["type": "set_layer_visibility", "id": id, "visible": !layer["visible"].bool]) }
            }.opacity(layer["visibility_blocked"].bool ? 0.35 : 1)
            if !simple { LayerButton(icon: layer["selection_icon"].string, label: store.catalog["native_copy"]["layers"]["select_row_help"].string, side: store.panelMetrics.height, selected: layer["selected"].bool, height: 36) {
                selectRow(toggle: true)
            }.accessibilityAddTraits(layer["selected"].bool ? .isSelected : []) }
            HStack(spacing: 2) {
                Color.clear.frame(width: 3, height: 28)
                thumbnail(mask: false)
                if !simple && layer["selection_layer"].bool {
                    IconTile(icon: "selection-load", label: layer["load_selection_tooltip"].string) {
                        perform { store.dispatch(["type": "selection", "action": ["op": "load_layer", "id": id, "mode": "new", "inverted": false]]) }
                    }.frame(width: 30, height: 30).accessibilityIdentifier("selection-load-\(id)")
                }
                if !simple && layer["has_mask"].bool {
                    LayerButton(icon: layer["mask_linked"].bool ? "link" : "unlink", label: layer["mask_linked"].bool ? store.catalog["native_copy"]["layers"]["unlink_mask"].string : store.catalog["native_copy"]["layers"]["link_mask_to_layer"].string, side: store.panelMetrics.height, size: 10) {
                        perform { store.layer(["op": "link_mask", "id": id, "value": !layer["mask_linked"].bool]) }
                    }.foregroundStyle(palette["text"]).disabled(layer["locked"].bool)
                    thumbnail(mask: true)
                }
                if !simple && !layer["paper"].isNull { texture(layer["paper"]) }
            }.padding(.leading, min(CGFloat(layer["depth"].uint) * 8, 24))
            VStack(alignment: .leading, spacing: 0) {
                LayerName(store: store, layer: layer, preview: preview, allowsAction: { interaction?.contact.consumeClick() != true })
                if !simple && !layer["description"].string.isEmpty { Text(layer["description"].string).lineLimit(1).opacity(0.55) }
            }.frame(maxWidth: .infinity, alignment: .leading).padding(.leading, 6)
                .modifier(LayerRowMeasurement(id: id, part: \.name, enabled: !preview))
                .contentShape(Rectangle()).onTapGesture {
                    if store.state["layer_tools"]["rename_layer"].optionalUInt != id {
                        selectRow()
                    }
                }
            if !simple { SharedIcon(name: layer["locked"].bool ? "lock" : "alpha-lock", size: 12)
                .opacity(layer["locked"].bool || layer["alpha_locked"].bool ? 1 : 0) }
            if !simple && layer["can_drop_below"].bool {
                SharedIcon(name: "grip").opacity(gripOpacity).frame(width: 16, height: 36)
                    .contentShape(Rectangle())
                    .modifier(LayerRowMeasurement(id: id, part: \.grip, enabled: !preview))
                    .accessibilityLabel(store.catalog["native_copy"]["layers"]["move_layer"].string).accessibilityIdentifier("layer-grip-\(id)")
            }
        }.padding(.horizontal, layerEdgeInset).padding(.vertical, 2).frame(minHeight: 40)
            .modifier(LayerRowMeasurement(id: id, part: \.swipe, enabled: !preview))
            .background((layer["selected"].bool ? surface.active : Color.clear)
                .contentShape(Rectangle()).onTapGesture {
                    selectRow()
                })
            .accessibilityElement(children: .contain)
            .accessibilityLabel(rowCaption)
            .onAppear { refreshCaption() }
            .onChange(of: layer["label"].string) { _, _ in refreshCaption() }
            .onChange(of: store.interfaceLanguage) { _, _ in refreshCaption() }
            .accessibilityValue(layer["mask_selected"].bool ? store.catalog["native_copy"]["layers"]["editing_mask"].string
                : layer["drawing"].bool ? (layer["selection_layer"].bool ? store.catalog["native_copy"]["layers"]["editing_selection"].string : store.catalog["native_copy"]["layers"]["drawing_target"].string)
                : layer["selected"].bool ? store.catalog["native_copy"]["layers"]["selected"].string : "")
            .accessibilityIdentifier("layer-row-\(id)")
    }
    private func refreshCaption() { rowCaption = NativeTextContext.caption(["type": "layer_row", "title": layer["label"].string], language: store.interfaceLanguage) }
    private var visibilityLabel: String {
        layer["selection_layer"].bool ? (layer["visible"].bool ? store.catalog["native_copy"]["layers"]["hide_selection"].string : store.catalog["native_copy"]["layers"]["show_selection"].string)
            : (layer["visible"].bool ? store.catalog["native_copy"]["layers"]["hide"].string : store.catalog["native_copy"]["layers"]["show"].string)
    }
    private func thumbnail(mask: Bool) -> some View {
        let editing = mask ? layer["mask_selected"].bool : layer["content_selected"].bool
        let target: [String: Any] = !mask && layer["group"].bool ? ["op": "collapse", "id": id] : ["op": "select", "id": id, "mask": mask]
        let copy = store.catalog["native_copy"]["layers"]
        let label = mask ? copy["edit_mask"].string : layer["group"].bool ? copy[layer["collapsed"].bool ? "expand" : "collapse"].string
            : layer["selection_layer"].bool ? copy["edit_selection"].string : copy["edit_content"].string
        return Button {
            perform {
                if let load = ThumbnailSelectionLoad.current(), !layer["group"].bool {
                    store.dispatch(["type": "selection", "action": ["op": "load_thumbnail", "id": id, "mask": mask, "shift": load.shift, "alt": load.alt]])
                } else {
                    store.layer(target)
                    let fill = layer["fill_color"]
                    if !mask && !fill.isNull {
                        store.colorEditing.open(colors: store.displayColors, value: fill["color"], opaque: fill["opaque"].bool, viewing: store.colorViewing) { color, _ in
                            store.dispatch(["type": "effect", "action": ["op": "set", "layer": id, "key": fill["key"].raw, "value": ["kind": "color", "value": color.raw]]])
                        }
                    }
                }
            }
        } label: {
            ZStack {
                if !mask && layer["group"].bool {
                    SharedIcon(name: layer["collapsed"].bool ? "folder" : "folder-open", size: 28)
                    if layer["pass_through"].bool {
                        SharedIcon(name: "layer-group-pass-through-symbolic", size: 12)
                            .padding(1).background(palette["input"], in: SquircleShape(2))
                            .frame(width: 28, height: 28, alignment: .bottomTrailing)
                            .offset(x: -3, y: -3)
                    }
                }
                else {
                    if mask || layer["has_thumbnail"].bool,
                       let image = previews.images[LayerThumbnails.key(id, mask)] {
                        Image(decorative: image, scale: 1).resizable().frame(width: 28, height: 28)
                            .clipShape(SquircleShape.tile)
                            .opacity(mask && !layer["mask_enabled"].bool ? 0.4 : 1)
                    }
                    if !mask && !layer["content_icon"].isNull && !layer["selection_layer"].bool {
                        SharedIcon(name: layer["content_icon"].string, size: layer["has_thumbnail"].bool ? 12 : layer["adjustment_effect"].bool ? 24 : 28)
                            .foregroundStyle(palette["text"])
                            .padding(layer["has_thumbnail"].bool ? 1 : 0)
                            .background(layer["has_thumbnail"].bool ? palette["input"] : Color.clear, in: SquircleShape(2))
                            .frame(width: 28, height: 28, alignment: layer["has_thumbnail"].bool ? .bottomTrailing : .center)
                            .offset(x: layer["has_thumbnail"].bool ? -3 : 0, y: layer["has_thumbnail"].bool ? -3 : 0)
                            .allowsHitTesting(false).accessibilityHidden(true)
                    }
                }
            }.frame(width: 30, height: 30)
                .overlay {
                    thumbnailEdge(editing ? palette["accent"] : palette["text"].opacity(0.1))
                    LayerThumbnailOutline(progress: editing ? 1 : 0, color: palette["accent"])
                        .animation(reduceMotion ? nil : .timingCurve(0.25, 0.46, 0.45, 0.94, duration: 0.2), value: editing)
                        .allowsHitTesting(false)
                }
                // Bound the hit region as well as the drawing. Without this,
                // iPad thumbnail hits can consume the adjacent checkbox tap.
                .contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityLabel(label)
            .modifier(ActionHelp(store: store, label: label, action: JSON(["type": "layer", "action": target])))
            .accessibilityIdentifier("layer-thumbnail-\(id)-\(mask ? "mask" : "content")")
            .accessibilityValue(thumbnailCaptureStatus(mask: mask))
            .accessibilityAddTraits((mask ? layer["mask_selected"].bool : layer["content_selected"].bool) ? .isSelected : [])
            .modifier(LayerRowMeasurement(id: id, part: mask ? \.mask : \.content, enabled: !preview))
    }
    private func thumbnailEdge(_ color: Color) -> some View {
        Path { edge in
            let bounds = CGRect(x: 0, y: 0, width: 30, height: 30)
            edge.addPath(SquircleShape.tile.path(in: bounds))
            edge.addPath(SquircleShape.tile.inset(by: 1).path(in: bounds))
        }.fill(color, style: FillStyle(eoFill: true)).allowsHitTesting(false)
    }
    private func texture(_ paper: JSON) -> some View {
        let label = store.catalog["native_copy"]["paper"]["choose"].string
        return Button {
            perform {
                store.layer(["op": "select", "id": id, "mask": false])
                if paper["editable"].bool { store.papers.choose(layer: id) }
            }
        } label: {
            PaperPreview(previews: store.papers.previews, id: paper["id"].raw as? String, side: 28)
                .frame(width: 30, height: 30).overlay { thumbnailEdge(palette["text"].opacity(0.1)) }
                .contentShape(Rectangle())
        }.buttonStyle(.plain).help(label).accessibilityLabel(label).accessibilityValue(paper["label"].string)
            .accessibilityIdentifier("layer-texture-\(id)")
    }
    private func selectRow(toggle: Bool = false) {
        let keys = ThumbnailSelectionLoad.modifiers()
        perform { store.layer(["op": "select_row", "id": id, "extend": keys.shift, "toggle": toggle || keys.toggle]) }
    }
    private func perform(_ action: () -> Void) {
        guard !preview, interaction?.contact.consumeClick() != true else { return }
        action()
    }
    private func thumbnailCaptureStatus(mask: Bool) -> String {
        #if DEBUG
        if ProcessInfo.processInfo.environment["CAPY_CAPTURE_PROBE"] == "1" {
            let symbolic = !mask && !layer["has_thumbnail"].bool
            return symbolic || previews.images[LayerThumbnails.key(id, mask)] != nil ? "Preview ready" : "Preview pending"
        }
        #endif
        return ""
    }
}

private struct LayerThumbnailOutline: View, Animatable {
    private static let contour = SquircleShape.tile.path(in: CGRect(x: 0, y: 0, width: 38, height: 38))
    var progress: CGFloat
    let color: Color
    var animatableData: CGFloat {
        get { progress }
        set { progress = newValue }
    }
    var body: some View {
        Path { edge in
            for inset in [-4 + 3 * progress, -4 + 6 * progress] {
                let scale = (30 - 2 * inset) / 38
                edge.addPath(Self.contour, transform: CGAffineTransform(a: scale, b: 0, c: 0, d: scale, tx: inset, ty: inset))
            }
        }.fill(color.opacity(Double(progress)), style: FillStyle(eoFill: true))
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
    private var renaming: Bool { !preview && store.state["layer_tools"]["rename_layer"].optionalUInt == layer["id"].uint }
    var body: some View {
        Group {
            if renaming {
                TextField(store.catalog["native_copy"]["layers"]["name"].string, text: $name).textFieldStyle(.plain).focused($focused)
                    .onSubmit { if !NativeTextContext.composing { finish() } }.onKeyPress(.escape) { guard !NativeTextContext.composing else { return .ignored }; finish(cancel: true); return .handled }
            } else {
                Text(layer["label"].string).lineLimit(1).help(layer["label"].string)
                    .onTapGesture(count: 2) {
                        if allowsAction(), layer["can_rename"].bool { store.layer(["op": "begin_rename", "id": layer["id"].raw]) }
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
    var inline = true
    var body: some View {
        let layer = store.state["layer_tools"]["editing_layer"]
        let epoch = store.state["document_file"]["epoch"].uint
        let change: (Double, String?, @escaping @MainActor (String?) -> Void) -> Void = { value, phase, completion in
            store.effect(layer["id"].uint, epoch: epoch, key: "opacity",
                action: ["op": "layer_opacity", "opacity": value], phase: phase, completion: completion)
        }
        NumberControl(store: store, label: store.catalog["native_copy"]["layers"]["opacity"].string, value: layer["opacity"].number,
            control: store.catalog[inline ? "layer_opacity" : "opacity"], identifier: "layer-opacity", inline: inline,
            gestureChange: { change($1, $0, $2) }) { value, completion in
            change(value, nil, completion)
        }.id("\(epoch):\(layer["id"].uint)")
    }
}
