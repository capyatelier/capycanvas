import SwiftUI

struct WorkspacePanels: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var workspace: WorkspacePresentation
    @State private var panelBridgeAllocations: [String: PanelBridgeAllocation] = [:]
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
                Group {
                    WorkspacePanelGroup(store: store, workspace: workspace, id: group["id"].uint)
                        .environment(\.workspaceLayer, groups.firstIndex(where: { $0.workspaceGroupID == group.workspaceGroupID }) ?? 0)
                    if workspace.expansion.isNull {
                        ForEach(group["resize_handles"].array.indices, id: \.self) { index in
                            let handle = group["resize_handles"][index]
                            WorkspaceResizeHandle(store: store, action: JSON(["type": "resize_floating", "group": group["id"].raw, "edge": handle["edge"].raw]))
                                .modifier(WorkspacePlacement(motion: store.workspaceMotion, group: group, bounds: handle["bounds"]))
                                .accessibilityIdentifier("workspace-resize-\(group["id"].uint)-\(handle["edge"].string)")
                        }
                    }
                }.modifier(WorkspaceChromeVisibility(hidden: store.snapshot["chrome_hidden"].bool && !group["floating"].bool && !group["zen_kept"].bool))
            }
            if workspace.expansion.isNull && !store.snapshot["chrome_hidden"].bool {
                ForEach(store.snapshot["layout"]["dividers"].array.indices, id: \.self) { index in
                    let divider = store.snapshot["layout"]["dividers"][index]
                    if !divider["fixed"].bool {
                        WorkspaceResizeHandle(store: store, action: JSON(["type": "drag_divider", "id": divider["id"].raw]),
                            reset: divider["band"].bool && divider["axis"].string == "horizontal" ? {
                                store.dispatch(["type": "reset_column_width", "id": divider["id"].raw,
                                    "viewport": store.snapshot["layout"]["viewport"].raw])
                            } : nil)
                            .placed(divider["bounds"])
                            .accessibilityIdentifier("workspace-divider-\(divider["id"].uint)")
                            .accessibilityAdjustableAction { direction in
                                store.dispatch(["type": "nudge_divider", "id": divider["id"].raw, "forward": direction == .increment,
                                    "viewport": store.snapshot["layout"]["viewport"].raw])
                            }
                    }
                }
            }
            CanvasBarLayer(store: store, presence: store.canvasBar).zIndex(180)
            CanvasFloorLayer(store: store, presence: store.notice, bar: store.canvasBar).zIndex(181)
            WorkspaceDropIndicator(workspace: workspace, palette: EditorPalette(source: store.state["palette"])).zIndex(300)
            WorkspaceCollapsedColumns(store: store).modifier(WorkspaceChromeVisibility(hidden: store.snapshot["chrome_hidden"].bool))
            WorkspaceContentDrawers(store: store, drawers: store.contentDrawers)
            WorkspaceTabSlideOverlay(store: store, slide: workspace.tabSlide).zIndex(250)
            ToolbarSliderPreviewOverlay(preview: workspace.sliderPreview, palette: EditorPalette(source: store.state["palette"])).zIndex(350)
            PaletteDragOverlay(controller: store.palettes, hdr: store.snapshot["color_panel"]["hdr"].bool, viewing: store.colorViewing).zIndex(380)
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
            .onPreferenceChange(PanelBridgeAllocations.self) { allocations in
                panelBridgeAllocations = allocations
                store.glass.panelMeasurements(allocations, layout: store.snapshot["layout"])
            }
            .onChange(of: store.snapshot["layout"].stableKey) { _, _ in
                store.glass.panelMeasurements(panelBridgeAllocations, layout: store.snapshot["layout"])
            }
            .onChange(of: store.state["palette"]["glass"].stableKey) { _, _ in store.glass.panelTabsChanged() }
            .onPreferenceChange(DrawerTileMeasurements.self) { store.contentDrawers.measureTiles($0) }
            .onPreferenceChange(ColumnDrawerMeasurements.self) { store.contentDrawers.measureColumns($0) }
            .onDisappear { store.contentDrawers.measureColumns([:]); store.contentDrawers.measureTiles([:]) }
    }
}

struct WorkspaceResizeHandle: View {
    @ObservedObject var store: EditorStore
    let action: JSON
    var reset: (() -> Void)? = nil
    @State private var hovering = false
    @Environment(\.editorPalette) private var palette
    var body: some View {
        Color.clear.contentShape(Rectangle())
            .background(hovering ? palette.accent.opacity(0.3) : Color.clear)
            .onHover { hovering = $0 }.accessibilityElement()
            .accessibilityLabel(store.catalog["document_delivery_copy"]["resize_dock"].string)
            .modifier(WorkspaceDrag(workspace: store.workspace, item: action))
            .focusable(action["type"].string == "drag_divider")
            .onKeyPress(keys: [.leftArrow, .upArrow, .rightArrow, .downArrow], phases: [.down, .repeat]) { press in
                guard action["type"].string == "drag_divider", press.modifiers.isEmpty else { return .ignored }
                store.dispatch(["type": "nudge_divider", "id": action["id"].raw,
                    "forward": press.key == .rightArrow || press.key == .downArrow,
                    "viewport": store.snapshot["layout"]["viewport"].raw])
                return .handled
            }
            .simultaneousGesture(TapGesture(count: 2).onEnded {
                guard !store.workspace.input.contact.consumeClick() else { return }
                reset?()
            }, isEnabled: reset != nil)
    }
}

