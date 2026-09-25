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
            WorkspacePanelLabelMeasurements(store: store)
            if !workspace.expansion.isNull {
                Color.clear.contentShape(Rectangle()).onTapGesture { store.customize(["type": "close_expanded"]) }
            }
            ForEach(groups, id: \.workspaceGroupID) { group in
                if !store.snapshot["chrome_hidden"].bool || (group["floating"].bool && !store.snapshot["hide_floating_panels"].bool) {
                    let expanded = workspace.expansion["group"].uint == group["id"].uint ? workspace.expansion : JSON()
                    WorkspacePanelGroup(store: store, group: group, expansion: expanded)
                        .environment(\.workspaceGesturesEnabled, workspace.expansion.isNull || !expanded.isNull)
                        .environment(\.workspaceLayer, groups.firstIndex(where: { $0.workspaceGroupID == group.workspaceGroupID }) ?? 0)
                        .modifier(WorkspacePlacement(motion: store.workspaceMotion, group: group,
                            bounds: expanded.isNull ? group["bounds"] : expanded["bounds"], clipsGroup: true))
                        .allowsHitTesting(workspace.expansion.isNull || !expanded.isNull)
                    if workspace.expansion.isNull {
                        ForEach(group["resize_handles"].array.indices, id: \.self) { index in
                            let handle = group["resize_handles"][index]
                            WorkspaceResizeHandle(store: store, action: JSON(["type": "resize_floating", "group": group["id"].raw, "edge": handle["edge"].raw]))
                                .modifier(WorkspacePlacement(motion: store.workspaceMotion, group: group, bounds: handle["bounds"]))
                        }
                    }
                }
            }
            if workspace.expansion.isNull && !store.snapshot["chrome_hidden"].bool {
                ForEach(store.snapshot["layout"]["dividers"].array.indices, id: \.self) { index in
                    let divider = store.snapshot["layout"]["dividers"][index]
                    if !divider["fixed"].bool {
                        WorkspaceResizeHandle(store: store, action: JSON(["type": "drag_divider", "id": divider["id"].raw]))
                            .placed(divider["bounds"])
                            .accessibilityIdentifier("workspace-divider-\(divider["id"].uint)")
                            .accessibilityAdjustableAction { direction in
                                store.dispatch(["type": "nudge_divider", "id": divider["id"].raw, "forward": direction == .increment,
                                    "viewport": store.snapshot["layout"]["viewport"].raw])
                            }
                    }
                }
            }
            WorkspaceDropIndicator(workspace: workspace, palette: EditorPalette(source: store.state["palette"])).zIndex(300)
            if !store.snapshot["chrome_hidden"].bool { WorkspaceCollapsedColumns(store: store) }
            WorkspaceContentDrawers(store: store, drawers: store.contentDrawers)
            WorkspaceTabSlideOverlay(store: store, slide: workspace.tabSlide).zIndex(250)
            ToolbarSliderPreviewOverlay(preview: workspace.sliderPreview, palette: EditorPalette(source: store.state["palette"])).zIndex(350)
            WorkspaceContactMenu(interaction: workspace.input, store: store).zIndex(400)
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .compositingGroup()
            .modifier(WorkspaceRootDrag(workspace: workspace))
            .onPreferenceChange(NavigatorPlacements.self) { placements in
                let ordered = placements.sorted { $0.key.uuidString < $1.key.uuidString }.map { $0.value.json }
                store.native?.navigatorPlacements(JSON(ordered))
            }
            .onDisappear { store.native?.navigatorPlacements(JSON([])) }
            .onPreferenceChange(WorkspaceSources.self) { workspace.sourceInstances = $0 }
            .onPreferenceChange(PanelSizeFacts.self) { store.panelMeasurements.receive($0) }
            .onChange(of: store.snapshot["panel_measurements"].stableKey) { _, _ in store.panelMeasurements.reconcile() }
            .onPreferenceChange(DrawerTileMeasurements.self) { store.contentDrawers.measureTiles($0) }
            .onPreferenceChange(ColumnDrawerMeasurements.self) { store.contentDrawers.measureColumns($0) }
            .onDisappear { store.contentDrawers.measureColumns([:]); store.contentDrawers.measureTiles([:]) }
    }
}

