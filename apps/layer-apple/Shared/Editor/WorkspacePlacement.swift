import SwiftUI

/// Apply absolute motion outside the retained panel body. Moving the native
/// view also moves its input, clip and live Navigator allocations together.
struct WorkspacePlacement: ViewModifier {
    let motion: WorkspaceMotion
    let group: JSON
    let bounds: JSON
    var clipsGroup = false
    func body(content: Content) -> some View {
        let moved = motion.position(group["id"].uint)
        let base = group["bounds"].rect
        let dx = moved.isNull ? 0 : moved.rect.minX - base.minX
        let dy = moved.isNull ? 0 : moved.rect.minY - base.minY
        let placed = bounds.rect.offsetBy(dx: dx, dy: dy)
        content.environment(\.workspaceClip, clipsGroup ? placed : .infinite)
            .frame(width: max(0, placed.width), height: max(0, placed.height), alignment: .topLeading)
            .offset(x: placed.minX, y: placed.minY)
    }
}

/// Only this overlay observes the current drop hint; panel contents retain
/// their normal model subscriptions throughout a drag.
struct WorkspaceDropIndicator: View {
    let workspace: WorkspacePresentation
    let palette: EditorPalette
    var body: some View {
        if !workspace.dropHint.isNull {
            RoundedRectangle(cornerRadius: 2).fill(palette.accent)
                .background { RoundedRectangle(cornerRadius: 3).fill(.black.opacity(0.2)).padding(-1) }
                .placed(workspace.dropHint["bounds"]).allowsHitTesting(false).accessibilityHidden(true)
        }
    }
}