private struct WorkspacePanelGroup: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var workspace: WorkspacePresentation
    let id: UInt64
    var body: some View {
        if let group = store.snapshot["layout"]["groups"].array.first(where: { $0["id"].uint == id }) {
            let expansion = workspace.expansion["group"].uint == id ? workspace.expansion : JSON()
            WorkspacePanelGroupContent(store: store, group: group, expansion: expansion,
                panels: group["panels"].array.map { store.panel($0.string) })
                .environment(\.workspaceGesturesEnabled, (!store.snapshot["chrome_hidden"].bool || group["floating"].bool || group["zen_kept"].bool) && (workspace.expansion.isNull || !expansion.isNull))
                .modifier(PanelBridgeMeasurement(key: "group:\(id)", active: expansion.isNull && !group["floating"].bool))
                .modifier(WorkspacePlacement(motion: store.workspaceMotion, group: group,
                    bounds: expansion.isNull ? group["bounds"] : expansion["bounds"], clipsGroup: true))
                .allowsHitTesting(workspace.expansion.isNull || !expansion.isNull)
        }
    }
}
private struct WorkspacePanelGroupContent: View {
    @ObservedObject var store: EditorStore
    let group: JSON
    let expansion: JSON
    let panels: [JSON]
    @Environment(\.workspaceClip) private var clip
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var active: JSON { panels.first { $0["id"].string == group["active"].string } ?? JSON() }
    private var radius: CGFloat {
        !group["tiles"].isNull && !group["tabs_visible"].bool ? group["tiles"]["tile_corner_radius"].number : SquircleShape.surfaceRadius
    }
    var body: some View {
        let sources = Array(store.contentDrawers.sources.values)
        let connected = expansion.isNull && store.glass.panelGroupConnected(group["id"].uint)
        let sharedRadii = store.glass.panelGroupRadii(group["id"].uint)
        let square = sharedRadii == nil ? DrawerSource.square(group["bounds"].rect, radius: radius, sources: sources) : []
        let shape = sharedRadii.map { SquircleShape(topLeading: $0[0], topTrailing: $0[1], bottomTrailing: $0[2], bottomLeading: $0[3]) }
            ?? SquircleShape(radius, square: square)
        ZStack(alignment: .topLeading) {
            if expansion.isNull && !connected { OutsideShadow(shape: shape, opacity: 0.16, radius: 4, y: 2) }
            Group {
                if expansion.isNull {
                    content.clipShape(shape.fittedClip)
                        .environment(\.editorPalette, palette.glassy)
                        .backgroundPreferenceValue(WorkspaceActiveTab.self) { tab in
                            if !connected { surface(radius: radius, square: square, tab: tab) }
                        }
                        .modifier(GlassRegistration(shape: shape, active: !connected))
                } else {
                    content.background(palette["panel"])
                        .clipShape(WorkspacePanelShape(expansion: expansion, radius: radius, square: []))
                        .shadow(color: .black.opacity(0.4), radius: 12, y: 8)
                }
            }.modifier(NavigatorReveal())
                .accessibilityIdentifier("workspace-group-\(group["id"].uint)")
        }
    }
    @ViewBuilder private func surface(radius: CGFloat, square: [Bool], tab: WorkspaceTabFrame?) -> some View {
        if group["tabs_visible"].bool && group["active"].string != "navigator" {
            let strip = SquircleShape(topLeading: square[0] ? 0 : radius, topTrailing: square[1] ? 0 : radius)
            let body = SquircleShape(bottomTrailing: square[2] ? 0 : radius, bottomLeading: square[3] ? 0 : radius)
            VStack(spacing: 0) {
                WorkspaceTabStrip(shape: strip, tab: tab, strip: palette.glassStrip, panel: palette.glassPanel,
                    slide: store.workspace.tabSlide, group: group["id"].uint, active: group["active"].string).frame(height: 36)
                body.fill(palette.glassPanel)
            }
        } else {
            SquircleShape(radius, square: square).fill(palette.glassPanel)
        }
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
            if group["tabs_visible"].bool { WorkspacePanelHeader(store: store, group: group, panels: panels, glass: expansion.isNull) }
            if !tiles.isNull { WorkspaceToolbar(store: store, panel: active, geometry: tiles, vertical: group["axis"].string == "vertical") }
            else { panelBody }
        }.accessibilityElement(children: .contain).accessibilityIdentifier("panel-preview-" + active["id"].string)
    }
    private var panelBody: some View {
        GeometryReader { viewport in
            let footer = CGFloat(group["footer_grip"]["height"].number)
            let height = max(0, viewport.size.height - footer)
            let minimum = CGFloat(group["min_body_height"].number)
            let bounds = viewport.frame(in: .named("editor-workspace"))
            VStack(spacing: 0) {
                Group {
                    if height < minimum {
                        EditorScrollView {
                            PanelControls(store: store, panel: active).frame(height: minimum)
                        }
                    } else { PanelControls(store: store, panel: active) }
                }.frame(height: height)
                    .environment(\.workspaceClip, CGRect(x: bounds.minX, y: bounds.minY, width: bounds.width, height: height).intersection(clip))
                    .background(alignment: .top) { companionMeasurement }
                    .clipped()
                if !group["footer_grip"].isNull { grip.frame(height: footer) }
            }
        }
    }
    private var grip: some View {
        WorkspaceGroupGrip(store: store, group: group["id"], vertical: true)
    }
    @ViewBuilder private var companionMeasurement: some View {
        let panels = group["panels"].array.map(\.string)
        if panels.contains("color") && panels.contains("palettes") {
            if active["id"].string == "palettes" {
                let hdr = store.snapshot["color_panel"]["hdr"].bool
                Color.clear.frame(maxWidth: .infinity).frame(height: 1)
                    .modifier(PanelBodyMeasurement(panel: "color", naturalHeight: { max(128, $0 - 16) * ColorPanel.aspect(hdr: hdr) + 16 }))
                    .environment(\.measuresWorkspacePanel, true)
            } else if active["id"].string == "color" {
                PaletteMeasurement(store: store, controller: store.palettes).environment(\.measuresWorkspacePanel, true)
            }
        }
    }
}

