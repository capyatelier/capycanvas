import SwiftUI

struct WorkspacePanels: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var workspace: WorkspacePresentation
    private var groups: [JSON] {
        store.snapshot["layout"]["groups"].array.sorted {
            ($0["id"].uint == workspace.expansion["group"].uint ? 1 : 0)
                < ($1["id"].uint == workspace.expansion["group"].uint ? 1 : 0)
        }
    }
    var body: some View {
        ZStack(alignment: .topLeading) {
            if !workspace.expansion.isNull {
                Color.clear.contentShape(Rectangle()).onTapGesture { store.customize(["type": "close_expanded"]) }
            }
            ForEach(groups, id: \.workspaceGroupID) { group in
                if !store.snapshot["chrome_hidden"].bool || (group["floating"].bool && !store.snapshot["hide_floating_panels"].bool) {
                    let expanded = workspace.expansion["group"].uint == group["id"].uint ? workspace.expansion : JSON()
                    WorkspacePanelGroup(store: store, group: group, expansion: expanded)
                        .environment(\.workspaceGesturesEnabled, workspace.expansion.isNull || !expanded.isNull)
                        .placed(expanded.isNull ? group["bounds"] : expanded["bounds"])
                        .allowsHitTesting(workspace.expansion.isNull || !expanded.isNull)
                    if workspace.expansion.isNull {
                        ForEach(group["resize_handles"].array.indices, id: \.self) { index in
                            let handle = group["resize_handles"][index]
                            WorkspaceResizeHandle(store: store, action: JSON(["type": "resize_floating", "group": group["id"].raw, "edge": handle["edge"].raw]))
                                .placed(handle["bounds"])
                        }
                    }
                }
            }
            if workspace.expansion.isNull && !store.snapshot["chrome_hidden"].bool {
                ForEach(store.snapshot["layout"]["dividers"].array.indices, id: \.self) { index in
                    let divider = store.snapshot["layout"]["dividers"][index]
                    WorkspaceResizeHandle(store: store, action: JSON(["type": "drag_divider", "id": divider["id"].raw]))
                        .placed(divider["bounds"])
                        .accessibilityAdjustableAction { direction in
                            store.dispatch(["type": "nudge_divider", "id": divider["id"].raw, "forward": direction == .increment,
                                "viewport": store.snapshot["layout"]["viewport"].raw])
                        }
                }
            }
            if !workspace.dropHint.isNull {
                RoundedRectangle(cornerRadius: 5).fill(Color.accentColor.opacity(0.2))
                    .overlay(RoundedRectangle(cornerRadius: 5).stroke(Color.accentColor, lineWidth: 2))
                    .placed(workspace.dropHint["bounds"]).allowsHitTesting(false).accessibilityHidden(true)
            }
            if store.snapshot["partial_zen"].bool { WorkspaceZenToolbars(store: store) }
            if !store.snapshot["chrome_hidden"].bool { WorkspaceCollapsedColumns(store: store) }
            WorkspaceContentDrawers(store: store, drawers: store.contentDrawers)
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .modifier(WorkspaceRootDrag(workspace: workspace))
            .onPreferenceChange(WorkspaceSources.self) { workspace.sources = $0 }
            .onPreferenceChange(DrawerTileMeasurements.self) { store.contentDrawers.measureTiles($0) }
    }
}

private struct WorkspaceResizeHandle: View {
    @ObservedObject var store: EditorStore
    let action: JSON
    @State private var hovering = false
    var body: some View {
        Color.clear.contentShape(Rectangle())
            .background(hovering ? Color.accentColor.opacity(0.3) : Color.clear)
            .onHover { hovering = $0 }.accessibilityElement().accessibilityLabel("Resize panel")
            .modifier(WorkspaceDrag(workspace: store.workspace, item: action))
    }
}

