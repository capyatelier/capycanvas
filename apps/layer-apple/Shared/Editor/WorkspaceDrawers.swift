import SwiftUI

struct WorkspaceCollapsedColumns: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        ForEach(store.snapshot["layout"]["collapsed"].array, id: \.workspaceColumnID) { column in
            if !store.glass.panelColumnConnected(column["id"].uint) {
                ForEach(column["open"]["connections"].array.indices, id: \.self) { index in
                    let connection = column["open"]["connections"][index]
                    DrawerBridge(connection: connection[1]).fill(EditorPalette(source: store.state["palette"]).glassPanel)
                        .placed(connection[1]["bounds"]).allowsHitTesting(false).zIndex(159)
                        .modifier(GlassConnection(key: "column:\(column["id"].uint):\(connection[0].string)", connection: connection[1]))
                        .accessibilityIdentifier("column-connection-\(column["id"].uint)-\(connection[0].string)")
                }
            }
            WorkspaceCollapsedColumn(store: store, id: column["id"].uint)
                .environment(\.workspaceLayer, 160).zIndex(160)
        }
    }
}
private struct WorkspaceCollapsedColumn: View {
    @ObservedObject var store: EditorStore
    let id: UInt64
    var body: some View {
        if let column = store.snapshot["layout"]["collapsed"].array.first(where: { $0["id"].uint == id }) {
            WorkspaceCollapsedColumnContent(store: store, column: column, panels: column["groups"].array.map { group in
                group["icons"].array.map { store.panel($0["panel"].string) }
            }).placed(column["bounds"])
        }
    }
}
private struct WorkspaceCollapsedColumnContent: View {
    @ObservedObject var store: EditorStore
    let column: JSON
    let panels: [[JSON]]
    @StateObject private var scrollPosition: EditorScrollPosition
    @State private var pendingPresses: [String: UInt64] = [:]
    @State private var pressSequence: UInt64 = 0
    init(store: EditorStore, column: JSON, panels: [[JSON]]) {
        self.store = store; self.column = column; self.panels = panels
        let position = EditorScrollPosition()
        position.offset = store.state["workspace"]["layout"]["column_scroll"].array.first { $0[0].uint == column["id"].uint }?[1].number ?? 0
        _scrollPosition = StateObject(wrappedValue: position)
    }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var openSources: [DrawerSource] {
        guard !column["open"].isNull else { return [] }
        return column["groups"].array.flatMap { group in
            group["icons"].array.filter { $0["panel"].string == group["active"].string }
                .map { DrawerSource(direction: column["open"]["direction"].string, bounds: $0["bounds"].rect) }
        }
    }
    private var offset: CGFloat {
        store.state["workspace"]["layout"]["column_scroll"].array.first { $0[0].uint == column["id"].uint }?[1].number ?? 0
    }
    var body: some View {
        let base = column["bounds"], clip = column["content"]
        ZStack(alignment: .topLeading) {
            EditorPositionedScrollView(position: scrollPosition, offsetChanged: { next in
                if abs(next - offset) > 0.5 { store.dispatch(["type": "measure_column_scroll", "column": column["id"].raw, "offset": next]) }
            }) {
                let bottom = column["groups"].array.map { $0["bounds"].rect.maxY }.max() ?? clip.rect.minY
                ZStack(alignment: .topLeading) {
                    ForEach(column["groups"].array.indices, id: \.self) { index in
                        let group = column["groups"][index], panels = self.panels[index]

                        if index > 0 {
                            ToolbarDivider(vertical: true, color: palette["text"].opacity(0.3))
                                .placed(JSON(group["divider"].rect.offsetBy(dx: -clip.rect.minX, dy: -clip.rect.minY + offset)))
                                .allowsHitTesting(false).accessibilityHidden(true)
                        }
                        ForEach(group["icons"].array.indices, id: \.self) { index in
                            let icon = group["icons"][index]
                            let panel = panels[index]
                            let panelID = panel["id"].string
                            let columnID = column["id"].uint
                            let source = store.contentDrawers.sources[String(columnID)]
                            let presented = store.state["customization"]["column_drawers"].array.contains { drawer in
                                drawer["anchor"]["column"].uint == columnID && drawer["anchor"]["origin"].string == panelID
                            }
                            let selected = !column["open"].isNull ? group["active"].string == panelID : presented
                            let direction = !column["open"].isNull ? column["open"]["direction"].string : presented ? source?.direction : nil
                            IconTile(icon: panel["icon"].string, label: panel["title"].string, selected: selected,
                                pendingPress: pendingPresses[panelID] != nil,
                                joinedEdge: selected ? direction : nil, corner: .half) {
                                guard !store.workspace.input.contact.consumeClick() else { return }
                                pressSequence &+= 1
                                let sequence = pressSequence
                                pendingPresses[panelID] = sequence
                                store.edit(["type": "customize", "action": ["type": "toggle_column_drawer",
                                    "group": group["group"].raw, "panel": panel["id"].raw]]) { error in
                                    if let error { store.failure = error }
                                    if pendingPresses[panelID] == sequence { pendingPresses.removeValue(forKey: panelID) }
                                }
                            }.modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": panel["id"].raw]),
                                surface: .tile, context: JSON(["kind": "panel", "panel": panel["id"].raw])))
                                .modifier(PanelBridgeMeasurement(key: "column-icon:\(column["id"].uint):\(panel["id"].string)"))
                                .placed(JSON(icon["bounds"].rect.offsetBy(dx: -clip.rect.minX, dy: -clip.rect.minY + offset)))
                                .accessibilityIdentifier("column-icon-" + panel["id"].string)
                        }
                    }
                }.frame(width: clip.rect.width, height: max(clip.rect.height, bottom - clip.rect.minY + offset), alignment: .topLeading)
            }.onChange(of: offset) { _, next in
                if abs(scrollPosition.offset - next) > 0.5 { scrollPosition.scrollTo(y: next) }
            }.environment(\.workspaceClip, clip.rect)
                .scrollIndicators(.hidden)
                .modifier(PanelBridgeMeasurement(key: "column-content:\(column["id"].uint)"))
                .placed(clip.relative(to: base))
            PanelGrip(vertical: true).frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                .accessibilityElement().accessibilityHidden(false).accessibilityLabel(store.catalog["document_delivery_copy"]["move_column"].string)
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "column", "column": column["id"].raw]),
                    context: JSON(["kind": "column", "column": column["id"].raw]), openOnTap: true))
                .placed(column["grip"].relative(to: base)).accessibilityIdentifier("column-grip-\(column["id"].uint)")
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "column", "column": column["id"].raw]),
                surface: .tile, context: JSON(["kind": "column", "column": column["id"].raw]), canDrag: false, priority: -1))
            .environment(\.editorPalette, palette.glassy)
            .modifier(DrawerContainerSurface(drawers: store.contentDrawers, bounds: column["bounds"].rect,
                cuts: column["open"]["connections"].array.map { $0[1]["bounds"].rect }, fill: palette.glassPanel,
                shadow: 4, shadowOpacity: 0.16, extra: openSources,
                sharedRadii: store.glass.panelColumnRadii(column["id"].uint),
                gpuSurface: store.glass.panelColumnConnected(column["id"].uint), identifier: "collapsed-column-\(column["id"].uint)"))
            .modifier(PanelBridgeMeasurement(key: "column:\(column["id"].uint)"))
    }

}

