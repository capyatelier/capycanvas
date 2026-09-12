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
            if drawer {
                GeometryReader { viewport in
                    ScrollView(.horizontal) { HStack(spacing: 0) { tabs } }
                        .scrollIndicators(.hidden)
                        .environment(\.workspaceClip, viewport.frame(in: .named("editor-workspace")).intersection(clip))
                }
            } else {
                tabs
                Spacer(minLength: 0)
            }
            WorkspaceGroupGrip(store: store, group: group["id"], drawer: drawer).frame(width: 20, height: 36)
        }.frame(height: 36).background(palette["tabbar"])
            .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "group", "group": group["id"].raw])))
    }
    @ViewBuilder private var tabs: some View {
        ForEach(group["panels"].array.indices, id: \.self) { index in
            let tab = store.snapshot["panels"].array.first { $0["id"].string == group["panels"][index].string } ?? JSON()
            let selected = tab["id"].string == group["active"].string
            Button { store.dispatch(["type": "select_panel_tab", "group": group["id"].raw, "panel": tab["id"].raw]) } label: {
                HStack(spacing: 6) {
                    if tab["tab"]["show_icon"].bool { SharedIcon(name: tab["icon"].string) }
                    if tab["tab"]["show_name"].bool { Text(tab["title"].string).fontWeight(.bold).lineLimit(1) }
                }.padding(.horizontal, 8).frame(height: 36)
                    .background(selected ? palette["panel"] : Color.clear)
                    .contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityLabel(tab["title"].string)
                .accessibilityIdentifier((drawer ? "drawer-tab-" : "panel-tab-") + tab["id"].string)
                .accessibilityAddTraits(selected ? .isSelected : [])
                .modifier(WorkspaceContext(store: store, target: JSON(["kind": "panel", "panel": tab["id"].raw])))
                .modifier(WorkspaceDrag(workspace: store.workspace, item: JSON(["kind": "panel", "panel": tab["id"].raw])))
                .modifier(WorkspaceTabMeasurement(group: group["id"].uint, index: index))
        }
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
            let bounds = allocation.frame(in: .named("editor-workspace")).intersection(clip)
            Color.clear.preference(key: WorkspaceTabs.self,
                value: enabled && !bounds.isEmpty ? ["\(group):\(index)": bounds] : [:])
        })
    }
}
