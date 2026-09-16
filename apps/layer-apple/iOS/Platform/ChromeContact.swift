import SwiftUI
import UIKit

extension View {
    func editorChromeContact(_ action: @escaping (CGPoint) -> Void) -> some View {
        background(EditorContact(action: action))
    }
}

private struct EditorContact: UIViewRepresentable {
    let action: (CGPoint) -> Void
    func makeUIView(context: Context) -> EditorContactView { EditorContactView() }
    func updateUIView(_ view: EditorContactView, context: Context) { view.action = action }
    static func dismantleUIView(_ view: EditorContactView, coordinator: ()) { view.detach() }
}

private final class EditorContactView: UIView, UIGestureRecognizerDelegate {
    var action: (CGPoint) -> Void = { _ in }
    private let observer = UITapGestureRecognizer()
    private weak var attached: UIView?
    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        observer.cancelsTouchesInView = false
        observer.delegate = self
    }
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
    override func didMoveToSuperview() { super.didMoveToSuperview(); attach() }
    override func didMoveToWindow() { super.didMoveToWindow(); attach() }
    private func attach() {
        let next = window == nil ? nil : superview
        guard next !== attached else { return }
        detach(); attached = next; next?.addGestureRecognizer(observer)
    }
    func detach() { attached?.removeGestureRecognizer(observer); attached = nil }
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        // Canvas input reports its own contact and can consume it for dismissal.
        // Observe chrome before button activation without recognizing a gesture.
        if !(touch.view is CanvasView) { action(touch.location(in: self)) }
        return false
    }
}
