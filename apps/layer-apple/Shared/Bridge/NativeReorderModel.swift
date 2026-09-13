import Foundation
import CoreGraphics

/// The host supplies native timing, device identity and contact delivery; each
/// editor projection supplies measured sources and shared transaction callbacks.
@MainActor protocol NativeReorderModel: AnyObject {
    var contact: ReorderContact { get }
    var viewport: CGRect { get set }
    var enabled: Bool { get }
    var usesNativeRowMenus: Bool { get }
    func nativeMenu(at point: CGPoint) -> NativeReorderMenu?
    func nativeDragChanged(_ active: Bool)
    func source(at point: CGPoint) -> ReorderTarget?
    func acceptsContext(at point: CGPoint) -> Bool
    func context(at point: CGPoint)
    func recognizeHold()
    func cancel()
    func nativeInputDetached()
}

extension NativeReorderModel {
    var usesNativeRowMenus: Bool { false }
    func nativeMenu(at point: CGPoint) -> NativeReorderMenu? { nil }
    func nativeDragChanged(_ active: Bool) {}
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

@MainActor struct NativeReorderMenu {
    let id: String
    let bounds: CGRect
    let load: AppleContextMenuRequest
    init(id: String, bounds: CGRect, content: AppleContextMenu) {
        self.id = id; self.bounds = bounds; load = { $0(content) }
    }
    init(id: String, bounds: CGRect, load: @escaping AppleContextMenuRequest) {
        self.id = id; self.bounds = bounds; self.load = load
    }
}
