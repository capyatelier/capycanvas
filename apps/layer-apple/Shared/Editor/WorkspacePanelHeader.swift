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
                ScrollView(.horizontal) { HStack(spacing: 0) { tabs } }
                    .scrollIndicators(.hidden)
                    .scrollDisabled(store.workspace.tabSlide.grab != nil)
                    .environment(\.workspaceClip, viewport.frame(in: .named("editor-workspace")).intersection(clip))
                    .clipped()
            }
            WorkspaceGroupGrip(store: store, group: group["id"], drawer: drawer).frame(width: 20, height: 36)
        }.frame(height: 36).background(palette[drawer || group["active"].string == "navigator" ? "panel" : "tabbar"])
            .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "group", "group": group["id"].raw])))
    }
    private var tabs: some View {
        WorkspacePanelTabs(store: store, slide: store.workspace.tabSlide, group: group, drawer: drawer)
    }
}

private struct WorkspacePanelTabs: View {
    @ObservedObject var store: EditorStore
    let slide: WorkspaceTabSlide
    let group: JSON
    let drawer: Bool
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        ForEach(group["panels"].array.indices, id: \.self) { index in
            let tab = store.panel(group["panels"][index].string)
            let selected = tab["id"].string == group["active"].string
            Button { store.dispatch(["type": "select_panel_tab", "group": group["id"].raw, "panel": tab["id"].raw]) } label: {
                WorkspaceTabLabel(tab: tab, selected: selected, palette: palette).contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityLabel(tab["title"].string)
                .accessibilityIdentifier((drawer ? "drawer-tab-" : "panel-tab-") + tab["id"].string)
                .accessibilityAddTraits(selected ? .isSelected : [])
                .modifier(WorkspaceContext(store: store, target: JSON(["kind": "panel", "panel": tab["id"].raw])))
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": tab["id"].raw])))
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

/// The shared tab silhouette: round upper corners and concave lower shoulders.
/// The shoulders extend into adjacent slots; the strip clips its outer edges.
private struct WorkspaceTabShape: Shape {
    func path(in rect: CGRect) -> Path {
        let r: CGFloat = 6, w = rect.width, h = rect.height
        var path = Path()
        path.move(to: CGPoint(x: 0, y: h - r))
        path.addLine(to: CGPoint(x: 0, y: r))
        path.addArc(center: CGPoint(x: r, y: r), radius: r, startAngle: .degrees(180), endAngle: .degrees(270), clockwise: false)
        path.addLine(to: CGPoint(x: w - r, y: 0))
        path.addArc(center: CGPoint(x: w - r, y: r), radius: r, startAngle: .degrees(270), endAngle: .degrees(360), clockwise: false)
        path.addLine(to: CGPoint(x: w, y: h - r))
        path.addArc(center: CGPoint(x: w + r, y: h - r), radius: r, startAngle: .degrees(180), endAngle: .degrees(90), clockwise: true)
        path.addLine(to: CGPoint(x: -r, y: h))
        path.addArc(center: CGPoint(x: -r, y: h - r), radius: r, startAngle: .degrees(90), endAngle: .degrees(0), clockwise: true)
        path.closeSubpath()
        return path.offsetBy(dx: rect.minX, dy: rect.minY)
    }
}

struct WorkspaceGroupGrip: View {
    @ObservedObject var store: EditorStore
    let group: JSON
    var drawer = false
    var body: some View {
        SharedIcon(name: "grip").opacity(0.65).frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle()).accessibilityElement().accessibilityLabel("Panel group options")
            .accessibilityIdentifier((drawer ? "drawer-group-options-" : "group-options-") + String(group.uint))
            .accessibilityHidden(false)
            .modifier(WorkspaceContext(store: store, target: JSON(["kind": "group", "group": group.raw]), openOnTap: true,
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
