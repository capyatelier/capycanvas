import SwiftUI

struct EditorScrollView<Content: View>: View {
    var axes: Axis.Set
    var showsIndicators: Bool
    var offsetChanged: ((CGFloat) -> Void)?
    var content: Content
    init(_ axes: Axis.Set = .vertical, showsIndicators: Bool = true, offsetChanged: ((CGFloat) -> Void)? = nil, @ViewBuilder content: () -> Content) {
        self.axes = axes; self.showsIndicators = showsIndicators; self.offsetChanged = offsetChanged; self.content = content()
    }
    @ViewBuilder var body: some View {
        if let offsetChanged {
            if #available(iOS 18.0, macOS 15.0, *) {
                scroll.onScrollGeometryChange(for: CGFloat.self) { max(0, $0.contentOffset.y + $0.contentInsets.top) } action: { _, next in offsetChanged(next) }
            } else {
                #if os(iOS)
                ScrollView(axes, showsIndicators: showsIndicators) {
                    content.background(NativePenScroll(showsIndicators: showsIndicators, axes: axes).frame(width: 0, height: 0))
                        .background(NativeScrollObservation(offsetChanged: { offsetChanged(max(0, $0)) }))
                }
                #endif
            }
        } else { scroll }
    }
    private var scroll: some View {
        ScrollView(axes, showsIndicators: showsIndicators) { content.background(NativePenScroll(showsIndicators: showsIndicators, axes: axes).frame(width: 0, height: 0)) }
    }
}

@MainActor final class EditorScrollPosition: ObservableObject {
    enum Target: Equatable { case offset(CGFloat), item(UInt64) }
    struct Request: Equatable { let id = UUID(); let target: Target }
    @Published private(set) var request: Request?
    var offset: CGFloat = 0
    func scrollTo(y: CGFloat) { request = Request(target: .offset(y)) }
    func scrollTo(id: UInt64) { request = Request(target: .item(id)) }
    func applied(_ request: Request) { if self.request?.id == request.id { self.request = nil } }
}

struct EditorPositionedScrollView<Content: View>: View {
    @ObservedObject var position: EditorScrollPosition
    var offsetChanged: ((CGFloat) -> Void)? = nil
    @ViewBuilder var content: () -> Content
    @ViewBuilder var body: some View {
        if #available(iOS 18.0, macOS 15.0, *) {
            PositionedScrollView(position: position, offsetChanged: offsetChanged, content: content)
        } else {
            #if os(iOS)
            ScrollViewReader { proxy in
                EditorScrollView {
                    content().background(NativeScrollObservation(position: position, offsetChanged: offsetChanged))
                }.onChange(of: position.request, initial: true) { _, request in
                    if let request, case .item(let id) = request.target { proxy.scrollTo(id); position.applied(request) }
                }
            }
            #endif
        }
    }
}

@available(iOS 18.0, macOS 15.0, *)
private struct PositionedScrollView<Content: View>: View {
    @ObservedObject var position: EditorScrollPosition
    @ViewBuilder var content: () -> Content
    @State private var scrollPosition: ScrollPosition
    let offsetChanged: ((CGFloat) -> Void)?
    init(position: EditorScrollPosition, offsetChanged: ((CGFloat) -> Void)?, @ViewBuilder content: @escaping () -> Content) {
        self.position = position; self.offsetChanged = offsetChanged; self.content = content
        _scrollPosition = State(initialValue: ScrollPosition(y: position.offset))
    }
    var body: some View {
        EditorScrollView { content() }.scrollPosition($scrollPosition)
            .onScrollGeometryChange(for: CGFloat.self) { max(0, $0.contentOffset.y + $0.contentInsets.top) } action: { _, next in position.offset = next; offsetChanged?(next) }
            .onChange(of: position.request, initial: true) { _, request in
                switch request?.target {
                case .offset(let y): scrollPosition.scrollTo(y: y)
                case .item(let id): scrollPosition.scrollTo(id: id)
                case nil: break
                }
                if let request { position.applied(request) }
            }
    }
}

struct EditorScrollActivity: ViewModifier {
    let action: () -> Void
    @ViewBuilder func body(content: Content) -> some View {
        if #available(iOS 18.0, macOS 15.0, *) {
            content.onScrollPhaseChange { _, phase in if phase != .idle { action() } }
        } else {
            #if os(iOS)
            content.background(NativeScrollObservation(activity: action))
            #endif
        }
    }
}
