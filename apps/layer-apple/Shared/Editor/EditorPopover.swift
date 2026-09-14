import SwiftUI

enum EditorPopoverPlacement { case below, inward }

extension View {
    func editorPopover<Popup: View>(isPresented: Binding<Bool>, placement: EditorPopoverPlacement = .below,
        @ViewBuilder content: () -> Popup) -> some View {
        modifier(EditorPopoverSource(isPresented: isPresented, placement: placement, popup: content()))
    }
}

private struct EditorPopoverSource<Popup: View>: ViewModifier {
    @Binding var isPresented: Bool
    let placement: EditorPopoverPlacement
    let popup: Popup
    @State private var id = UUID()
    func body(content: Content) -> some View {
        let presented = isPresented
        return content.transformAnchorPreference(key: EditorPopovers.self, value: .bounds) { requests, anchor in
            if presented {
                requests.append(EditorPopoverRequest(id: id, anchor: anchor, placement: placement,
                    content: AnyView(popup), dismiss: { isPresented = false }))
            }
        }
    }
}

private struct EditorPopoverRequest {
    let id: UUID
    let anchor: Anchor<CGRect>
    let placement: EditorPopoverPlacement
    let content: AnyView
    let dismiss: () -> Void
}
private struct EditorPopovers: PreferenceKey {
    static var defaultValue: [EditorPopoverRequest] { [] }
    static func reduce(value: inout [EditorPopoverRequest], nextValue: () -> [EditorPopoverRequest]) { value += nextValue() }
}

/// Install once at the editor or sheet root, outside clipped panel stacks.
/// An ordinary overlay preserves the held contact and needs no presentation
/// controller, extra window, native menu handoff, or animation override.
struct EditorPopoverHost: ViewModifier {
    func body(content: Content) -> some View {
        content.overlayPreferenceValue(EditorPopovers.self) { requests in
            GeometryReader { geometry in
                if let request = requests.last {
                    EditorPopoverLayer(request: request, source: geometry[request.anchor], viewport: geometry.size)
                        .id(request.id)
                }
            }
        }.transformPreference(EditorPopovers.self) { $0.removeAll() }
    }
}

private struct EditorPopoverLayer: View {
    let request: EditorPopoverRequest
    let source: CGRect
    let viewport: CGSize
    @State private var size = CGSize(width: 340, height: 300)
    @Environment(\.scenePhase) private var phase
    private var origin: CGPoint {
        let x: CGFloat, y: CGFloat
        if request.placement == .inward {
            x = source.midX > viewport.width / 2 ? source.minX - size.width - 6 : source.maxX + 6
            y = source.midY - min(24, size.height / 2)
        } else {
            x = source.minX
            y = source.maxY + size.height + 6 <= viewport.height - 8 ? source.maxY + 6 : source.minY - size.height - 6
        }
        return CGPoint(x: max(8, min(x, viewport.width - size.width - 8)),
            y: max(8, min(y, viewport.height - size.height - 8)))
    }
    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.clear.contentShape(Rectangle()).onTapGesture { request.dismiss() }.accessibilityHidden(true)
            request.content
                .frame(maxWidth: max(0, viewport.width - 16), maxHeight: max(0, viewport.height - 16))
                .fixedSize(horizontal: true, vertical: true)
                .onGeometryChange(for: CGSize.self) { $0.size } action: { size = $0 }
                .modifier(EditorPopupSurface(shape: RoundedRectangle(cornerRadius: 8)))
                .overlay { RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.15), lineWidth: 0.5).allowsHitTesting(false) }
                .shadow(color: .black.opacity(0.18), radius: 5, y: 2)
                .offset(x: origin.x, y: origin.y)
                .accessibilityAddTraits(.isModal)
        }.onKeyPress(.escape) { request.dismiss(); return .handled }
            .onChange(of: phase) { _, phase in if phase != .active { request.dismiss() } }
    }
}
