import SwiftUI

/// Visual copies sit above panels and below drop indicators. The real
/// buttons keep their original layout, accessibility and input measurements.
struct WorkspaceTabSlideOverlay: View {
    @ObservedObject var store: EditorStore
    let slide: WorkspaceTabSlide
    var body: some View {
        Group {
            if let grab = slide.grab, !slide.preview.isNull {
                let groups = store.snapshot["layout"]["groups"].array
                    + store.contentDrawers.items.values.map { $0.model["tabs"].replacing("id", with: $0.model["tabs"]["group"]).replacing("tab_presentations", with: $0.geometry["placement"]["tab_presentations"]) }
                let active = groups.first { $0["id"].uint == grab.group }?["active"].string ?? ""
                let drawer = store.state["customization"]["column_drawers"].array.contains { $0["tabs"]["group"].uint == grab.group }
                let panels = store.workspace.presentedTabs(group: groups.first { $0["id"].uint == grab.group } ?? JSON(), panels: grab.panels.map { store.panel($0) })
                WorkspaceTabSlideContent(slide: slide, grab: grab, panels: panels, active: active, drawer: drawer, palette: EditorPalette(source: store.state["palette"]))
                    .id(grab.id)
            }
        }.onChange(of: slide.preview.stableKey, initial: true) { _, _ in store.glass.panelSlideChanged() }
    }
}

private struct WorkspaceTabSlideContent: View {
    let slide: WorkspaceTabSlide
    let grab: WorkspaceTabSlide.Grab
    let panels: [JSON]
    let active: String
    let drawer: Bool
    let palette: EditorPalette
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var appeared = false
    var body: some View {
        ZStack(alignment: .topLeading) {
            ForEach(grab.frames.indices, id: \.self) { index in
                let tab = panels[index], frame = grab.frames[index].bounds
                let selected = tab["id"].string == active
                let source = index == grab.source
                // Establish the neighbors' original positions even when
                // down and the first preview arrive in one SwiftUI update.
                let offset = source || appeared ? slide.offset(group: grab.group, index: index) : 0
                WorkspaceTabLabel(tab: tab, selected: selected, palette: palette, fill: .some(drawer ? palette.glassPanel : nil))
                    .frame(width: frame.width, height: frame.height)
                    .offset(x: frame.minX - grab.clip.minX + offset, y: frame.minY - grab.clip.minY)
                    .animation(source || reduceMotion ? nil : .timingCurve(0, 0, 0.58, 1, duration: 0.12), value: offset)
                    .zIndex(source ? 2 : selected ? 1 : 0)
            }
        }.frame(width: grab.clip.width, height: grab.clip.height, alignment: .topLeading)
            .clipped().offset(x: grab.clip.minX, y: grab.clip.minY)
            .allowsHitTesting(false).accessibilityHidden(true)
            .onAppear { appeared = true }
    }
}