struct WorkspaceContentDrawers: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var drawers: ContentDrawersPresentation
    var body: some View {
        ForEach(drawers.items.values.sorted { $0.id < $1.id }) { drawer in
            WorkspaceContentDrawer(store: store, drawer: drawer)
                .environment(\.workspaceLayer, drawer.id == "tool" ? 220 : 200)
                .zIndex(drawer.id == "tool" ? 220 : 200)
        }
    }
}
private struct WorkspaceContentDrawer: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var drawer: ContentDrawerPresentation
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        let placement = drawer.geometry["placement"], connection = drawer.geometry["connection"], tabs = drawer.model["tabs"]
#if os(macOS)
        let shadowCuts = connection.isNull ? [] : [connection["bounds"].rect, placement["anchor"].rect]
#else
        let shadowCuts = connection.isNull ? [] : [connection["bounds"].rect]
#endif
        let panels = tabs["panels"].array.map { drawer.panel($0) }
        if !placement.isNull {
            ZStack(alignment: .topLeading) {
                if !connection.isNull {
                    DrawerBridge(connection: connection).fill(palette.glassPanel).placed(connection["bounds"])
                        .modifier(GlassConnection(key: "drawer:" + drawer.id, connection: connection))
                }
                ZStack(alignment: .topLeading) {
                    ForEach(placement["columns"].array.indices, id: \.self) { index in
                        let bounds = placement["columns"][index]
                        VStack(spacing: 0) {
                            if !tabs.isNull {
                                WorkspacePanelHeader(store: store, group: tabs.replacing("id", with: tabs["group"]), panels: panels, drawer: true)
                            }
                            GeometryReader { clip in
                                if !drawer.model["layer_properties"].isNull {
                                    LayerInspector(store: store, drawer: drawer, width: bounds.rect.width, height: bounds.rect.height)
                                } else { EditorScrollView(.vertical, showsIndicators: false) {
                                    VStack(spacing: 0) {
                                        ForEach(drawer.model["columns"][index].array.indices, id: \.self) { row in
                                            let panel = drawer.panel(drawer.model["columns"][index][row])
                                            DrawerPanelBody(store: store, panel: panel, width: bounds.rect.width,
                                                splitFilters: drawer.model["columns"].array.contains { $0.array.contains { $0.string == "filter_types" } },
                                                toolSet: drawer.model["tool_set"])
                                        }
                                    }.frame(maxWidth: .infinity, alignment: .topLeading)
                                        .background(GeometryReader { body in
                                            Color.clear.preference(key: DrawerHeights.self, value: [index: body.size.height + (tabs.isNull ? 0 : 36)])
                                        })
                                }.environment(\.drawerColumn, UInt64(drawer.id))
                                    .environment(\.workspaceClip, clip.frame(in: .named("editor-workspace")))
                                }
                            }
                        }.placed(bounds)
                    }
                }.frame(width: placement["bounds"].rect.width, height: placement["bounds"].rect.height, alignment: .topLeading)
                    .environment(\.editorPalette, palette.glassy)
                    .modifier(DrawerContainerSurface(drawers: store.contentDrawers, bounds: placement["bounds"].rect,
                        cuts: shadowCuts,
                        fill: palette.glassPanel, shadow: 12, joined: connection["square_corners"], excluding: drawer.id,
                        identifier: drawer.id == "tool" ? "tool-drawer" : "column-drawer-" + drawer.id))
                    .modifier(NavigatorReveal())
                    .background(GeometryReader { body in
                        Color.clear.preference(key: ColumnDrawerMeasurements.self,
                            value: drawer.interactive && !drawer.model["tabs"].isNull
                                ? [drawer.model["tabs"]["group"].uint: body.frame(in: .named("editor-workspace"))] : [:])
                    })
                    .placed(placement["bounds"])
            }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                // This view observes the drawer itself. Reading interactive in
                // the parent list left a reopened drawer with stale disabled
                // gesture preferences when its identity survived the close.
                .environment(\.workspaceGesturesEnabled, drawer.interactive)
                .allowsHitTesting(drawer.interactive)
                .onPreferenceChange(DrawerHeights.self) { heights in for (column, height) in heights { drawer.measure(height, column: column) } }
        }
    }

}

