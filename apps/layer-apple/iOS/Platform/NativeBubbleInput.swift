import UIKit

final class BubbleRecognizer: UIGestureRecognizer {
    private weak var marker: ReorderInputView?
    private var contacts: [ObjectIdentifier: UInt64] = [:]
    private var next: UInt64 = 0
    private var timer: Timer?
    private var secondary = false
    private var colorKeys = Set<ObjectIdentifier>()
    private var model: BubbleInteraction? { marker?.model?.bubbleInput }
    init(marker: ReorderInputView) {
        self.marker = marker; super.init(target: nil, action: nil)
        cancelsTouchesInView = true; delaysTouchesBegan = false; delaysTouchesEnded = false
        allowedTouchTypes = [UITouch.TouchType.direct, .pencil, .indirectPointer].map { NSNumber(value: $0.rawValue) }
    }
    override func canPrevent(_ other: UIGestureRecognizer) -> Bool { secondary || model?.captures == true }
    override func canBePrevented(by other: UIGestureRecognizer) -> Bool { !contacts.isEmpty ? false : super.canBePrevented(by: other) }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let marker, let model else { state = .failed; return }
        model.viewport = marker.bounds.size
        model.claim = { [weak self] id in
            guard let self, self.contacts.values.contains(id) else { return }
            switch self.state {
            case .possible: self.state = .began
            case .began, .changed: self.state = .changed
            default: break
            }
        }
        for touch in touches {
            let point = touch.location(in: marker)
            if touch.type == .direct, model.colorTouch(at: point) {
                colorKeys.insert(ObjectIdentifier(touch))
                model.colorTouches.contact(ObjectIdentifier(touch), phase: "down",
                    position: ColorTouchRecognizer.position(touch), timestamp: touch.timestamp)
            }
            guard marker.bounds.contains(point) || model.active else { continue }
            next &+= 1
            let device: ReorderDevice = touch.type == .pencil ? .pen : touch.type == .indirectPointer ? .mouse : .touch
            if event.buttonMask.contains(.secondary) {
                if model.context(at: point) {
                    secondary = true; contacts[ObjectIdentifier(touch)] = next
                    state = state == .possible ? .began : .changed
                }
                continue
            }
            let result = model.down(point, id: next, device: device, clicks: touch.tapCount,
                doubleTime: 0, time: touch.timestamp)
            marker.focusBubbleKeyboard()
            guard result != .ignore else {
                if colorKeys.contains(ObjectIdentifier(touch)) { contacts[ObjectIdentifier(touch)] = next }
                continue
            }
            contacts[ObjectIdentifier(touch)] = next
            if result == .capture { state = state == .possible ? .began : .changed }
        }
        guard !contacts.isEmpty else { state = .failed; return }
        model.hold = { [weak self] in self?.timer?.invalidate(); self?.timer = nil }
        if model.canHold {
            let interval = UILongPressGestureRecognizer().minimumPressDuration * model.holdMultiplier
            timer = Timer.scheduledTimer(withTimeInterval: interval, repeats: false) { [weak self] _ in
                MainActor.assumeIsolated { self?.model?.recognizeHold() }
            }
        }
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let marker, let model else { return }
        var capture = false
        for touch in touches {
            model.colorTouches.contact(ObjectIdentifier(touch), phase: "move",
                position: ColorTouchRecognizer.position(touch), timestamp: touch.timestamp)
            guard let id = contacts[ObjectIdentifier(touch)] else { continue }
            for sample in event.coalescedTouches(for: touch) ?? [touch] {
                capture = model.move(sample.location(in: marker), id: id) || capture
            }
        }
        if capture { state = state == .possible ? .began : .changed }
    }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) { finish(touches, cancelled: false) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) { finish(touches, cancelled: true) }
    private func finish(_ touches: Set<UITouch>, cancelled: Bool) {
        guard let marker else { return }
        for touch in touches {
            colorKeys.remove(ObjectIdentifier(touch))
            model?.colorTouches.contact(ObjectIdentifier(touch), phase: cancelled ? "cancel" : "up",
                position: ColorTouchRecognizer.position(touch), timestamp: touch.timestamp)
            guard let id = contacts.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            _ = model?.up(touch.location(in: marker), id: id, cancelled: cancelled)
        }
        if contacts.isEmpty {
            state = state == .possible ? .failed : cancelled ? .cancelled : .ended
            if let model { DispatchQueue.main.async { _ = model.consumeClick() } }
        }
    }
    func cancel() { timer?.invalidate(); timer = nil; contacts.removeAll(); if state == .began || state == .changed { state = .cancelled } }
    override func reset() {
        for key in colorKeys { model?.colorTouches.contact(key, phase: "cancel", position: .zero, timestamp: ProcessInfo.processInfo.systemUptime) }
        colorKeys.removeAll()
        super.reset(); timer?.invalidate(); timer = nil; contacts.removeAll(); secondary = false
    }
}
