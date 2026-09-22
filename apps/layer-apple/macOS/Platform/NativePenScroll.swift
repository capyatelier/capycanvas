import SwiftUI
import AppKit
import ObjectiveC

struct NativePenScroll: NSViewRepresentable {
    func makeNSView(context: Context) -> PenScrollView { PenScrollView() }
    func updateNSView(_ view: PenScrollView, context: Context) { view.attach() }
}

/// Keep one recognizer for the scroll view's lifetime, including virtualized Lists.
final class PenScrollView: NSView {
    private static var controllerKey: UInt8 = 0
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); attach() }
    override func layout() { super.layout(); attach() }
    func attach() {
        guard let scroll = enclosingScrollView,
              objc_getAssociatedObject(scroll, &Self.controllerKey) == nil else { return }
        objc_setAssociatedObject(scroll, &Self.controllerKey, PenScrollController(scroll: scroll), .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
    }
}

/// AppKit supplies tablet identity and movement slop; mouse keeps native scrolling.
private final class PenScrollController: NSObject, NSGestureRecognizerDelegate {
    private weak var scroll: NSScrollView?
    private var origin = CGPoint.zero
    private var pan: NSPanGestureRecognizer!
    init(scroll: NSScrollView) {
        self.scroll = scroll
        super.init()
        pan = NSPanGestureRecognizer(target: self, action: #selector(panned)); pan.delegate = self
        pan.delaysPrimaryMouseButtonEvents = true
        scroll.addGestureRecognizer(pan)
    }
    func gestureRecognizer(_ recognizer: NSGestureRecognizer, shouldAttemptToRecognizeWith event: NSEvent) -> Bool {
        guard event.type == .leftMouseDown, event.subtype == .tabletPoint, let scroll,
              let hit = scroll.hitTest(scroll.convert(event.locationInWindow, from: nil)),
              hit.enclosingScrollView === scroll, !(hit is NSTextView), !(hit is NSControl) else { return false }
        origin = scroll.contentView.bounds.origin
        return true
    }
    func gestureRecognizerShouldBegin(_ recognizer: NSGestureRecognizer) -> Bool {
        guard let scroll, let document = scroll.documentView else { return false }
        let delta = pan.translation(in: scroll.contentView), viewport = scroll.contentView.bounds
        return abs(delta.y) >= abs(delta.x) ? document.bounds.height > viewport.height
            : document.bounds.width > viewport.width
    }
    func gestureRecognizer(_ recognizer: NSGestureRecognizer, shouldRequireFailureOf other: NSGestureRecognizer) -> Bool {
        (other.delegate as? ReorderInputView)?.precedesScrolling(other) == true
    }
    @objc private func panned(_ recognizer: NSPanGestureRecognizer) {
        guard let scroll, let document = scroll.documentView else { return }
        guard [.began, .changed, .ended].contains(recognizer.state) else { return }
        let clip = scroll.contentView, delta = recognizer.translation(in: clip)
        clip.scroll(to: CGPoint(x: max(0, min(max(0, document.bounds.width - clip.bounds.width), origin.x - delta.x)),
            y: max(0, min(max(0, document.bounds.height - clip.bounds.height), origin.y - delta.y))))
        scroll.reflectScrolledClipView(clip)
    }
}