struct LayerInspector<Owner: LayerInspectorOwner>: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ObservedObject var store: EditorStore
    @ObservedObject var drawer: Owner
    let width: CGFloat
    let height: CGFloat
    @State private var measurement = JSON(["fixed_height": 0, "unit_height": 40])
    @State private var propertiesHeight: CGFloat = 0
    @State private var footerHover = false
    private var open: Bool { drawer.propertiesOpen }
    private var footerHeight: CGFloat { drawer.footerHeight }
    private var sizingKey: String { JSON([measurement.raw, propertiesHeight, width, store.state["layers"].array.count, store.state["camera"]["viewport"].raw, drawer.sizingContext]).stableKey }
    var body: some View {
        let view = store.state["layer_properties"], disclosure = view["disclosure"]
        let label = drawer.disclosure ?? disclosure[open ? "return_label" : "label"].string
        let summary = open ? "" : disclosure["summary"].string
        let pageHeight = max(0, height - footerHeight), palette = EditorPalette(source: store.state["palette"])
        let inset = drawer.simple ? store.panelMetrics.inset : store.panelMetrics.compound
        VStack(spacing: 0) {
            ZStack(alignment: .topLeading) {
                LayerPanel(store: store, panel: drawer.panel(JSON("layers")), drawerPagePosition: drawer.pagePosition, simple: drawer.simple)
                    .environment(\.measuresWorkspacePanel, true)
                    .frame(width: width, height: max(0, pageHeight - inset * 2, measurement["fixed_height"].number + measurement["unit_height"].number - inset * 2)).padding(.vertical, inset)
                    .offset(y: -drawer.pagePosition * pageHeight).allowsHitTesting(!open).accessibilityHidden(open)
                    .onPreferenceChange(PanelSizeFacts.self) { facts in
                        let fixed = facts.filter { $0.key.panel == "layers" && $0.key.kind == .fixed }.values.reduce(inset * 2, +)
                        let unit = facts.filter { $0.key.panel == "layers" && $0.key.kind == .unit }.values.max() ?? 40
                        let next = JSON(["fixed_height": fixed, "unit_height": unit])
                        if next.stableKey != measurement.stableKey { measurement = next }
                    }
                EditorScrollView(.vertical) {
                    LayerPropertiesPanel(store: store).padding(8)
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { propertiesHeight = $0 }
                }.frame(width: width, height: pageHeight).offset(y: (1 - drawer.pagePosition) * pageHeight)
                    .allowsHitTesting(open).accessibilityHidden(!open)
            }.frame(height: pageHeight, alignment: .topLeading).contentShape(Rectangle()).clipped()
                .accessibilityElement(children: .contain).accessibilityIdentifier("layer-properties-pages")
            Rectangle().fill(palette["text"].opacity(0.16)).frame(height: 1).accessibilityHidden(true)
            HStack(spacing: drawer.simple ? store.panelMetrics.group : 0) {
            if drawer.simple {
                IconTile(icon: "add-layer", label: store.catalog["native_copy"]["layers"]["new_layer"].string) {
                    store.layer(["op": "new", "group": false, "clipped": false])
                }
                    .frame(width: store.panelMetrics.height + 2 * layerEdgeInset, height: footerHeight - 1).opacity(open ? 0 : 1).disabled(open).accessibilityHidden(open)
                    .accessibilityIdentifier("bubble-new-layer")
            }
            Button { drawer.toggleProperties() } label: {
                HStack(spacing: 6) {
                    if !drawer.simple { SharedIcon(name: "properties").frame(width: 16, height: 16).frame(width: store.panelMetrics.height, height: store.panelMetrics.height) }
                    VStack(alignment: drawer.simple ? .trailing : .leading, spacing: 1) {
                        Text(label).lineLimit(1).accessibilityIdentifier("layer-properties-disclosure-label")
                        if !drawer.simple && !summary.isEmpty { Text(summary).font(.system(size: store.catalog["text_size_pt"].number * 4 / 3 * 0.9)).foregroundStyle(palette["text"].opacity(0.55)).lineLimit(1) }
                    }.frame(maxWidth: .infinity, alignment: drawer.simple ? .trailing : .leading)
                    SharedIcon(name: "chevron-down").frame(width: 16, height: 16).rotationEffect(.degrees(open ? 0 : 180))
                        .frame(width: store.panelMetrics.height, height: store.panelMetrics.height)
                }.padding(.horizontal, layerEdgeInset).frame(height: footerHeight - 1).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle(active: footerHover, joinedEdge: "top"))
                .onHover { footerHover = $0 }.disabled(!open && view["layer"].isNull).help([label, summary].filter { !$0.isEmpty }.joined(separator: "\n"))
                .accessibilityLabel(label)
                .accessibilityIdentifier("layer-properties-disclosure")
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .onChange(of: reduceMotion, initial: true) { _, value in drawer.reduceMotion = value }
            .task(id: sizingKey) {
                let result: JSON = await withCheckedContinuation { c in
                    store.query(["type": "layer_drawer_sizing", "measurement": measurement.raw, "properties_height": propertiesHeight]) { c.resume(returning: $0) }
                }
                if !Task.isCancelled { drawer.measureLayerPages(result) }
            }
    }
}

private struct DrawerPanelBody: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let width: CGFloat
    let splitFilters: Bool
    let toolSet: JSON
    @State private var tiles = JSON()
    private var tileKey: String { JSON([panel["id"].raw, panel["tiles"].array.map { [$0["id"].raw, $0["control"].raw, $0["slider_style"].raw] }, panel["tile_style"].raw, width]).stableKey }
    var body: some View {
        Group {
            if !panel["tiles"].array.isEmpty {
                WorkspaceToolbar(store: store, panel: panel, geometry: tiles, vertical: true)
                    .frame(height: tiles["content_height"].number)
                    .task(id: tileKey) {
                        let result: JSON = await withCheckedContinuation { c in
                            store.query(["type": "drawer_toolbar", "panel": panel["id"].raw, "width": max(1, width), "height": 800]) { c.resume(returning: $0) }
                        }
                        if !Task.isCancelled { tiles = result }
                    }
            } else {
                PanelControls(store: store, panel: panel, scrollable: false, measureForWorkspace: false, splitFilters: splitFilters,
                    drawerToolSet: toolSet)
                    .frame(height: ["layers", "adjustments", "filter_types"].contains(panel["id"].string) ? 480 : panel["id"].string == "navigator" ? 240 : nil)
            }
        }
    }
}

