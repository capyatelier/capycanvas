import SwiftUI

/// Native scroll behavior, including pen panning before a row hold wins.
struct EditorScrollView<Content: View>: View {
    var axes: Axis.Set
    var showsIndicators: Bool
    var content: Content
    init(_ axes: Axis.Set = .vertical, showsIndicators: Bool = true, @ViewBuilder content: () -> Content) {
        self.axes = axes; self.showsIndicators = showsIndicators; self.content = content()
    }
    var body: some View {
        ScrollView(axes, showsIndicators: showsIndicators) { content.background(NativePenScroll().frame(width: 0, height: 0)) }
    }
}
