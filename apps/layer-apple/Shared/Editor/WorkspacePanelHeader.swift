import SwiftUI

/// A column drawer is another presentation of the same dock group. Its tabs,
/// options and drag sources use the ordinary panel actions and shared policy.
struct WorkspacePanelHeader: View {
    @ObservedObject var store: EditorStore
    let group: JSON
    let panels: [JSON]
    var drawer = false
    var glass = false
    @Environment(\.workspaceClip) private var clip
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    private var navigator: Bool { group["active"].string == "navigator" }
    private var strip: Color { glass || drawer ? .clear : palette[navigator ? "panel" : "tabbar"] }
    private var tab: Color? { drawer || glass ? nil : palette["panel"] }
    var body: some View {
        HStack(spacing: 0) {
            GeometryReader { viewport in
                EditorScrollView(.horizontal) { HStack(spacing: 0) { tabs() } }
                    .scrollIndicators(.hidden)
                    .scrollDisabled(store.workspace.tabSlide.grab != nil)
                    .environment(\.workspaceClip, viewport.frame(in: .named("editor-workspace")).intersection(clip))
                    .clipped()
            }
            WorkspaceGroupGrip(store: store, group: group["id"], drawer: drawer).frame(width: 20, height: 36)
        }.frame(height: 36).background(strip)
            .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "group", "group": group["id"].raw]),
                context: JSON(["kind": "group", "group": group["id"].raw])))
    }
    private func tabs() -> some View {
        WorkspacePanelTabs(store: store, slide: store.workspace.tabSlide, group: group, panels: panels, drawer: drawer, fill: tab, cutsStrip: glass)
    }
}

private struct WorkspacePanelTabs: View {
    @ObservedObject var store: EditorStore
    let slide: WorkspaceTabSlide
    let group: JSON
    let panels: [JSON]
    let drawer: Bool
    let fill: Color?
    let cutsStrip: Bool
    private var palette: EditorPalette { EditorPalette(source: store.state["palette"]) }
    var body: some View {
        let tabs = store.workspace.presentedTabs(group: group, panels: panels)
        ForEach(group["panels"].array.indices, id: \.self) { index in
            let tab = tabs[index]
            let selected = tab["id"].string == group["active"].string
            let hidden = slide.isVisible(in: group["id"].uint)
            Button {
                if !store.workspace.input.contact.consumeClick() { store.dispatch(["type": "select_panel_tab", "group": group["id"].raw, "panel": tab["id"].raw]) }
            } label: {
                WorkspaceTabLabel(tab: tab, selected: selected, palette: palette, fill: fill).contentShape(Rectangle())
            }.buttonStyle(WorkspaceTabButtonStyle(selected: selected, ink: palette["text"], bottom: tab["tab"]["hover_bottom"])).accessibilityLabel(tab["title"].string)
                .accessibilityIdentifier((drawer ? "drawer-tab-" : "panel-tab-") + tab["id"].string)
                .accessibilityAddTraits(selected ? .isSelected : [])
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": tab["id"].raw]),
                    context: JSON(["kind": "panel", "panel": tab["id"].raw])))
                .modifier(WorkspaceTabMeasurement(group: group["id"].uint, index: index, cutsStrip: cutsStrip && selected && !hidden))
                .zIndex(selected ? 1 : 0)
                .opacity(hidden ? 0 : 1)
        }
    }
}

private struct WorkspaceTabButtonStyle: ButtonStyle {
    let selected: Bool
    let ink: Color
    let bottom: JSON
    func makeBody(configuration: Configuration) -> some View {
        Feedback(label: configuration.label, selected: selected, ink: ink, bottom: bottom)
    }
    private struct Feedback: View {
        let label: ButtonStyleConfiguration.Label
        let selected: Bool
        let ink: Color
        let bottom: JSON
        @State private var hovered = false
        var body: some View {
            label.background {
                if hovered && !selected {
                    WorkspaceTabShape(bottomLeading: CGFloat(bottom[0].number), bottomTrailing: CGFloat(bottom[1].number)).fill(ink.opacity(0.08))
                }
            }.onHover { hovered = $0 }
        }
    }
}

struct WorkspaceTabLabel: View {
    let tab: JSON
    let selected: Bool
    let palette: EditorPalette
    var fill: Color?? = .none
    var body: some View {
        HStack(spacing: 6) {
            if tab["tab"]["show_icon"].bool { PanelIcon(name: tab["icon"].string) }
            if tab["tab"]["show_name"].bool { Text(tab["title"].string).fontWeight(.bold).lineLimit(1) }
        }.padding(.horizontal, 8)
            .frame(width: tab["tab"]["show_name"].bool ? nil : 36, height: 36)
            .fixedSize(horizontal: true, vertical: false)
            .background { if selected, let color = fill ?? palette["panel"] { WorkspaceTabShape().fill(color) } }
    }
}

