import SwiftUI

/// Visual copies sit above panels and below drop indicators. The real
/// buttons keep their original layout, accessibility and input measurements.
struct WorkspaceTabSlideOverlay: View {
    @ObservedObject var store: EditorStore
    let slide: WorkspaceTabSlide
    var body: some View {
        if let grab = slide.grab, !slide.preview.isNull {
            WorkspaceTabSlideContent(slide: slide, grab: grab, palette: EditorPalette(source: store.state["palette"]))
                .id(grab.id)
        }
    }
}

private struct WorkspaceTabSlideContent: View {
    let slide: WorkspaceTabSlide
    let grab: WorkspaceTabSlide.Grab
    let palette: EditorPalette
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var appeared = false
    var body: some View {
        ZStack(alignment: .topLeading) {
            ForEach(grab.frames.indices, id: \.self) { index in
                let tab = grab.panels[index], frame = grab.frames[index].bounds
                let selected = tab["id"].string == grab.active
                let source = index == grab.source
                // Establish the neighbors' original positions even when
                // down and the first preview arrive in one SwiftUI update.
                let offset = source || appeared ? slide.offset(group: grab.group, index: index) : 0
                WorkspaceTabLabel(tab: tab, selected: selected, palette: palette)
                    .frame(width: frame.width, height: frame.height)
                    .background(UnevenRoundedRectangle(topLeadingRadius: 6, bottomLeadingRadius: 0,
                        bottomTrailingRadius: 0, topTrailingRadius: 6).fill(palette["tabbar"]))
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
