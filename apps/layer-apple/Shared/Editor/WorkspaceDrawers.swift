import SwiftUI

struct WorkspaceZenToolbars: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        ForEach(store.snapshot["zen_toolbars"]["sections"].array.indices, id: \.self) { index in
            let section = store.snapshot["zen_toolbars"]["sections"][index]
            let panel = store.snapshot["panels"].array.first { $0["id"].string == section["panel"].string } ?? JSON()
            let tiles = section["tiles"].array.compactMap { pair in panel["tiles"].array.first { $0["id"].uint == pair[0].uint }?.raw }
            WorkspaceToolbar(store: store,
                panel: panel.replacing("tiles", with: JSON(tiles)).replacing("tile_style", with: section["style"]),
                geometry: JSON(["tiles": section["tiles"].array.map { $0[1].raw }]))
                .background(EditorPalette(source: store.state["palette"])["panel"], in: RoundedRectangle(cornerRadius: 6))
                .shadow(color: .black.opacity(0.22), radius: 6, y: 2)
                .placed(section["bounds"]).zIndex(150).environment(\.workspaceGesturesEnabled, false)
                .accessibilityIdentifier("zen-toolbar-\(index)")
        }
    }
}

struct WorkspaceCollapsedColumns: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        ForEach(store.snapshot["layout"]["collapsed"].array, id: \.workspaceColumnID) { column in
            WorkspaceCollapsedColumn(store: store, column: column).placed(column["bounds"])
                .environment(\.workspaceLayer, 160).zIndex(160)
        }
    }
}
private struct WorkspaceCollapsedColumn: View {
    @ObservedObject var store: EditorStore
    let column: JSON
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var offset: CGFloat {
        store.state["workspace"]["layout"]["column_scroll"].array.first { $0[0].uint == column["id"].uint }?[1].number ?? 0
    }
    var body: some View {
        let base = column["bounds"], clip = column["content"]
        ZStack(alignment: .topLeading) {
            IconTile(icon: "column-expand", label: "Expand column") {
                store.customize(["type": "set_column_collapsed", "group": column["id"].raw, "collapsed": false])
            }.placed(column["expand"].relative(to: base)).accessibilityIdentifier("expand-column-\(column["id"].uint)")
            ScrollView(.vertical) {
                let bottom = column["groups"].array.map { $0["bounds"].rect.maxY }.max() ?? clip.rect.minY
                ZStack(alignment: .topLeading) {
                    ForEach(column["groups"].array.indices, id: \.self) { index in
                        let group = column["groups"][index]
                        ForEach(group["icons"].array.indices, id: \.self) { index in
                            let icon = group["icons"][index]
                            let panel = store.snapshot["panels"].array.first { $0["id"].string == icon["panel"].string } ?? JSON()
                            IconTile(icon: panel["icon"].string, label: panel["title"].string, selected: group["active"].string == panel["id"].string) {
                                store.customize(["type": "toggle_column_drawer", "group": group["group"].raw, "panel": panel["id"].raw])
                            }.modifier(WorkspaceContext(store: store, target: JSON(["kind": "panel", "panel": panel["id"].raw])))
                                .placed(JSON(icon["bounds"].rect.offsetBy(dx: -clip.rect.minX, dy: -clip.rect.minY + offset)))
                                .accessibilityIdentifier("column-icon-" + panel["id"].string)
                        }
                    }
                }.frame(width: clip.rect.width, height: max(clip.rect.height, bottom - clip.rect.minY + offset), alignment: .topLeading)
            }.scrollIndicators(.hidden).placed(clip.relative(to: base))
                .onScrollGeometryChange(for: CGFloat.self) { max(0, $0.contentOffset.y + $0.contentInsets.top) } action: { _, next in
                    if abs(next - offset) > 0.5 { store.dispatch(["type": "measure_column_scroll", "column": column["id"].raw, "offset": next]) }
                }
            SharedIcon(name: "grip").frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                .accessibilityElement().accessibilityHidden(false).accessibilityLabel("Move column")
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "column", "column": column["id"].raw])))
                .placed(column["grip"].relative(to: base)).accessibilityIdentifier("column-grip-\(column["id"].uint)")
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(palette["panel"]).clipShape(RoundedRectangle(cornerRadius: 8))
            .shadow(color: .black.opacity(0.22), radius: 6, y: 2)
            .accessibilityElement(children: .contain).accessibilityIdentifier("collapsed-column-\(column["id"].uint)")
    }
}

