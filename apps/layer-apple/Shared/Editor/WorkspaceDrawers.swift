import SwiftUI

struct WorkspaceZenToolbars: View {
    @ObservedObject var store: EditorStore
    var body: some View {
        ForEach(store.snapshot["zen_toolbars"]["sections"].array.indices, id: \.self) { index in
            let section = store.snapshot["zen_toolbars"]["sections"][index]
            let panel = store.panel(section["panel"].string)
            let tiles = section["tiles"].array.compactMap { pair in panel["tiles"].array.first { $0["id"].uint == pair[0].uint }?.raw }
            WorkspaceToolbar(store: store,
                panel: panel.replacing("tiles", with: JSON(tiles)).replacing("tile_style", with: section["style"]),
                geometry: JSON(["tiles": section["tiles"].array.map { $0[1].raw }]),
                vertical: ["left", "right"].contains(section["edge"].string))
                .background(EditorPalette(source: store.state["palette"])["panel"], in: RoundedRectangle(cornerRadius: 6))
                .shadow(color: .black.opacity(0.22), radius: 6, y: 2)
                .placed(section["bounds"]).zIndex(150).environment(\.workspaceGesturesEnabled, false)
                .accessibilityElement(children: .contain).accessibilityIdentifier("zen-toolbar-\(index)")
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
    private var attachedPanel: JSON { column["group_panel"] }
    private var offset: CGFloat {
        store.state["workspace"]["layout"]["column_scroll"].array.first { $0[0].uint == column["id"].uint }?[1].number ?? 0
    }
    var body: some View {
        let base = column["bounds"], clip = column["content"]
        ZStack(alignment: .topLeading) {
            Button {
                store.customize(["type": "set_column_collapsed", "group": column["id"].raw, "collapsed": false])
            } label: {
                SharedIcon(name: base.rect.midX < store.snapshot["layout"]["work_area"].rect.midX
                    ? "chevron-double-right" : "chevron-double-left")
                    .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
            }.buttonStyle(EditorControlButtonStyle()).accessibilityLabel("Expand column").help("Expand column")
                .placed(column["expand"].relative(to: base)).accessibilityIdentifier("expand-column-\(column["id"].uint)")
            ScrollView(.vertical) {
                let bottom = column["groups"].array.map { $0["bounds"].rect.maxY }.max() ?? clip.rect.minY
                ZStack(alignment: .topLeading) {
                    ForEach(column["groups"].array.indices, id: \.self) { index in
                        let group = column["groups"][index]
                        if !attachedPanel.isNull && attachedPanel["group"].uint == group["group"].uint {
                            DrawerBodyShape(corners: stripCorners, radius: 6).fill(palette["panel"])
                                .placed(JSON(group["bounds"].rect.offsetBy(dx: -clip.rect.minX, dy: -clip.rect.minY + offset)))
                                .allowsHitTesting(false).accessibilityHidden(true)
                        }
                        Rectangle().fill(palette["text"].opacity(0.3))
                            .placed(JSON(group["divider"].rect.offsetBy(dx: -clip.rect.minX, dy: -clip.rect.minY + offset)))
                            .allowsHitTesting(false).accessibilityHidden(true)
                        ForEach(group["icons"].array.indices, id: \.self) { index in
                            let icon = group["icons"][index]
                            let panel = store.panel(icon["panel"].string)
                            let selected = attachedPanel.isNull && store.state["customization"]["column_drawers"].array.contains {
                                $0["anchor"]["column"].uint == column["id"].uint && $0["anchor"]["origin"].string == panel["id"].string
                            }
                            IconTile(icon: panel["icon"].string, label: panel["title"].string, selected: selected) {
                                guard !store.workspace.input.contact.consumeClick() else { return }
                                store.customize(["type": "toggle_column_drawer", "group": group["group"].raw, "panel": panel["id"].raw])
                            }.modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": panel["id"].raw]),
                                surface: .tile, context: JSON(["kind": "panel", "panel": panel["id"].raw])))
                                .placed(JSON(icon["bounds"].rect.offsetBy(dx: -clip.rect.minX, dy: -clip.rect.minY + offset)))
                                .accessibilityIdentifier("column-icon-" + panel["id"].string)
                                .accessibilityAddTraits(selected ? .isSelected : [])
                        }
                    }
                }.frame(width: clip.rect.width, height: max(clip.rect.height, bottom - clip.rect.minY + offset), alignment: .topLeading)
            }.environment(\.workspaceClip, clip.rect)
                .scrollIndicators(.hidden).placed(clip.relative(to: base))
                .onScrollGeometryChange(for: CGFloat.self) { max(0, $0.contentOffset.y + $0.contentInsets.top) } action: { _, next in
                    if abs(next - offset) > 0.5 { store.dispatch(["type": "measure_column_scroll", "column": column["id"].raw, "offset": next]) }
                }
            SharedIcon(name: "grip").frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                .accessibilityElement().accessibilityHidden(false).accessibilityLabel("Move column")
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "column", "column": column["id"].raw])))
                .placed(column["grip"].relative(to: base)).accessibilityIdentifier("column-grip-\(column["id"].uint)")
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(palette[attachedPanel.isNull ? "panel" : "tabbar"]).clipShape(DrawerBodyShape(corners: stripCorners))
            .shadow(color: .black.opacity(0.22), radius: 6, y: 2)
            .accessibilityElement(children: .contain).accessibilityIdentifier("collapsed-column-\(column["id"].uint)")
    }
    private var stripCorners: JSON {
        switch attachedPanel["direction"].string {
        case "right": JSON([false, true, true, false])
        case "left": JSON([true, false, false, true])
        default: JSON()
        }
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
                if !connection.isNull { DrawerBridge(connection: connection).fill(palette["panel"]).placed(connection["bounds"]) }
                ZStack(alignment: .topLeading) {
                    ForEach(placement["columns"].array.indices, id: \.self) { index in
                        let bounds = placement["columns"][index]
                        VStack(spacing: 0) {
                            let tabs = drawer.model["tabs"]
                            if !tabs.isNull {
                                WorkspacePanelHeader(store: store, group: tabs.replacing("id", with: tabs["group"]), drawer: true)
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
                    if drawer.isGroupPanel && !store.snapshot["partial_zen"].bool {
                        WorkspaceColumnPanelHandles(store: store, column: drawer.model["anchor"]["column"], bounds: placement["bounds"])
                    }
                }.frame(width: placement["bounds"].rect.width, height: placement["bounds"].rect.height, alignment: .topLeading)
                    .background(palette["panel"]).clipShape(DrawerBodyShape(corners: corners(placement, connection)))
                    .shadow(color: .black.opacity(0.22), radius: 12, y: 2)
                    .modifier(NavigatorReveal())
                    .background(GeometryReader { body in
                        Color.clear.preference(key: ColumnDrawerMeasurements.self,
                            value: drawer.interactive && !drawer.model["tabs"].isNull && !store.snapshot["partial_zen"].bool
                                ? [drawer.model["tabs"]["group"].uint: body.frame(in: .named("editor-workspace"))] : [:])
                    })
                    .placed(placement["bounds"])
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier(drawer.id == "tool" ? "tool-drawer" : "column-drawer-" + drawer.id)
            }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                // This view observes the drawer itself. Reading interactive in
                // the parent list left a reopened drawer with stale disabled
                // gesture preferences when its identity survived the close.
                .environment(\.workspaceGesturesEnabled, drawer.interactive && !store.snapshot["partial_zen"].bool)
                .allowsHitTesting(drawer.interactive)
                .onPreferenceChange(DrawerHeights.self) { heights in for (column, height) in heights { drawer.measure(height, column: column) } }
        }
    }
    private func corners(_ placement: JSON, _ connection: JSON) -> JSON {
        guard drawer.isGroupPanel && !store.snapshot["partial_zen"].bool else { return connection["square_corners"] }
        return placement["direction"].string == "right" ? JSON([true, false, false, true]) : JSON([false, true, true, false])
    }
}

