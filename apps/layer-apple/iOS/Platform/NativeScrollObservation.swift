import SwiftUI
import UIKit

struct NativeScrollObservation: View {
    var position: EditorScrollPosition?
    var offsetChanged: ((CGFloat) -> Void)?
    var activity: (() -> Void)?
    private let request: EditorScrollPosition.Request?
    init(position: EditorScrollPosition? = nil, offsetChanged: ((CGFloat) -> Void)? = nil, activity: (() -> Void)? = nil) {
        self.position = position; self.offsetChanged = offsetChanged; self.activity = activity
        request = position?.request
    }
    var body: some View {
        ScrollObservationHost(position: position, offsetChanged: offsetChanged, activity: activity, request: request).allowsHitTesting(false)
    }
}

private struct ScrollObservationHost: UIViewRepresentable {
    let position: EditorScrollPosition?
    let offsetChanged: ((CGFloat) -> Void)?
    let activity: (() -> Void)?
    let request: EditorScrollPosition.Request?
    func makeUIView(context: Context) -> ScrollObservationView { ScrollObservationView() }
    func updateUIView(_ view: ScrollObservationView, context: Context) {
        view.position = position; view.offsetChanged = offsetChanged; view.activity = activity; view.attach(); view.restore(request)
    }
    static func dismantleUIView(_ view: ScrollObservationView, coordinator: ()) { view.detach() }
}

final class ScrollObservationView: UIView {
    weak var position: EditorScrollPosition?
    var offsetChanged: ((CGFloat) -> Void)?
    var activity: (() -> Void)?
    private weak var scroll: UIScrollView?
    private var observation: NSKeyValueObservation?
    private var generation: UInt64 = 0
    private var scheduled: UInt64?
    private var active = false
    private var lastOffset: CGFloat?
    private var retainedOffset: CGFloat?
    override init(frame: CGRect) { super.init(frame: frame); isUserInteractionEnabled = false }
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
    override func didMoveToWindow() { super.didMoveToWindow(); attach() }
    override func layoutSubviews() { super.layoutSubviews(); attach() }
    func attach() {
        guard window != nil else { detach(); return }
        var node = superview
        while let view = node {
            if let next = view as? UIScrollView {
                if next !== scroll {
                    let offset = position?.offset
                    detach(); scroll = next; retainedOffset = offset
                    next.panGestureRecognizer.addTarget(self, action: #selector(panned))
                    observation = next.observe(\.contentOffset, options: [.initial, .new]) { [weak self] _, _ in
                        MainActor.assumeIsolated { self?.changed() }
                    }
                }
                if retainedOffset != nil { changed() }
                if let request = position?.request, case .offset = request.target { changed() }
                return
            }
            node = view.superview
        }
        detach()
    }
    func detach() {
        scroll?.panGestureRecognizer.removeTarget(self, action: #selector(panned))
        observation?.invalidate(); observation = nil; scroll = nil; active = false; lastOffset = nil; retainedOffset = nil
        generation &+= 1; scheduled = nil
    }
    func restore(_ request: EditorScrollPosition.Request?) {
        switch request?.target {
        case .offset: changed()
        case .item: retainedOffset = nil
        case nil: break
        }
    }
    @objc private func panned(_ recognizer: UIPanGestureRecognizer) {
        let next = recognizer.state == .began || recognizer.state == .changed
        if next && !active { activity?() }
        active = next
    }
    private func changed() {
        guard scheduled == nil else { return }
        let generation = generation; scheduled = generation
        DispatchQueue.main.async { [weak self] in
            guard let self, generation == self.generation else { return }; scheduled = nil
            guard let scroll, window != nil else { return }
            let request = position?.request
            let requestedOffset: CGFloat?
            if let request, case .offset(let y) = request.target { requestedOffset = y }
            else { requestedOffset = retainedOffset }
            if let y = requestedOffset {
                guard scroll.bounds.height > 0, scroll.contentSize.height > 0 else { return }
                retainedOffset = nil
                let minimum = -scroll.adjustedContentInset.top
                let maximum = max(minimum, scroll.contentSize.height - scroll.bounds.height + scroll.adjustedContentInset.bottom)
                scroll.setContentOffset(CGPoint(x: scroll.contentOffset.x, y: min(maximum, max(minimum, y + minimum))), animated: false)
                if let request, case .offset = request.target { position?.applied(request) }
            }
            let offset = max(0, scroll.contentOffset.y + scroll.adjustedContentInset.top)
            position?.offset = offset
            if offset != lastOffset { lastOffset = offset; offsetChanged?(offset) }
            let next = scroll.isTracking || scroll.isDragging || scroll.isDecelerating
            if next && !active { activity?() }
            active = next
        }
    }
}