private struct WorkspacePanelGroup: View {
    @ObservedObject var store: EditorStore
    let group: JSON
    let expansion: JSON
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func panel(_ id: JSON) -> JSON { store.snapshot["panels"].array.first { $0["id"].string == id.string } ?? JSON() }
    private var active: JSON { panel(group["active"]) }
    var body: some View {
        Group {
            if expansion.isNull { preview(tiles: group["tiles"]) }
            else {
                ZStack(alignment: .topLeading) {
                    preview(tiles: expansion["tiles"]).placed(expansion["preview"])
                    PanelConfiguration(store: store, panel: active).placed(expansion["configuration"])
                }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        }.background(palette["panel"])
            .clipShape(WorkspacePanelShape(expansion: expansion))
            .shadow(color: .black.opacity(0.22), radius: expansion.isNull ? 6 : 16, y: 2)
            .accessibilityIdentifier("workspace-group-\(group["id"].uint)")
    }
    private func preview(tiles: JSON) -> some View {
        VStack(spacing: 0) {
            if group["tabs_visible"].bool { header }
            if !tiles.isNull { WorkspaceToolbar(store: store, panel: active, geometry: tiles) }
            else {
                PanelControls(store: store, panel: active)
                    .overlay(alignment: .topLeading) {
                        if !group["footer_grip"].isNull { grip.placed(group["footer_grip"]) }
                    }
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("panel-preview-" + active["id"].string)
    }
    private var header: some View {
        HStack(spacing: 0) {
            ForEach(group["panels"].array.indices, id: \.self) { index in
                let tab = panel(group["panels"][index])
                Button { store.dispatch(["type": "select_panel_tab", "group": group["id"].raw, "panel": tab["id"].raw]) } label: {
                    HStack(spacing: 6) {
                        if tab["tab"]["show_icon"].bool { SharedIcon(name: tab["icon"].string) }
                        if tab["tab"]["show_name"].bool { Text(tab["title"].string).fontWeight(.bold).lineLimit(1) }
                    }.padding(.horizontal, 8).frame(height: 36)
                        .background(tab["id"].string == active["id"].string ? palette["panel"] : Color.clear)
                }.buttonStyle(.plain).accessibilityLabel(tab["title"].string)
                    .accessibilityIdentifier("panel-tab-" + tab["id"].string)
                    .modifier(WorkspaceContext(store: store, target: JSON(["kind": "panel", "panel": tab["id"].raw])))
                    .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": tab["id"].raw])))
                    .background(GeometryReader { allocation in
                        Color.clear.preference(key: WorkspaceTabs.self,
                            value: ["\(group["id"].uint):\(index)": allocation.frame(in: .named("editor-workspace"))])
                    })
            }
            Spacer(minLength: 0)
            grip.frame(width: 20, height: 36)
        }.background(palette["tabbar"])
    }
    private var grip: some View {
        SharedIcon(name: "grip").opacity(0.65).frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle()).accessibilityElement().accessibilityLabel("Panel group options")
            .accessibilityIdentifier("group-options-\(group["id"].uint)")
            .accessibilityHidden(false)
            .modifier(WorkspaceContext(store: store, target: JSON(["kind": "group", "group": group["id"].raw]), openOnTap: true,
                doubleClick: { store.doubleClickHandle(JSON(["kind": "group", "group": group["id"].raw])) }))
            .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "group", "group": group["id"].raw])))
    }
}

struct WorkspaceToolbar: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let geometry: JSON
    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.clear
            ForEach(panel["tiles"].array.indices, id: \.self) { index in
                let tile = panel["tiles"][index]
                WorkspaceTile(store: store, panel: panel, tile: tile)
                    .modifier(DrawerTileMeasurement(panel: panel["id"].string, tile: tile["id"].uint))
                    .modifier(WorkspaceContext(store: store, target: JSON(["kind": "tile", "panel": panel["id"].raw, "tile": tile["id"].raw])))
                    .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "tile", "panel": panel["id"].raw, "tile": tile["id"].raw])))
                    .placed(geometry["tiles"][index])
            }
            if !geometry["grip"].isNull {
                SharedIcon(name: "grip").opacity(0.65).frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Rectangle())
                .accessibilityElement().accessibilityLabel("Toolbar options")
                .accessibilityIdentifier("toolbar-options-" + panel["id"].string)
                .accessibilityValue(panel["title"].string)
                .accessibilityHidden(false)
                .modifier(WorkspaceContext(store: store, target: JSON(["kind": "ribbon", "panel": panel["id"].raw]), openOnTap: true,
                    doubleClick: { store.doubleClickHandle(JSON(["kind": "panel", "panel": panel["id"].raw])) }))
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": panel["id"].raw])))
                .placed(geometry["grip"])
            }
        }
    }
}

