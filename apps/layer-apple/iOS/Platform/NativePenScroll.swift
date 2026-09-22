import SwiftUI
import UIKit

struct NativePenScroll: UIViewRepresentable {
    func makeUIView(context: Context) -> PenScrollView { PenScrollView() }
    func updateUIView(_ view: PenScrollView, context: Context) { view.attach() }
}

final class PenScrollView: UIView {
    override init(frame: CGRect) { super.init(frame: frame); isUserInteractionEnabled = false }
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
    override func didMoveToWindow() { super.didMoveToWindow(); attach() }
    override func layoutSubviews() { super.layoutSubviews(); attach() }
    func attach() {
        var node = superview
        while let view = node {
            if let next = view as? UIScrollView {
                let pencil = NSNumber(value: UITouch.TouchType.pencil.rawValue)
                if !next.panGestureRecognizer.allowedTouchTypes.contains(pencil) {
                    next.panGestureRecognizer.allowedTouchTypes.append(pencil)
                }
                return
            }
            node = view.superview
        }
    }
}