struct WorkspaceContentDrawers: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var drawers: ContentDrawersPresentation
    var body: some View {
        ForEach(drawers.items.values.sorted { $0.id < $1.id }) { drawer in
            WorkspaceContentDrawer(store: store, drawer: drawer)
                .environment(\.workspaceLayer, drawer.id == "tool" ? 220 : 200)
                .environment(\.workspaceGesturesEnabled, !store.snapshot["partial_zen"].bool)
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
                if !connection.isNull { DrawerBridge(connection: connection).fill(palette["panel"]).placed(connection["bounds"]) }
                ZStack(alignment: .topLeading) {
                    ForEach(placement["columns"].array.indices, id: \.self) { index in
                        let bounds = placement["columns"][index]
                        VStack(spacing: 0) {
                            let tabs = drawer.model["tabs"]
                            if !tabs.isNull {
                                ScrollView(.horizontal) {
                                    HStack(spacing: 0) {
                                        ForEach(tabs["panels"].array.indices, id: \.self) { i in
                                            let panel = drawer.panel(tabs["panels"][i])
                                            Button { store.dispatch(["type": "select_panel_tab", "group": tabs["group"].raw, "panel": panel["id"].raw]) } label: {
                                                HStack(spacing: 6) {
                                                    SharedIcon(name: panel["icon"].string)
                                                    if panel["id"].string == tabs["active"].string { Text(panel["title"].string).fontWeight(.bold) }
                                                }.padding(.horizontal, 8).frame(height: 36).background(panel["id"].string == tabs["active"].string ? palette["panel"] : Color.clear)
                                            }.buttonStyle(.plain).accessibilityIdentifier("drawer-tab-" + panel["id"].string)
                                        }
                                    }
                                }.scrollIndicators(.hidden).frame(height: 36).background(palette["tabbar"])
                            }
                            GeometryReader { clip in
                                ScrollView(.vertical) {
                                    VStack(spacing: 6) {
                                        ForEach(drawer.model["columns"][index].array.indices, id: \.self) { row in
                                            let panel = drawer.panel(drawer.model["columns"][index][row])
                                            DrawerPanelBody(store: store, panel: panel, width: bounds.rect.width)
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
                    .background(palette["panel"]).clipShape(DrawerBodyShape(corners: connection["square_corners"]))
                    .shadow(color: .black.opacity(0.22), radius: 12, y: 2)
                    .modifier(NavigatorReveal())
                    .placed(placement["bounds"])
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier(drawer.id == "tool" ? "tool-drawer" : "column-drawer-" + drawer.id)
            }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .onPreferenceChange(DrawerHeights.self) { heights in for (column, height) in heights { drawer.measure(height, column: column) } }
        }
    }
}
private struct DrawerPanelBody: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let width: CGFloat
    @State private var tiles = JSON()
    private var tileKey: String { JSON([panel["id"].raw, panel["tiles"].array.map { [$0["id"].raw, $0["control"].raw] }, panel["tile_style"].raw, width]).stableKey }
    var body: some View {
        Group {
            if !panel["tiles"].array.isEmpty {
                WorkspaceToolbar(store: store, panel: panel, geometry: tiles)
                    .frame(height: tiles["content_height"].number)
                    .task(id: tileKey) {
                        let result: JSON = await withCheckedContinuation { c in
                            store.query(["type": "drawer_toolbar", "panel": panel["id"].raw, "width": max(1, width), "height": 800]) { c.resume(returning: $0) }
                        }
                        if !Task.isCancelled { tiles = result }
                    }
            } else {
                PanelControls(store: store, panel: panel, scrollable: false)
                    .frame(height: panel["id"].string == "layers" || panel["id"].string == "adjustments" ? 480 : panel["id"].string == "navigator" ? 240 : nil)
            }
        }
    }
}

private struct DrawerHeights: PreferenceKey {
    static var defaultValue: [Int: CGFloat] { [:] }
    static func reduce(value: inout [Int: CGFloat], nextValue: () -> [Int: CGFloat]) { value.merge(nextValue(), uniquingKeysWith: max) }
}
private struct DrawerBodyShape: Shape {
    let corners: JSON
    func path(in rect: CGRect) -> Path {
        UnevenRoundedRectangle(topLeadingRadius: corners[0].bool ? 0 : 8, bottomLeadingRadius: corners[3].bool ? 0 : 8,
            bottomTrailingRadius: corners[2].bool ? 0 : 8, topTrailingRadius: corners[1].bool ? 0 : 8).path(in: rect)
    }
}
private struct DrawerBridge: Shape {
    let connection: JSON
    func path(in rect: CGRect) -> Path {
        let length = connection["length"].number, depth = connection["depth"].number
        let r0 = connection["radii"][0].number, r1 = connection["radii"][1].number, k = 0.5522848
        var p = Path()
        p.move(to: .zero); p.addLine(to: CGPoint(x: length, y: 0)); p.addLine(to: CGPoint(x: length, y: depth - r1))
        p.addCurve(to: CGPoint(x: length + r1, y: depth), control1: CGPoint(x: length, y: depth - r1 + r1 * k), control2: CGPoint(x: length + r1 - r1 * k, y: depth))
        p.addLine(to: CGPoint(x: -r0, y: depth))
        p.addCurve(to: CGPoint(x: 0, y: depth - r0), control1: CGPoint(x: -r0 + r0 * k, y: depth), control2: CGPoint(x: 0, y: depth - r0 + r0 * k))
        p.closeSubpath()
        let t = connection["transform"]
        return p.applying(CGAffineTransform(a: t[0].number, b: t[1].number, c: t[2].number, d: t[3].number, tx: t[4].number, ty: t[5].number))
    }
}
private extension JSON { var workspaceColumnID: UInt64 { self["id"].uint } }
