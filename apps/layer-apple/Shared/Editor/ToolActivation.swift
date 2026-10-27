import SwiftUI
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

@MainActor enum ToolActivation {
    private static var last: (store: ObjectIdentifier, anchor: String, source: String, time: TimeInterval)?
    #if os(iOS)
    static var pointer: (source: String, time: TimeInterval, token: UUID)?
    #endif
    static func activate(_ control: JSON, anchor: [String: Any], doubleClick: Bool, store: EditorStore, normally: () -> Void) {
        let picker = control["kind"].string == "color_picker"
            || control["kind"].string == "command" && control["command"].string == "eyedropper"
        guard picker || doubleClick, let pointer = pointerActivation else { last = nil; normally(); return }
        let key = JSON(anchor).stableKey
        if let last, last.store == ObjectIdentifier(store), last.anchor == key, last.source == pointer.source,
            pointer.time >= last.time, pointer.time - last.time <= interval {
            self.last = nil
            if picker { store.dispatch(["type": "color_picker", "action": ["kind": "settings", "anchor": anchor]]) }
            else { store.dispatch(["type": "double_click_tool", "control": control.raw]) }
        } else {
            last = (ObjectIdentifier(store), key, pointer.source, pointer.time)
            normally()
        }
    }
    private static var interval: TimeInterval {
        #if canImport(AppKit)
        NSEvent.doubleClickInterval
        #else
        0.35
        #endif
    }
    private static var pointerActivation: (source: String, time: TimeInterval)? {
        #if canImport(AppKit)
        guard let event = NSApp.currentEvent, [.leftMouseUp, .leftMouseDown, .tabletPoint].contains(event.type) else { return nil }
        return (event.pointerSubtype == .tabletPoint ? "pen" : "mouse", event.timestamp)
        #else
        defer { pointer = nil }
        return pointer.map { ($0.source, $0.time) }
        #endif
    }
}

extension View {
    func tracksToolActivation() -> some View {
        #if os(iOS)
        background(ToolPointerInput().allowsHitTesting(false))
        #else
        self
        #endif
    }
}

#if os(iOS)
private struct ToolPointerInput: UIViewRepresentable {
    func makeUIView(context: Context) -> ObserverView { ObserverView() }
    func updateUIView(_ view: ObserverView, context: Context) {}
    static func dismantleUIView(_ view: ObserverView, coordinator: ()) { view.detach() }
    final class ObserverView: UIView {
        let observer = Pointer()
        private weak var attached: UIWindow?
        override init(frame: CGRect) {
            super.init(frame: frame); isUserInteractionEnabled = false
            observer.coordinates = self; observer.cancelsTouchesInView = false
            observer.delaysTouchesBegan = false; observer.delaysTouchesEnded = false
            observer.requiresExclusiveTouchType = false
            observer.allowedTouchTypes = [UITouch.TouchType.direct, .pencil, .indirectPointer].map { NSNumber(value: $0.rawValue) }
        }
        required init?(coder: NSCoder) { fatalError("Use init(frame:)") }
        override func didMoveToWindow() {
            super.didMoveToWindow(); detach(); attached = window; attached?.addGestureRecognizer(observer)
        }
        func detach() { observer.retire(); attached?.removeGestureRecognizer(observer); attached = nil }
    }
    final class Pointer: UIGestureRecognizer {
        weak var coordinates: UIView?
        private weak var touch: UITouch?
        private var token: UUID?
        override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
            guard let coordinates, let native = touches.first, native.window === coordinates.window,
                coordinates.bounds.contains(native.location(in: coordinates)), event.buttonMask.isEmpty || event.buttonMask.contains(.primary) else { return }
            retire(); touch = native; let token = UUID(); self.token = token
            ToolActivation.pointer = (native.type == .pencil ? "pen" : native.type == .indirectPointer ? "mouse" : "touch", native.timestamp, token)
        }
        override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
            guard let touch, touches.contains(touch), let token else { return }
            self.touch = nil
            DispatchQueue.main.async {
                if ToolActivation.pointer?.token == token { ToolActivation.pointer = nil }
            }
        }
        override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { if let touch, touches.contains(touch) { retire() } }
        override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent) { ToolActivation.pointer = nil }
        override func canPrevent(_ preventedGestureRecognizer: UIGestureRecognizer) -> Bool { false }
        override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool { false }
        func retire() { if ToolActivation.pointer?.token == token { ToolActivation.pointer = nil }; touch = nil; token = nil }
    }
}
#endif
