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
}

extension NativeReorderModel {
    var swiping: Bool { false }
    func beginSwipe(at point: CGPoint) -> Bool { false }
    func moveSwipe(to point: CGPoint) {}
    func finishSwipe(cancelled: Bool) {}
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
