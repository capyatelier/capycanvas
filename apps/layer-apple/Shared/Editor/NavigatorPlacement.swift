import SwiftUI

/// Only native layout crosses to the render owner. Ink and camera changes do
/// not publish images, schedule preview polls or update this preference.
struct NavigatorPlacement: Equatable {
    let bounds: CGRect
    let clip: CGRect
    let image: CGRect
    let order: Int
    var json: [String: Any] {
        func rect(_ r: CGRect) -> [CGFloat] { [r.minX, r.minY, r.width, r.height] }
        return ["bounds": rect(bounds), "clip": rect(clip), "order": order]
    }
}
struct NavigatorPlacements: PreferenceKey {
    static var defaultValue: [UUID: NavigatorPlacement] { [:] }
    static func reduce(value: inout [UUID: NavigatorPlacement], nextValue: () -> [UUID: NavigatorPlacement]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}

/// Cut after this panel's background, clip and shadow, at its native stacking
/// position. Higher panels still paint above the live Metal image. The workspace
/// is the compositing boundary; the Metal canvas is a separate sibling below it.
struct NavigatorReveal: ViewModifier {
    func body(content: Content) -> some View {
        content.overlayPreferenceValue(NavigatorPlacements.self) { placements in
            GeometryReader { allocation in
                let origin = allocation.frame(in: .named("editor-workspace")).origin
                Canvas { context, _ in
                    for placement in placements.values {
                        let window = placement.image.intersection(placement.clip)
                        if !window.isNull && !window.isEmpty {
                            context.fill(Path(window.offsetBy(dx: -origin.x, dy: -origin.y)), with: .color(.white))
                        }
                    }
                }.blendMode(.destinationOut)
            }.allowsHitTesting(false).accessibilityHidden(true)
        }
    }
}
