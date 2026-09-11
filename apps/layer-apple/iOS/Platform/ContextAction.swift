import SwiftUI

extension View {
    func editorContextAction(_ action: @escaping () -> Void) -> some View {
        highPriorityGesture(LongPressGesture(minimumDuration: 0.5, maximumDistance: 10)
            .onEnded { _ in action() })
    }
}