struct WorkspaceResizeHandle: View {
    @ObservedObject var store: EditorStore
    let action: JSON
    var label = "Resize panel"
    @State private var hovering = false
    @Environment(\.editorPalette) private var palette
    var body: some View {
        Color.clear.contentShape(Rectangle())
            .background(hovering ? palette.accent.opacity(0.3) : Color.clear)
            .onHover { hovering = $0 }.accessibilityElement().accessibilityLabel(label)
            .modifier(WorkspaceDrag(workspace: store.workspace, item: action))
    }
}

private struct WorkspacePanelGroup: View {
    @ObservedObject var store: EditorStore
    let group: JSON
    let expansion: JSON
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private func panel(_ id: JSON) -> JSON { store.panel(id.string) }
    private var active: JSON { panel(group["active"]) }
    private var radius: CGFloat {
        !group["tiles"].isNull && !group["tabs_visible"].bool ? active["tile_corner_radius"].number : SquircleShape.surfaceRadius
    }
    var body: some View {
        Group {
            if expansion.isNull {
                let sources = Array(store.contentDrawers.sources.values)
                let shape = SquircleShape(radius, square: DrawerSource.square(group["bounds"].rect, radius: radius, sources: sources))
                content.clipShape(shape.fittedClip)
                    .background { shape.fill(palette["panel"]).shadow(color: .black.opacity(0.16), radius: 4, y: 2) }
            } else {
                content.background(palette["panel"])
                    .clipShape(WorkspacePanelShape(expansion: expansion, radius: radius, square: []))
                    .shadow(color: .black.opacity(0.4), radius: 12, y: 8)
            }
        }.modifier(NavigatorReveal())
            .accessibilityIdentifier("workspace-group-\(group["id"].uint)")
    }
    private var content: some View {
        Group {
            if expansion.isNull { preview(tiles: group["tiles"]) }
            else {
                ZStack(alignment: .topLeading) {
                    preview(tiles: expansion["tiles"]).placed(expansion["preview"])
                    PanelConfiguration(store: store, panel: active).placed(expansion["configuration"])
                }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        }
    }
    private func preview(tiles: JSON) -> some View {
        VStack(spacing: 0) {
            if group["tabs_visible"].bool { WorkspacePanelHeader(store: store, group: group) }
            if !tiles.isNull { WorkspaceToolbar(store: store, panel: active, geometry: tiles, vertical: group["axis"].string == "vertical") }
            else {
                PanelControls(store: store, panel: active)
                    .overlay(alignment: .topLeading) {
                        if !group["footer_grip"].isNull { grip.placed(group["footer_grip"]) }
                    }
            }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("panel-preview-" + active["id"].string)
    }
    private var grip: some View {
        WorkspaceGroupGrip(store: store, group: group["id"], vertical: true)
    }
}

struct WorkspaceToolbar: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let geometry: JSON
    var vertical = false
    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.clear
            ForEach(panel["tiles"].array.indices, id: \.self) { index in
                let tile = panel["tiles"][index], bounds = geometry["tiles"][index]
                if tile["component"].isNull {
                    WorkspaceTile(store: store, panel: panel, tile: tile, vertical: vertical)
                        .modifier(DrawerTileMeasurement(panel: panel["id"].string, tile: tile["id"].uint))
                        .modifier(WorkspaceDrag(workspace: store.workspace,
                            item: JSON(["kind": "tile", "panel": panel["id"].raw, "tile": tile["id"].raw]), surface: .tile,
                            context: JSON(["kind": "tile", "panel": panel["id"].raw, "tile": tile["id"].raw])))
                        .placed(bounds)
                } else if bounds["width"].number > 0 && bounds["height"].number > 0 {
                    ToolbarComponentView(store: store, panel: panel, tile: tile,
                        size: CGSize(width: bounds["width"].number, height: bounds["height"].number), vertical: vertical)
                        .modifier(DrawerTileMeasurement(panel: panel["id"].string, tile: tile["id"].uint))
                        .placed(bounds)
                }
            }
            if !geometry["grip"].isNull {
                PanelGrip(vertical: vertical).frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Rectangle())
                .accessibilityElement().accessibilityLabel("Toolbar options for " + panel["title"].string)
                .accessibilityIdentifier("toolbar-options-" + panel["id"].string)
                .accessibilityValue(panel["title"].string)
                .accessibilityHidden(false)
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": panel["id"].raw]),
                    context: JSON(["kind": "ribbon", "panel": panel["id"].raw]), openOnTap: true,
                    doubleClick: { store.doubleClickHandle(JSON(["kind": "panel", "panel": panel["id"].raw])) }))
                .placed(geometry["grip"])
            }
        }
    }
}

