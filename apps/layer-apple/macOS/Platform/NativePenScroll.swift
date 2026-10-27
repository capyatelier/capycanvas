import SwiftUI
import AppKit
import ObjectiveC

struct NativePenScroll: NSViewRepresentable {
    var showsIndicators = true
    var axes: Axis.Set = .vertical
    func makeNSView(context: Context) -> PenScrollView { let view = PenScrollView(); view.showsIndicators = showsIndicators; view.axes = axes; return view }
    func updateNSView(_ view: PenScrollView, context: Context) { view.showsIndicators = showsIndicators; view.axes = axes; view.attach() }
}

/// Keep one recognizer for the scroll view's lifetime, including virtualized Lists.
final class PenScrollView: NSView {
    private static var controllerKey: UInt8 = 0
    var showsIndicators = true
    var axes: Axis.Set = .vertical
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
    override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); attach() }
    override func layout() { super.layout(); attach() }
    func attach() {
        guard let scroll = enclosingScrollView else { return }
        let controller = objc_getAssociatedObject(scroll, &Self.controllerKey) as? PenScrollController ?? PenScrollController(scroll: scroll)
        objc_setAssociatedObject(scroll, &Self.controllerKey, controller, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
        controller.showsIndicators = showsIndicators; controller.axes = axes; controller.indicators()
    }
}

private final class ScrollPointerInput {
    static let shared = ScrollPointerInput()
    private(set) var mouse = false
    let controllers = NSHashTable<PenScrollController>.weakObjects()
    private var monitor: Any?
    init() {
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.mouseMoved, .leftMouseDown, .rightMouseDown, .scrollWheel, .tabletPoint, .tabletProximity]) { [weak self] event in
            guard let self else { return event }
            let next = event.pointerSubtype != .tabletPoint && event.pointerSubtype != .tabletProximity
            if mouse != next { mouse = next; for controller in controllers.allObjects { controller.indicators() } }
            return event
        }
    }
    deinit { if let monitor { NSEvent.removeMonitor(monitor) } }
}

/// AppKit supplies tablet identity and movement slop; mouse keeps native scrolling.
private final class PenScrollController: NSObject, NSGestureRecognizerDelegate {
    private weak var scroll: NSScrollView?
    private var origin = CGPoint.zero
    private var pan: NSPanGestureRecognizer!
    var showsIndicators = true
    var axes: Axis.Set = .vertical
    private var style: NSKeyValueObservation?
    init(scroll: NSScrollView) {
        self.scroll = scroll
        super.init()
        style = scroll.observe(\.scrollerStyle, options: [.initial, .new]) { [weak scroll] _, _ in
            DispatchQueue.main.async {
                guard let scroll, scroll.scrollerStyle != .overlay else { return }
                scroll.scrollerStyle = .overlay
            }
        }
        ScrollPointerInput.shared.controllers.add(self)
        pan = NSPanGestureRecognizer(target: self, action: #selector(panned)); pan.delegate = self
        pan.delaysPrimaryMouseButtonEvents = true
        scroll.addGestureRecognizer(pan)
    }
    func indicators() {
        guard let scroll else { return }
        let horizontal = axes.contains(.horizontal) && showsIndicators && ScrollPointerInput.shared.mouse
        let vertical = axes.contains(.vertical) && showsIndicators && ScrollPointerInput.shared.mouse
        if scroll.hasHorizontalScroller != horizontal { scroll.hasHorizontalScroller = horizontal }
        if scroll.hasVerticalScroller != vertical { scroll.hasVerticalScroller = vertical }
    }
    func gestureRecognizer(_ recognizer: NSGestureRecognizer, shouldAttemptToRecognizeWith event: NSEvent) -> Bool {
        guard event.type == .leftMouseDown, event.pointerSubtype == .tabletPoint, let scroll,
              let hit = scroll.hitTest(scroll.superview?.convert(event.locationInWindow, from: nil) ?? event.locationInWindow),
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
