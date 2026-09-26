import SwiftUI

struct WorkspaceCollapsedColumns: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        ForEach(store.snapshot["layout"]["collapsed"].array, id: \.workspaceColumnID) { column in
            ForEach(column["open"]["connections"].array.indices, id: \.self) { index in
                let connection = column["open"]["connections"][index]
                DrawerBridge(connection: connection[1]).fill(EditorPalette(source: store.state["palette"]).glassPanel)
                    .placed(connection[1]["bounds"]).allowsHitTesting(false).zIndex(159)
                    .modifier(GlassConnection(key: "column:\(column["id"].uint):\(connection[0].string)", connection: connection[1]))
                    .accessibilityIdentifier("column-connection-\(column["id"].uint)-\(connection[0].string)")
            }
            WorkspaceCollapsedColumn(store: store, column: column).placed(column["bounds"])
                .environment(\.workspaceLayer, 160).zIndex(160)
        }
    }
}
private struct WorkspaceCollapsedColumn: View {
    @ObservedObject var store: EditorStore
    let column: JSON
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
            EditorScrollView(.vertical) {
                let bottom = column["groups"].array.map { $0["bounds"].rect.maxY }.max() ?? clip.rect.minY
                ZStack(alignment: .topLeading) {
                    ForEach(column["groups"].array.indices, id: \.self) { index in
                        let group = column["groups"][index]

                        if index > 0 {
                            Rectangle().fill(palette["text"].opacity(0.3))
                                .placed(JSON(group["divider"].rect.offsetBy(dx: -clip.rect.minX, dy: -clip.rect.minY + offset)))
                                .allowsHitTesting(false).accessibilityHidden(true)
                        }
                        ForEach(group["icons"].array.indices, id: \.self) { index in
                            let icon = group["icons"][index]
                            let panel = store.panel(icon["panel"].string)
                            let source = store.contentDrawers.sources[String(column["id"].uint)]
                            let presented = source?.anchor["origin"].string == panel["id"].string
                            let selected = !column["open"].isNull ? group["active"].string == panel["id"].string : presented
                            let direction = !column["open"].isNull ? column["open"]["direction"].string : presented ? source?.direction : nil
                            IconTile(icon: panel["icon"].string, label: panel["title"].string, selected: selected,
                                joinedEdge: selected ? direction : nil, corner: .half) {
                                guard !store.workspace.input.contact.consumeClick() else { return }
                                store.customize(["type": "toggle_column_drawer", "group": group["group"].raw, "panel": panel["id"].raw])
                            }.modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": panel["id"].raw]),
                                surface: .tile, context: JSON(["kind": "panel", "panel": panel["id"].raw])))
                                .placed(JSON(icon["bounds"].rect.offsetBy(dx: -clip.rect.minX, dy: -clip.rect.minY + offset)))
                                .accessibilityIdentifier("column-icon-" + panel["id"].string)
                        }
                    }
                }.frame(width: clip.rect.width, height: max(clip.rect.height, bottom - clip.rect.minY + offset), alignment: .topLeading)
            }.environment(\.workspaceClip, clip.rect)
                .scrollIndicators(.hidden).placed(clip.relative(to: base))
                .onScrollGeometryChange(for: CGFloat.self) { max(0, $0.contentOffset.y + $0.contentInsets.top) } action: { _, next in
                    if abs(next - offset) > 0.5 { store.dispatch(["type": "measure_column_scroll", "column": column["id"].raw, "offset": next]) }
                }
            PanelGrip(vertical: true).frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                .accessibilityElement().accessibilityHidden(false).accessibilityLabel("Move column")
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "column", "column": column["id"].raw]),
                    context: JSON(["kind": "column", "column": column["id"].raw]), openOnTap: true))
                .placed(column["grip"].relative(to: base)).accessibilityIdentifier("column-grip-\(column["id"].uint)")
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .environment(\.editorPalette, palette.glassy)
            .modifier(DrawerContainerSurface(drawers: store.contentDrawers, bounds: column["bounds"].rect,
                cuts: column["open"]["connections"].array.map { $0[1]["bounds"].rect }, fill: palette.glassPanel,
                shadow: 4, shadowOpacity: 0.16, extra: openSources, identifier: "collapsed-column-\(column["id"].uint)"))
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
        let placement = drawer.geometry["placement"], connection = drawer.geometry["connection"]
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
                            let tabs = drawer.model["tabs"]
                            if !tabs.isNull {
                                WorkspacePanelHeader(store: store, group: tabs.replacing("id", with: tabs["group"]), drawer: true)
                            }
                            GeometryReader { clip in
                                EditorScrollView(.vertical, showsIndicators: false) {
                                    VStack(spacing: 6) {
                                        ForEach(drawer.model["columns"][index].array.indices, id: \.self) { row in
                                            let panel = drawer.panel(drawer.model["columns"][index][row])
                                            DrawerPanelBody(store: store, panel: panel, width: bounds.rect.width,
                                                splitFilters: drawer.model["columns"].array.contains { $0.array.contains { $0.string == "filter_types" } })
                                        }
                                    }.frame(maxWidth: .infinity, alignment: .topLeading)
                                        .background(GeometryReader { body in
                                            Color.clear.preference(key: DrawerHeights.self, value: [index: body.size.height + (tabs.isNull ? 0 : 36)])
                                        })
                                }.environment(\.drawerColumn, UInt64(drawer.id))
                                    .environment(\.workspaceClip, clip.frame(in: .named("editor-workspace")))
                            }
                        }.placed(bounds)
                    }
                }.frame(width: placement["bounds"].rect.width, height: placement["bounds"].rect.height, alignment: .topLeading)
                    .environment(\.editorPalette, palette.glassy)
                    .modifier(DrawerContainerSurface(drawers: store.contentDrawers, bounds: placement["bounds"].rect,
                        cuts: connection.isNull ? [] : [connection["bounds"].rect],
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

private struct DrawerPanelBody: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let width: CGFloat
    let splitFilters: Bool
    @State private var tiles = JSON()
    private var tileKey: String { JSON([panel["id"].raw, panel["tiles"].array.map { [$0["id"].raw, $0["control"].raw] }, panel["tile_style"].raw, width]).stableKey }
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
                PanelControls(store: store, panel: panel, scrollable: false, measureForWorkspace: false, splitFilters: splitFilters)
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
    let identifier: String
    func body(content: Content) -> some View {
        let sources = drawers.sources.filter { $0.key != excluding }.map(\.value) + extra
        let shape = SquircleShape(SquircleShape.surfaceRadius,
            square: DrawerSource.square(bounds, radius: SquircleShape.surfaceRadius, sources: sources, joined: joined))
        ZStack(alignment: .topLeading) {
            OutsideShadow(shape: shape, opacity: shadowOpacity, radius: shadow, y: 2,
                cuts: cuts.map { $0.offsetBy(dx: -bounds.minX, dy: -bounds.minY) })
            content.clipShape(shape.fittedClip)
                .background { shape.fill(fill) }
                .modifier(GlassRegistration(shape: shape))
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
