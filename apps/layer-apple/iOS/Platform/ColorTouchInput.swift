import UIKit

final class ColorTouchRecognizer: UIGestureRecognizer {
    weak var surface: ColorTouchSurface?
    private var held = Set<ObjectIdentifier>()
    init(surface: ColorTouchSurface) {
        self.surface = surface
        super.init(target: nil, action: nil)
        allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]
        cancelsTouchesInView = false; delaysTouchesBegan = false; delaysTouchesEnded = false
    }
    static func position(_ touch: UITouch) -> CGPoint {
        let point = touch.location(in: nil), scale = touch.window?.screen.scale ?? 1
        return CGPoint(x: point.x * scale, y: point.y * scale)
    }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        for touch in touches {
            held.insert(ObjectIdentifier(touch))
            surface?.contact(ObjectIdentifier(touch), phase: "down", position: Self.position(touch), timestamp: touch.timestamp)
        }
        if surface?.claimed == true { state = state == .possible ? .began : .changed }
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        for touch in touches {
            surface?.contact(ObjectIdentifier(touch), phase: "move", position: Self.position(touch), timestamp: touch.timestamp)
        }
        if state == .began || state == .changed { state = .changed }
    }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) { finish(touches, phase: "up") }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { finish(touches, phase: "cancel") }
    private func finish(_ touches: Set<UITouch>, phase: String) {
        for touch in touches {
            held.remove(ObjectIdentifier(touch))
            surface?.contact(ObjectIdentifier(touch), phase: phase, position: Self.position(touch), timestamp: touch.timestamp)
        }
        guard held.isEmpty else { return }
        let recognized = state == .began || state == .changed
        state = recognized ? (phase == "cancel" ? .cancelled : .ended) : .failed
    }
    override func reset() {
        for key in held { surface?.contact(key, phase: "cancel", position: .zero, timestamp: ProcessInfo.processInfo.systemUptime) }
        held.removeAll()
        super.reset()
    }
    override func canPrevent(_ preventedGestureRecognizer: UIGestureRecognizer) -> Bool { surface?.claimed == true }
    override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool { false }
}
