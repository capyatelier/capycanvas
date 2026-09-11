import AppKit
import QuartzCore

/// Capture AppKit values immediately. The Rust owner receives numeric samples;
/// no NSEvent, NSView, device serial number or vendor identifier crosses queues.
@MainActor final class MacInput {
    private weak var view: MacCanvasView?
    private let store: EditorStore
    private struct Contact {
        let id: UInt64
        let device: Int?
        let tool: UInt32
        let button: UInt32
        let nativeButton: Int
        var last: [Double]
    }
    private var contact: Contact?
    private var nextContact: UInt64 = 0
    private var tools: [Int: UInt32] = [:]
    private var modifiers: NSEvent.ModifierFlags = []

    init(view: MacCanvasView, store: EditorStore) { self.view = view; self.store = store }
    private func tablet(_ event: NSEvent) -> Bool {
        event.type == .tabletPoint || event.subtype == .tabletPoint
    }
    private func position(_ event: NSEvent) -> CGPoint {
        guard let view else { return .zero }
        let point = view.convert(event.locationInWindow, from: nil)
        let scale = view.window?.backingScaleFactor ?? 1
        return CGPoint(x: point.x * scale, y: point.y * scale)
    }
    private func pack(_ event: NSEvent, phase: Double) -> [Double] {
        let point = position(event), pen = tablet(event)
        // NSEvent tilt has the same right/down axis convention as the core.
        let tilt = pen ? event.tilt : .zero
        return [point.x, point.y, pen ? Double(event.pressure) : 1,
            tilt.x * .pi / 2, tilt.y * .pi / 2,
            pen ? Double(event.rotation) * .pi / 180 : 0, 0,
            event.timestamp * 1_000_000_000, phase]
    }
    private func send(_ value: Contact, _ record: [Double]) {
        store.native?.pointer(id: value.id, tool: value.tool, button: value.button,
            records: record, predicted: false, revision: store.cameraRevision)
        view?.wake()
    }
    func mouse(_ event: NSEvent, phase: Double) {
        if event.subtype == .tabletProximity { proximity(event); return }
        updateModifiers(event.modifierFlags)
        if phase == 1 {
            guard contact == nil else { return }
            view?.window?.makeFirstResponder(view)
            nextContact &+= 1
            let device = tablet(event) ? event.deviceID : nil
            let tool = device.map { tools[$0] ?? 0 } ?? 1
            let button: UInt32 = event.buttonNumber == 0 ? 0 : event.buttonNumber <= 2 ? 1 : 2
            let value = Contact(id: nextContact, device: device, tool: tool, button: button,
                nativeButton: event.buttonNumber, last: pack(event, phase: phase))
            contact = value; send(value, value.last)
        } else if var value = contact, value.nativeButton == event.buttonNumber {
            // Proximity/hover may have completed this contact already. An
            // eventual mouseUp then has no active contact and is harmless.
            if tablet(event), value.device != event.deviceID { return }
            value.last = pack(event, phase: phase)
            send(value, value.last)
            contact = phase == 3 ? nil : value
        }
    }
    func tabletPoint(_ event: NSEvent) {
        updateModifiers(event.modifierFlags)
        let touching = event.pressure > 0 || event.buttonMask.contains(.penTip)
        if var value = contact {
            guard value.device == event.deviceID else { return }
            if !touching { finishAtLastSample(timestamp: event.timestamp); return }
            value.last = pack(event, phase: 2)
            contact = value; send(value, value.last)
        } else if touching {
            // Some drivers deliver standalone tablet events instead of mouse
            // subtypes, including pressure changes before the first drag.
            guard let view, view.bounds.contains(view.convert(event.locationInWindow, from: nil)) else { return }
            view.window?.makeFirstResponder(view)
            nextContact &+= 1
            let value = Contact(id: nextContact, device: event.deviceID,
                tool: tools[event.deviceID] ?? 0, button: 0, nativeButton: 0,
                last: pack(event, phase: 1))
            contact = value; send(value, value.last)
        } else { hover(event) }
    }
    func proximity(_ event: NSEvent) {
        if event.isEnteringProximity {
            tools[event.deviceID] = event.pointingDeviceType == .eraser ? 2 : event.pointingDeviceType == .cursor ? 1 : 0
        } else {
            if contact?.device == event.deviceID { finishAtLastSample(timestamp: event.timestamp) }
            tools.removeValue(forKey: event.deviceID)
            clearHover()
        }
    }
    func hover(_ event: NSEvent) {
        if event.subtype == .tabletProximity { proximity(event); return }
        if contact != nil {
            if tablet(event), event.deviceID == contact?.device, event.pressure == 0,
                !event.buttonMask.contains(.penTip) {
                finishAtLastSample(timestamp: event.timestamp)
            } else { return }
        }
        let tool: UInt32 = tablet(event) ? tools[event.deviceID] ?? 0 : 1
        store.native?.pointer(id: 0, tool: tool, button: 0, records: pack(event, phase: 0),
            predicted: false, revision: store.cameraRevision)
        view?.wake()
    }
    private func finishAtLastSample(timestamp: TimeInterval) {
        guard let value = contact else { return }
        var terminal = value.last
        terminal[7] = max(terminal[7], timestamp * 1_000_000_000); terminal[8] = 3
        contact = nil; send(value, terminal)
    }
    func clearHover() {
        guard contact == nil else { return } // Pointer capture survives view exit.
        store.native?.pointer(id: 0, tool: 1, button: 0,
            records: [0, 0, 0, 0, 0, 0, 0, CACurrentMediaTime() * 1_000_000_000, 4],
            predicted: false, revision: store.cameraRevision)
        view?.wake()
    }
    func blur() {
        // Focus loss is an explicit shared interruption policy. Native hover
        // and proximity exit above are normal lift, never cancellation.
        contact = nil; modifiers = []
        store.input(["type": "blur"])
    }
    func scroll(_ event: NSEvent) {
        guard contact == nil else { return }
        let point = position(event), unit: CGFloat = event.hasPreciseScrollingDeltas ? 1 : 16
        store.native?.scroll(x: Float(point.x), y: Float(point.y),
            dx: Float(-event.scrollingDeltaX * unit), dy: Float(-event.scrollingDeltaY * unit),
            scale: Float(view?.window?.backingScaleFactor ?? 1),
            zoom: event.modifierFlags.contains(.control), horizontal: event.modifierFlags.contains(.shift))
        view?.wake()
    }
    func gesture(_ event: NSEvent, rotate: Bool) {
        guard contact == nil else { return }
        let point = position(event)
        store.native?.gesture(x: Float(point.x), y: Float(point.y),
            scale: rotate ? 1 : Float(max(0.01, 1 + event.magnification)),
            rotation: rotate ? -event.rotation * .pi / 180 : 0)
        view?.wake()
    }
    func key(_ event: NSEvent, pressed: Bool) {
        let names: [UInt16: String] = [36: "Enter", 48: "Tab", 51: "Backspace", 53: "Escape",
            76: "Enter", 115: "Home", 116: "PageUp", 117: "Delete", 119: "End", 121: "PageDown",
            123: "ArrowLeft", 124: "ArrowRight", 125: "ArrowDown", 126: "ArrowUp"]
        let key = names[event.keyCode] ?? event.charactersIgnoringModifiers ?? ""
        guard !key.isEmpty else { return }
        sendKey(key, pressed: pressed, repeatKey: event.isARepeat, flags: event.modifierFlags)
    }
    func updateModifiers(_ next: NSEvent.ModifierFlags) {
        for (flag, name): (NSEvent.ModifierFlags, String) in [(.shift, "Shift"), (.control, "Control"), (.option, "Alt"), (.command, "Meta")] {
            if next.contains(flag) != modifiers.contains(flag) {
                sendKey(name, pressed: next.contains(flag), repeatKey: false, flags: next)
            }
        }
        modifiers = next
    }
    private func sendKey(_ key: String, pressed: Bool, repeatKey: Bool, flags: NSEvent.ModifierFlags) {
        store.input(["type": "key", "key": key, "pressed": pressed, "repeat": repeatKey,
            "modifiers": ["command": !flags.intersection([.command, .control]).isEmpty,
                "alt": flags.contains(.option), "shift": flags.contains(.shift)]])
    }
}
