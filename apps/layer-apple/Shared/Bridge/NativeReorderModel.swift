import Foundation
import CoreGraphics

/// The host supplies native timing, device identity and contact delivery; each
/// editor projection supplies measured sources and shared transaction callbacks.
@MainActor protocol NativeReorderModel: AnyObject {
    var contact: ReorderContact { get }
    var viewport: CGRect { get set }
    var enabled: Bool { get }
    func source(at point: CGPoint) -> ReorderTarget?
    func acceptsContext(at point: CGPoint) -> Bool
    func context(at point: CGPoint)
    func recognizeHold()
    func cancel()
    func nativeInputDetached()
    var swiping: Bool { get }
    func beginSwipe(at point: CGPoint) -> Bool
    func moveSwipe(to point: CGPoint)
    func finishSwipe(cancelled: Bool)
    var edgeScroll: ReorderEdgeScroll? { get }
}

struct ReorderEdgeScroll {
    var inside: CGFloat = 20
    var outside: CGFloat = 12
    var speed: CGFloat = 240
    func delta(at point: CGPoint, in viewport: CGRect, elapsed: TimeInterval) -> CGFloat {
        guard point.x >= viewport.minX, point.x <= viewport.maxX else { return 0 }
        let step = speed * CGFloat(min(0.05, max(0, elapsed)))
        if point.y < viewport.minY + inside && point.y >= viewport.minY - outside { return -step }
        if point.y > viewport.maxY - inside && point.y <= viewport.maxY + outside { return step }
        return 0
    }
}

extension NativeReorderModel {
    var swiping: Bool { false }
    func beginSwipe(at point: CGPoint) -> Bool { false }
    func moveSwipe(to point: CGPoint) {}
    func finishSwipe(cancelled: Bool) {}
    var edgeScroll: ReorderEdgeScroll? { nil }
    func recognizeHold() { if !swiping { contact.recognizeHold() } }
    func nativeInputDetached() {
        // Native recognizers are already detached. SwiftUI can still own its
        // graph exclusively; defer publication and preserve a remounted contact.
        let generation = contact.generation
        DispatchQueue.main.async { [self] in
            if contact.generation == generation { cancel() }
        }
    }
}