private struct DrawerHeights: PreferenceKey {
    static var defaultValue: [Int: CGFloat] { [:] }
    static func reduce(value: inout [Int: CGFloat], nextValue: () -> [Int: CGFloat]) { value.merge(nextValue(), uniquingKeysWith: max) }
}
private struct DrawerContainerSurface: ViewModifier {
    @ObservedObject var drawers: ContentDrawersPresentation
    let bounds: CGRect
    var cuts: [CGRect] = []
    let fill: Color
    let shadow: CGFloat
    var shadowOpacity = 0.22
    var joined = JSON()
    var excluding: String?
    var extra: [DrawerSource] = []
    var sharedRadii: [CGFloat]? = nil
    var gpuSurface = false
    let identifier: String
    func body(content: Content) -> some View {
        let sources = drawers.sources.filter { $0.key != excluding }.map(\.value) + extra
        let shape = sharedRadii.map { SquircleShape(topLeading: $0[0], topTrailing: $0[1], bottomTrailing: $0[2], bottomLeading: $0[3]) }
            ?? SquircleShape(SquircleShape.surfaceRadius,
                square: DrawerSource.square(bounds, radius: SquircleShape.surfaceRadius, sources: sources, joined: joined))
        ZStack(alignment: .topLeading) {
            if !gpuSurface {
                OutsideShadow(shape: shape, opacity: shadowOpacity, radius: shadow, y: 2,
                    cuts: cuts.map { $0.offsetBy(dx: -bounds.minX, dy: -bounds.minY) })
            }
            content.clipShape(shape.fittedClip)
                .background { if !gpuSurface { shape.fill(fill) } }
                .modifier(GlassRegistration(shape: shape, active: !gpuSurface))
                .accessibilityElement(children: .contain).accessibilityIdentifier(identifier)
        }
    }
}
private struct DrawerBridge: Shape {
    let connection: JSON
    func path(in rect: CGRect) -> Path {
        let l = connection["length"].number, d = connection["depth"].number
        let a = connection["radii"][0].number, b = connection["radii"][1].number
        var p = Path()
        p.move(to: .zero); p.addLine(to: CGPoint(x: l, y: 0)); p.addLine(to: CGPoint(x: l, y: d - b))
        p.squircle(center: CGPoint(x: l + b, y: d - b), start: CGVector(dx: -b, dy: 0), end: CGVector(dx: 0, dy: b))
        p.addLine(to: CGPoint(x: -a, y: d))
        p.squircle(center: CGPoint(x: -a, y: d - a), start: CGVector(dx: 0, dy: a), end: CGVector(dx: a, dy: 0))
        p.closeSubpath()
        let t = connection["transform"]
        return p.applying(CGAffineTransform(a: t[0].number, b: t[1].number, c: t[2].number, d: t[3].number, tx: t[4].number, ty: t[5].number))
    }
}
private extension JSON { var workspaceColumnID: UInt64 { self["id"].uint } }
