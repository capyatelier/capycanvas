import AppKit

@MainActor final class BubbleInputCapture {
    private weak var marker: ReorderInputView?
    private var model: BubbleInteraction? { marker?.model?.bubbleInput }
    private var monitor: Any?
    private var timer: Timer?
    private var forwardedDown: Bool?
    init(marker: ReorderInputView) { self.marker = marker }
    func attach() {
        guard monitor == nil, model != nil, marker?.window != nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp, .rightMouseDown, .mouseMoved, .mouseExited, .keyDown]) { [weak self] event in
            guard let self else { return event }; return self.handle(event)
        }
    }
    func detach() {
        timer?.invalidate(); timer = nil; forwardedDown = nil
        if let monitor { NSEvent.removeMonitor(monitor); self.monitor = nil }
    }
    deinit { if let monitor { NSEvent.removeMonitor(monitor) } }
    private func handle(_ event: NSEvent) -> NSEvent? {
        guard event.window === marker?.window, let marker, let model else { return event }
        model.viewport = marker.bounds.size
        switch event.type {
        case .mouseExited:
            model.hover(nil); NSCursor.setHiddenUntilMouseMoves(false); return event
        case .keyDown:
            if event.keyCode == 53 && model.active { model.cancel(); return nil }
            return event
        default: break
        }
        let point = marker.convert(event.locationInWindow, from: nil)
        let device: ReorderDevice = event.pointerSubtype == .tabletPoint ? .pen : .mouse
        defer { NSCursor.setHiddenUntilMouseMoves(model.hidesPointer(at: point, pen: device == .pen)) }
        model.hold = { [weak self] in self?.timer?.invalidate(); self?.timer = nil }
        switch event.type {
        case .leftMouseDown:
            forwardedDown = true
            guard marker.bounds.contains(point) else { return event }
            let admission = model.down(point, id: 1, device: device, clicks: event.clickCount,
                doubleTime: NSEvent.doubleClickInterval, time: event.timestamp)
            forwardedDown = admission != .capture
            if admission == .capture && model.canHold {
                timer = Timer.scheduledTimer(withTimeInterval: NSPressGestureRecognizer().minimumPressDuration * model.holdMultiplier, repeats: false) { [weak self] _ in
                    MainActor.assumeIsolated { self?.model?.recognizeHold() }
                }
            }
            return admission == .capture ? nil : event
        case .leftMouseDragged: return model.move(point, id: 1) ? nil : event
        case .leftMouseUp:
            let forwarded = forwardedDown; forwardedDown = nil
            _ = model.up(point, id: 1, cancelled: false)
            DispatchQueue.main.async { _ = model.consumeClick() }
            return forwarded == false ? nil : event
        case .rightMouseDown:
            return model.context(at: point) ? nil : event
        case .mouseMoved: model.hover(marker.bounds.contains(point) ? point : nil)
        default: break
        }
        return event
    }
}