private struct WorkspaceTile: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let tile: JSON
    let vertical: Bool
    private var kind: String { tile["control"]["kind"].string }
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var drawerOpen: Bool { store.contentDrawers.sources["tool"]?.opens(tile: tile, in: panel) == true }
    var body: some View {
        Group {
            if kind == "divider" {
                Rectangle().fill(palette["text"].opacity(0.3))
                    .frame(width: vertical ? nil : 1, height: vertical ? 1 : nil)
                    .padding(vertical ? .horizontal : .vertical, 4)
                    .frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
            } else {
                ToolbarTileButton(panel: panel, tile: tile, palette: palette, colors: store.paintPair, drawerOpen: drawerOpen,
                    drawerDirection: store.contentDrawers.sources["tool"]?.direction) {
                    guard !store.workspace.input.contact.consumeClick() else { return }
                    let anchor: [String: Any] = ["kind": "tile", "panel": panel["id"].raw, "tile": tile["id"].raw]
                    PickerActivation.activate(tile["control"], anchor: anchor, store: store) {
                        store.dispatch(["type": "activate_tile", "panel": panel["id"].raw, "tile": tile["id"].raw])
                    }
                }
            }
        }.accessibilityLabel(tile["label"].string).help(tile["tooltip"].string)
            .accessibilityIdentifier("toolbar-tile-\(panel["id"].string)-\(tile["id"].uint)")
            .accessibilityAddTraits(tile["selected"].bool ? .isSelected : [])
    }
}

/// Same preview/configuration silhouette as Android, in logical UI coordinates.
private struct WorkspacePanelShape: Shape {
    let expansion: JSON
    let radius: CGFloat
    let square: [Bool]
    func path(in rect: CGRect) -> Path {
        guard !expansion.isNull, expansion["configuration"]["y"].number > 0 else {
            return SquircleShape(radius, square: square).path(in: rect)
        }
        let preview = expansion["preview"].rect, configuration = expansion["configuration"].rect
        let left = preview.minX, right = preview.maxX, top = configuration.minY
        let width = rect.width, height = rect.height, r = min(SquircleShape.surfaceRadius, height / 2, width / 2), join = min(8, r)
        func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: x, y: y) }
        var p = Path()
        p.move(to: point(left + r, 0)); p.addLine(to: point(right - r, 0))
        p.squircle(center: point(right - r, r), start: CGVector(dx: 0, dy: -r), end: CGVector(dx: r, dy: 0))
        if right < width {
            p.addLine(to: point(right, top)); p.addLine(to: point(width - r, top))
            p.squircle(center: point(width - r, top + r), start: CGVector(dx: 0, dy: -r), end: CGVector(dx: r, dy: 0))
        }
        p.addLine(to: point(width, height - r))
        p.squircle(center: point(width - r, height - r), start: CGVector(dx: r, dy: 0), end: CGVector(dx: 0, dy: r))
        p.addLine(to: point(r, height))
        p.squircle(center: point(r, height - r), start: CGVector(dx: 0, dy: r), end: CGVector(dx: -r, dy: 0))
        if left > 0 {
            p.addLine(to: point(0, top + r))
            p.squircle(center: point(r, top + r), start: CGVector(dx: -r, dy: 0), end: CGVector(dx: 0, dy: -r))
            if expansion["concave_join"].bool {
                p.addLine(to: point(left - join, top))
                p.squircle(center: point(left - join, top - join), start: CGVector(dx: 0, dy: join), end: CGVector(dx: join, dy: 0))
            } else { p.addLine(to: point(left, top)) }
        }
        p.addLine(to: point(left, r))
        p.squircle(center: point(left + r, r), start: CGVector(dx: -r, dy: 0), end: CGVector(dx: 0, dy: -r))
        p.closeSubpath()
        return p.offsetBy(dx: rect.minX, dy: rect.minY)
    }
}

private extension JSON { var workspaceGroupID: UInt64 { self["id"].uint } }