/// The shared tab silhouette: squircle upper corners and concave lower feet.
/// The feet extend into adjacent slots; the strip clips its outer edges.
private struct WorkspaceTabShape: Shape {
    var bottomLeading: CGFloat = -6
    var bottomTrailing: CGFloat = -6
    func path(in rect: CGRect) -> Path {
        let w = rect.width, h = rect.height, r = min(SquircleShape.surfaceRadius, w / 2, h), left = bottomLeading, right = bottomTrailing
        var path = Path()
        path.move(to: CGPoint(x: r, y: 0)); path.addLine(to: CGPoint(x: w - r, y: 0))
        path.squircle(center: CGPoint(x: w - r, y: r), start: CGVector(dx: 0, dy: -r), end: CGVector(dx: r, dy: 0))
        path.addLine(to: CGPoint(x: w, y: h - abs(right)))
        path.squircle(center: CGPoint(x: w - right, y: h - abs(right)), start: CGVector(dx: right, dy: 0), end: CGVector(dx: 0, dy: abs(right)))
        path.addLine(to: CGPoint(x: left, y: h))
        path.squircle(center: CGPoint(x: left, y: h - abs(left)), start: CGVector(dx: 0, dy: abs(left)), end: CGVector(dx: -left, dy: 0))
        path.addLine(to: CGPoint(x: 0, y: r))
        path.squircle(center: CGPoint(x: r, y: r), start: CGVector(dx: -r, dy: 0), end: CGVector(dx: 0, dy: -r))
        path.closeSubpath()
        return path.offsetBy(dx: rect.minX, dy: rect.minY)
    }
}

struct WorkspaceActiveTab: PreferenceKey {
    static var defaultValue: WorkspaceTabFrame? { nil }
    static func reduce(value: inout WorkspaceTabFrame?, nextValue: () -> WorkspaceTabFrame?) { value = nextValue() ?? value }
}

struct WorkspaceTabStrip: View {
    let shape: SquircleShape
    let tab: WorkspaceTabFrame?
    let strip: Color
    let panel: Color
    let slide: WorkspaceTabSlide
    let group: UInt64
    let active: String
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    var body: some View {
        let moving = slide.grab.flatMap { grab -> (frame: WorkspaceTabFrame, offset: CGFloat, source: Bool)? in
            guard grab.group == group, !slide.preview.isNull,
                let index = grab.panels.firstIndex(of: active), grab.frames.indices.contains(index) else { return nil }
            return (grab.frames[index], CGFloat(slide.offset(group: group, index: index)), index == grab.source)
        }
        WorkspaceTabStripFill(shape: shape, tab: moving?.frame ?? tab, strip: strip, panel: panel, offset: moving?.offset ?? 0)
            .animation(moving?.source != false || reduceMotion ? nil : .timingCurve(0, 0, 0.58, 1, duration: 0.12), value: moving?.offset ?? 0)
    }
}

private struct WorkspaceTabStripFill: View, Animatable {
    let shape: SquircleShape
    let tab: WorkspaceTabFrame?
    let strip: Color
    let panel: Color
    var offset: CGFloat
    var animatableData: CGFloat {
        get { offset }
        set { offset = newValue }
    }
    var body: some View {
        GeometryReader { allocation in
            let origin = allocation.frame(in: .named("editor-workspace")).origin
            let cut = tab.flatMap { active -> Path? in
                let clip = active.clip.offsetBy(dx: -origin.x, dy: -origin.y)
                guard !clip.isNull, !clip.isEmpty else { return nil }
                return WorkspaceTabShape().path(in: active.bounds.offsetBy(dx: offset - origin.x, dy: -origin.y))
                    .intersection(Path(clip))
            } ?? Path()
            Canvas { context, size in
                context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(strip))
                context.blendMode = .copy
                context.fill(cut, with: .color(panel))
            }.clipShape(shape)
        }
    }
}

struct WorkspaceGroupGrip: View {
    @ObservedObject var store: EditorStore
    let group: JSON
    var drawer = false
    var vertical = false
    var body: some View {
        PanelGrip(vertical: vertical).frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle()).accessibilityElement().accessibilityLabel(store.catalog["native_copy"]["tool_controls"]["more_options"].string)
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
    let cutsStrip: Bool
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
                .preference(key: WorkspaceActiveTab.self, value: cutsStrip ? WorkspaceTabFrame(bounds: full, clip: clip) : nil)
        })
    }
}
