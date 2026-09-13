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
}

extension NativeReorderModel {
    func recognizeHold() { contact.recognizeHold() }
    func nativeInputDetached() {
        // Native recognizers are already detached. SwiftUI can still own its
        // graph exclusively; defer publication and preserve a remounted contact.
        let generation = contact.generation
        DispatchQueue.main.async { [self] in
            if contact.generation == generation { cancel() }
        }
    }
}