struct WorkspaceToolbar: View {
    @ObservedObject var store: EditorStore
    let panel: JSON
    let geometry: JSON
    var vertical = false
    @State private var gripCaption = ""
    private var presented: JSON {
        guard !geometry["tile_style"].isNull else { return panel }
        var merged = panel.object
        for key in ["tile_style", "tile_icon_size", "tile_corner_radius", "tile_label_lines", "tile_label_bold"] {
            merged[key] = geometry[key].raw
        }
        return JSON(merged)
    }
    var body: some View {
        let panel = presented
        return ZStack(alignment: .topLeading) {
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
                .accessibilityElement().accessibilityLabel(gripCaption)
                .accessibilityIdentifier("toolbar-options-" + panel["id"].string)
                .accessibilityValue(panel["title"].string)
                .accessibilityHidden(false)
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": panel["id"].raw]),
                    context: JSON(["kind": "ribbon", "panel": panel["id"].raw]), openOnTap: true,
                    doubleClick: { store.doubleClickHandle(JSON(["kind": "panel", "panel": panel["id"].raw])) }))
                .placed(geometry["grip"])
            }
        }.modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": panel["id"].raw]),
            surface: .tile, context: JSON(["kind": "ribbon", "panel": panel["id"].raw]), canDrag: false, priority: -1))
        .onChange(of: [self.panel["title"].string, store.interfaceLanguage], initial: true) { _, key in
            gripCaption = NativeTextContext.caption(["type": "options_for", "title": key[0]], language: key[1])
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
                ToolbarDivider(vertical: vertical, color: palette["text"].opacity(0.3))
                    .contentShape(Rectangle())
            } else {
                ToolbarTileButton(panel: panel, tile: tile, palette: palette, drawerOpen: drawerOpen,
                    drawerDirection: store.contentDrawers.sources["tool"]?.direction) {
                    guard !store.workspace.input.contact.consumeClick() else { return }
                    let anchor: [String: Any] = ["kind": "tile", "panel": panel["id"].raw, "tile": tile["id"].raw]
                    ToolActivation.activate(tile["resolved_control"].isNull ? tile["control"] : tile["resolved_control"],
                        anchor: anchor, doubleClick: tile["double_click"].bool, store: store) {
                        store.dispatch(["type": "activate_tile", "panel": panel["id"].raw, "tile": tile["id"].raw])
                    }
                }.tracksToolActivation()
            }
        }.accessibilityLabel(tile["label"].string).help(tile.disabledReason ?? tile["tooltip"].string)
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

struct WorkspaceChromeVisibility: ViewModifier {
    let hidden: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    func body(content: Content) -> some View {
        content.modifier(WorkspaceChromeOpacity(opacity: hidden ? 0 : 1))
            .allowsHitTesting(!hidden).accessibilityHidden(hidden).disabled(hidden)
            .environment(\.workspaceGesturesEnabled, !hidden)
            .animation(reduceMotion ? nil : .easeOut(duration: 0.18), value: hidden)
    }
}

private struct WorkspaceChromeOpacity: AnimatableModifier {
    var opacity: Double
    var animatableData: Double {
        get { opacity }
        set { opacity = newValue }
    }
    func body(content: Content) -> some View {
        content.opacity(opacity).environment(\.workspaceChromeOpacity, opacity)
    }
}
private struct WorkspaceChromeOpacityKey: EnvironmentKey { static let defaultValue = 1.0 }
extension EnvironmentValues {
    var workspaceChromeOpacity: Double {
        get { self[WorkspaceChromeOpacityKey.self] }
        set { self[WorkspaceChromeOpacityKey.self] = newValue }
    }
}
