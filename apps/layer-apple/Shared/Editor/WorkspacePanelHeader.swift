import SwiftUI

/// A column drawer is another presentation of the same dock group. Its tabs,
/// options and drag sources use the ordinary panel actions and shared policy.
struct WorkspacePanelHeader: View {
    @ObservedObject var store: EditorStore
    let group: JSON
    var drawer = false
    @Environment(\.workspaceClip) private var clip
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        HStack(spacing: 0) {
            GeometryReader { viewport in
                EditorScrollView(.horizontal) { HStack(spacing: 0) { tabs(available: viewport.size.width) } }
                    .scrollIndicators(.hidden)
                    .scrollDisabled(store.workspace.tabSlide.grab != nil)
                    .environment(\.workspaceClip, viewport.frame(in: .named("editor-workspace")).intersection(clip))
                    .clipped()
            }
            WorkspaceGroupGrip(store: store, group: group["id"], drawer: drawer).frame(width: 20, height: 36)
        }.frame(height: 36).background(palette[drawer || group["active"].string == "navigator" ? "panel" : "tabbar"])
            .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "group", "group": group["id"].raw]),
                context: JSON(["kind": "group", "group": group["id"].raw])))
    }
    private func tabs(available: CGFloat) -> some View {
        WorkspacePanelTabs(store: store, slide: store.workspace.tabSlide, group: group, drawer: drawer, available: available)
    }
}

private struct WorkspacePanelTabs: View {
    @ObservedObject var store: EditorStore
    let slide: WorkspaceTabSlide
    let group: JSON
    let drawer: Bool
    let available: CGFloat
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var automatic: Bool {
        func style(_ node: JSON) -> String? {
            if node["kind"].string == "tabs" && node["id"].uint == group["id"].uint { return node["tab_style"].string }
            for child in node.object.values.map(JSON.init) where !child.object.isEmpty || !child.array.isEmpty {
                if let found = style(child) { return found }
            }
            for child in node.array { if let found = style(child) { return found } }
            return nil
        }
        return style(store.state["workspace"]["layout"]) == "automatic"
    }
    private var tabs: [JSON] {
        let panels = group["panels"].array.map { store.panel($0.string) }
        guard automatic, available > 0 else { store.workspace.fittedTabs[group["id"].uint] = nil; return panels }
        let size = store.catalog["text_size_pt"].number > 0 ? store.catalog["text_size_pt"].number * 4 / 3 : 44 / 3
        let widths = panels.map { [38 + EditorTextMetrics.width($0["title"].string, size: size, weight: .bold), 36] }
        let fitted = ToolbarUI.cached(["type": "automatic_tab_names", "available": available, "widths": widths]).array.map(\.bool)
        store.workspace.fittedTabs[group["id"].uint] = fitted.count == panels.count ? fitted : nil
        return store.workspace.presentedTabs(group: group["id"].uint, panels: panels)
    }
    var body: some View {
        let tabs = self.tabs
        ForEach(group["panels"].array.indices, id: \.self) { index in
            let tab = tabs[index]
            let selected = tab["id"].string == group["active"].string
            Button {
                if !store.workspace.input.contact.consumeClick() { store.dispatch(["type": "select_panel_tab", "group": group["id"].raw, "panel": tab["id"].raw]) }
            } label: {
                WorkspaceTabLabel(tab: tab, selected: selected, palette: palette).contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityLabel(tab["title"].string)
                .accessibilityIdentifier((drawer ? "drawer-tab-" : "panel-tab-") + tab["id"].string)
                .accessibilityAddTraits(selected ? .isSelected : [])
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": tab["id"].raw]),
                    context: JSON(["kind": "panel", "panel": tab["id"].raw])))
                .modifier(WorkspaceTabMeasurement(group: group["id"].uint, index: index))
                .zIndex(selected ? 1 : 0)
                .opacity(slide.isVisible(in: group["id"].uint) ? 0 : 1)
        }
    }
}

struct WorkspaceTabLabel: View {
    let tab: JSON
    let selected: Bool
    let palette: EditorPalette
    var body: some View {
        HStack(spacing: 6) {
            if tab["tab"]["show_icon"].bool { SharedIcon(name: tab["icon"].string) }
            if tab["tab"]["show_name"].bool { Text(tab["title"].string).fontWeight(.bold).lineLimit(1) }
        }.padding(.horizontal, 8)
            .frame(width: tab["tab"]["show_name"].bool ? nil : 36, height: 36)
            .fixedSize(horizontal: true, vertical: false)
            .background { if selected { WorkspaceTabShape().fill(palette["panel"]) } }
    }
}

/// The shared tab silhouette: squircle upper corners and concave lower feet.
/// The feet extend into adjacent slots; the strip clips its outer edges.
private struct WorkspaceTabShape: Shape {
    func path(in rect: CGRect) -> Path {
        let w = rect.width, h = rect.height, r = min(SquircleShape.surfaceRadius, w / 2, h), f: CGFloat = 6
        var path = Path()
        path.move(to: CGPoint(x: r, y: 0)); path.addLine(to: CGPoint(x: w - r, y: 0))
        path.squircle(center: CGPoint(x: w - r, y: r), start: CGVector(dx: 0, dy: -r), end: CGVector(dx: r, dy: 0))
        path.addLine(to: CGPoint(x: w, y: h - f))
        path.squircle(center: CGPoint(x: w + f, y: h - f), start: CGVector(dx: -f, dy: 0), end: CGVector(dx: 0, dy: f))
        path.addLine(to: CGPoint(x: -f, y: h))
        path.squircle(center: CGPoint(x: -f, y: h - f), start: CGVector(dx: 0, dy: f), end: CGVector(dx: f, dy: 0))
        path.addLine(to: CGPoint(x: 0, y: r))
        path.squircle(center: CGPoint(x: r, y: r), start: CGVector(dx: -r, dy: 0), end: CGVector(dx: 0, dy: -r))
        path.closeSubpath()
        return path.offsetBy(dx: rect.minX, dy: rect.minY)
    }
}

struct WorkspaceGroupGrip: View {
    @ObservedObject var store: EditorStore
    let group: JSON
    var drawer = false
    var vertical = false
    var body: some View {
        PanelGrip(vertical: vertical).frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle()).accessibilityElement().accessibilityLabel("Panel group options")
            .accessibilityIdentifier((drawer ? "drawer-group-options-" : "group-options-") + String(group.uint))
            .accessibilityHidden(false)
            .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "group", "group": group.raw]),
                context: JSON(["kind": "group", "group": group.raw]), openOnTap: true,
                doubleClick: { store.doubleClickHandle(JSON(["kind": "group", "group": group.raw])) }))
    }
}

private struct WorkspaceTabMeasurement: ViewModifier {
    let group: UInt64
    let index: Int
    @Environment(\.workspaceClip) private var clip
    @Environment(\.workspaceGesturesEnabled) private var enabled
    func body(content: Content) -> some View {
        content.background(GeometryReader { allocation in
            let full = allocation.frame(in: .named("editor-workspace"))
            let bounds = full.intersection(clip)
            Color.clear.preference(key: WorkspaceTabs.self,
                value: enabled && !bounds.isEmpty ? ["\(group):\(index)": bounds] : [:])
                .preference(key: WorkspaceTabFrames.self,
                    value: enabled && !full.isEmpty ? ["\(group):\(index)": WorkspaceTabFrame(bounds: full, clip: clip)] : [:])
        })
    }
}
