import SwiftUI

/// Optional direct-capture geometry. Ordinary editor views do not create
/// measurement readers or publish preferences for these header controls.
private struct MeasureHeaderControls: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    var measureHeaderControls: Bool {
        get { self[MeasureHeaderControls.self] }
        set { self[MeasureHeaderControls.self] = newValue }
    }
}
struct HeaderControlFrames: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}
struct HeaderControlMeasurement: ViewModifier {
    let id: String
    @Environment(\.measureHeaderControls) private var enabled
    func body(content: Content) -> some View {
        if enabled {
            content.background(GeometryReader { geometry in
                Color.clear.preference(key: HeaderControlFrames.self,
                    value: [id: geometry.frame(in: .named("editor-workspace"))])
            })
        } else { content }
    }
}