private struct WorkspaceTile: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let tile: JSON
    private var kind: String { tile["control"]["kind"].string }
    private var style: String { panel["tile_style"].string }
    var body: some View {
        Group {
            if kind == "divider" {
                Rectangle().fill(.secondary.opacity(0.4)).frame(width: 1).padding(.vertical, 7)
                    .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
            } else {
                Button { store.dispatch(["type": "activate_tile", "panel": panel["id"].raw, "tile": tile["id"].raw]) } label: {
                    VStack(spacing: 4) {
                        if kind == "color" { ColorSwatch(rgba: store.state["brush"]["color"]).frame(width: 22, height: 22) }
                        else if kind == "size" { Text(String(tile["control"]["pixels"].uint)) }
                        else { SharedIcon(name: tile["icon"].string, size: style == "large" ? 32 : 16) }
                        if style == "labeled" { Text(tile["label"].string).font(.system(size: 11)).lineLimit(2).multilineTextAlignment(.center) }
                    }.frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                }.buttonStyle(.plain).disabled(!tile["enabled"].bool).opacity(tile["enabled"].bool ? 1 : 0.4)
                    .background(tile["selected"].bool ? Color.accentColor.opacity(0.22) : Color.clear, in: RoundedRectangle(cornerRadius: 6))
            }
        }.accessibilityLabel(tile["label"].string).help(tile["tooltip"].string)
            .accessibilityIdentifier("toolbar-tile-\(panel["id"].string)-\(tile["id"].uint)")
            .accessibilityAddTraits(tile["selected"].bool ? .isSelected : [])
    }
}

/// Same preview/configuration silhouette as Android, in logical UI coordinates.
private struct WorkspacePanelShape: Shape {
    let expansion: JSON
    func path(in rect: CGRect) -> Path {
        guard !expansion.isNull else { return Path(roundedRect: rect, cornerRadius: 8) }
        let preview = expansion["preview"].rect, configuration = expansion["configuration"].rect
        let left = preview.minX, right = preview.maxX, top = configuration.minY
        let width = rect.width, height = rect.height, radius = min(8, min(height, width) / 2)
        var p = Path()
        p.move(to: CGPoint(x: left + radius, y: 0)); p.addLine(to: CGPoint(x: right - radius, y: 0))
        p.addQuadCurve(to: CGPoint(x: right, y: radius), control: CGPoint(x: right, y: 0))
        if right < width {
            p.addLine(to: CGPoint(x: right, y: top)); p.addLine(to: CGPoint(x: width - radius, y: top))
            p.addQuadCurve(to: CGPoint(x: width, y: top + radius), control: CGPoint(x: width, y: top))
        }
        p.addLine(to: CGPoint(x: width, y: height - radius))
        p.addQuadCurve(to: CGPoint(x: width - radius, y: height), control: CGPoint(x: width, y: height))
        p.addLine(to: CGPoint(x: radius, y: height)); p.addQuadCurve(to: CGPoint(x: 0, y: height - radius), control: CGPoint(x: 0, y: height))
        if left > 0 {
            p.addLine(to: CGPoint(x: 0, y: top + radius)); p.addQuadCurve(to: CGPoint(x: radius, y: top), control: CGPoint(x: 0, y: top))
            if expansion["concave_join"].bool {
                p.addLine(to: CGPoint(x: left - radius, y: top)); p.addQuadCurve(to: CGPoint(x: left, y: top - radius), control: CGPoint(x: left, y: top))
            } else { p.addLine(to: CGPoint(x: left, y: top)) }
        }
        p.addLine(to: CGPoint(x: left, y: radius)); p.addQuadCurve(to: CGPoint(x: left + radius, y: 0), control: CGPoint(x: left, y: 0)); p.closeSubpath()
        return p
    }
}

private extension JSON { var workspaceGroupID: UInt64 { self["id"].uint } }
