import SwiftUI
import UIKit

extension View {
    func nativeEditorContextMenu(identity: String, load: @escaping AppleContextMenuRequest,
        visibility: @escaping (Bool) -> Void, region: ((CGPoint, CGSize) -> Bool)? = nil) -> some View {
        modifier(EditorContextSource(identity: identity, load: load, visibility: visibility, region: region))
    }
}

private struct EditorContextSource: ViewModifier {
    let identity: String
    let load: AppleContextMenuRequest
    let visibility: (Bool) -> Void
    let region: ((CGPoint, CGSize) -> Bool)?
    @State private var menu: AppleContextMenu?
    @State private var request = UUID()
    @State private var point = CGPoint.zero
    func body(content: Content) -> some View {
        NativeContextMenuHost(content: content, open: open, cancel: close, region: region)
            .overlay {
                GeometryReader { _ in
                    Color.clear.frame(width: 1, height: 1)
                        .editorPopover(isPresented: Binding(get: { menu != nil }, set: { if !$0 { close() } }), placement: .below) {
                            if let menu { EditorActionMenu(model: menu, dismiss: close) }
                        }.position(x: point.x + 0.5, y: point.y + 0.5)
                }.allowsHitTesting(false)
            }
            .onChange(of: identity) { _, _ in close() }
            .onDisappear { close() }
    }
    private func open(_ location: CGPoint) {
        point = location
        let ticket = UUID(); request = ticket
        load { result in
            guard request == ticket else { return }
            menu = result; visibility(result != nil)
        }
    }
    private func close() { request = UUID(); menu = nil; visibility(false) }
}

private struct NativeMenuContent<Content: View>: View {
    let content: Content
    let values: EnvironmentValues
    var body: some View {
        content.environment(\.editorPopupStore, values.editorPopupStore)
            .environment(\.colorScheme, values.colorScheme)
            .environment(\.isEnabled, values.isEnabled)
            .font(values.font).tint(values.editorPalette.accent).environment(\.editorPalette, values.editorPalette)
            .foregroundStyle(values.editorPopupStore.map { EditorPalette(source: $0.state["palette"])["text"] } ?? .primary)
    }
}

/// UIKit supplies device classification and standard hold/secondary-click
/// recognition. The shared SwiftUI helper owns all menu layout and actions.
private struct NativeContextMenuHost<Content: View>: UIViewControllerRepresentable {
    let content: Content
    let open: (CGPoint) -> Void
    let cancel: () -> Void
    let region: ((CGPoint, CGSize) -> Bool)?
    func makeUIViewController(context: Context) -> NativeContextMenuController<Content> {
        NativeContextMenuController(rootView: NativeMenuContent(content: content, values: context.environment))
    }
    func updateUIViewController(_ controller: NativeContextMenuController<Content>, context: Context) {
        controller.open = open; controller.cancel = cancel; controller.region = region
        controller.rootView = NativeMenuContent(content: content, values: context.environment)
    }
    func sizeThatFits(_ proposal: ProposedViewSize, uiViewController: NativeContextMenuController<Content>, context: Context) -> CGSize? {
        uiViewController.sizeThatFits(in: CGSize(width: proposal.width ?? UIView.layoutFittingExpandedSize.width,
            height: proposal.height ?? UIView.layoutFittingExpandedSize.height))
    }
}

private final class NativeContextMenuController<Content: View>: UIHostingController<NativeMenuContent<Content>>, UIGestureRecognizerDelegate {
    var open: (CGPoint) -> Void = { _ in }
    var cancel: () -> Void = {}
    var region: ((CGPoint, CGSize) -> Bool)?
    private lazy var hold = UILongPressGestureRecognizer(target: self, action: #selector(held))
    private lazy var secondary = UITapGestureRecognizer(target: self, action: #selector(clicked))
    override func viewDidLoad() {
        super.viewDidLoad(); view.backgroundColor = .clear
        safeAreaRegions = []; sizingOptions = .intrinsicContentSize
        hold.delegate = self; hold.allowedTouchTypes = [UITouch.TouchType.direct, .pencil].map { NSNumber(value: $0.rawValue) }
        secondary.delegate = self; secondary.buttonMaskRequired = .secondary
        view.addGestureRecognizer(hold); view.addGestureRecognizer(secondary)
    }
    @objc private func held(_ recognizer: UILongPressGestureRecognizer) {
        if recognizer.state == .began {
            let point = recognizer.location(in: view)
            (view.hitTest(point, with: nil) as? ColorWheelInput.ContactView)?.contact.cancel()
            open(point)
        }
        else if recognizer.state == .cancelled { cancel() }
    }
    @objc private func clicked(_ recognizer: UITapGestureRecognizer) { open(recognizer.location(in: view)) }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        region.map { $0(touch.location(in: view), view.bounds.size) } ?? true
    }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldReceive event: UIEvent) -> Bool {
        recognizer === secondary ? event.buttonMask.contains(.secondary) : !event.buttonMask.contains(.secondary)
    }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
        recognizer === hold && other is ColorWheelInput.Contact
    }
    func gestureRecognizer(_ recognizer: UIGestureRecognizer, shouldBeRequiredToFailBy other: UIGestureRecognizer) -> Bool {
        recognizer === hold && other.view === recognizer.view
            && !(other is UIPanGestureRecognizer) && !(other is UIPinchGestureRecognizer)
    }

}