private struct WorkspaceColumnPanelHandles: View {
    @ObservedObject var store: EditorStore
    let column: JSON
    let bounds: JSON
    @Environment(\.workspaceLayer) private var layer
    private var panel: JSON {
        store.snapshot["layout"]["collapsed"].array.first { $0["id"].uint == column.uint }?["group_panel"] ?? JSON()
    }
    var body: some View {
        if !panel.isNull {
            ForEach(panel["dividers"].array.indices, id: \.self) { index in
                handle(after: panel["panels"][index]["panel"], label: "Resize panels")
                    .background(EditorPalette(source: store.state["palette"])["tabbar"])
                    .placed(panel["dividers"][index].relative(to: bounds))
                    .environment(\.workspaceLayer, layer + 1).zIndex(1)
            }
            // The outer width grip is painted above each split's endpoint.
            // Give the retained input source the same ordering at intersections.
            handle(after: JSON(), label: "Resize group panel").placed(panel["resize"].relative(to: bounds))
                .environment(\.workspaceLayer, layer + 2).zIndex(2)
        }
    }
    private func handle(after: JSON, label: String) -> some View {
        WorkspaceResizeHandle(store: store,
            action: JSON(["type": "resize_column_panel", "column": column.raw, "after": after.raw]), label: label)
            .accessibilityIdentifier("column-panel-resize-\(column.uint)-\(after.isNull ? "width" : after.string)")
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
                WorkspaceToolbar(store: store, panel: panel, geometry: tiles, vertical: true)
                    .frame(height: tiles["content_height"].number)
                    .task(id: tileKey) {
                        let result: JSON = await withCheckedContinuation { c in
                            store.query(["type": "drawer_toolbar", "panel": panel["id"].raw, "width": max(1, width), "height": 800]) { c.resume(returning: $0) }
                        }
                        if !Task.isCancelled { tiles = result }
                    }
            } else {
                PanelControls(store: store, panel: panel, scrollable: false, measureForWorkspace: false)
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
    var radius: CGFloat = 8
    func path(in rect: CGRect) -> Path {
        UnevenRoundedRectangle(topLeadingRadius: corners[0].bool ? 0 : radius, bottomLeadingRadius: corners[3].bool ? 0 : radius,
            bottomTrailingRadius: corners[2].bool ? 0 : radius, topTrailingRadius: corners[1].bool ? 0 : radius).path(in: rect)
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
