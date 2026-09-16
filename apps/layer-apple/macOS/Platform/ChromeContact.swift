import SwiftUI
import AppKit

extension View {
    func editorChromeContact(_ action: @escaping (CGPoint) -> Void) -> some View {
        simultaneousGesture(SpatialTapGesture(coordinateSpace: .named("editor-workspace"))
            .onEnded { action($0.location) })
    }
}
