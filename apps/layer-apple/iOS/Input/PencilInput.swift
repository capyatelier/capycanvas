import UIKit

extension CanvasView: UIPointerInteractionDelegate {
    func pointerInteraction(_ interaction: UIPointerInteraction, styleFor region: UIPointerRegion) -> UIPointerStyle? {
        // Rust renders the selected cursor, including the None hover fallback.
        .hidden()
    }
}

struct PickerHold {
    let key: ObjectIdentifier
    let start: CGPoint
    let work: DispatchWorkItem
}

struct PencilContact {
    let id: UInt64
    let tool: UInt32
    let button: UInt32
    var lastTimestamp: TimeInterval = -1
    var last: [Double] = []
}

extension CanvasView {
    func installIndirectGestures() {
        let scroll = UIPanGestureRecognizer(target: self, action: #selector(scrolled(_:)))
        scroll.allowedScrollTypesMask = .all
        for recognizer in [scroll,
            UIPinchGestureRecognizer(target: self, action: #selector(pinched(_:))),
            UIRotationGestureRecognizer(target: self, action: #selector(rotated(_:)))] {
            // The app opts into indirect events. Finger/Pencil contacts keep
            // their existing shared routing; these handle scroll/transform events.
            recognizer.allowedTouchTypes = []
            recognizer.delegate = self
            addGestureRecognizer(recognizer)
        }
    }
    @objc func scrolled(_ recognizer: UIPanGestureRecognizer) {
        let delta = recognizer.translation(in: self)
        recognizer.setTranslation(.zero, in: self)
        guard contacts.isEmpty, [.began, .changed, .ended].contains(recognizer.state) else { return }
        let point = recognizer.location(in: self), scale = contentScaleFactor
        store.native?.scroll(x: Float(point.x * scale), y: Float(point.y * scale),
            dx: Float(-delta.x), dy: Float(-delta.y), scale: Float(scale),
            zoom: recognizer.modifierFlags.contains(.control), horizontal: recognizer.modifierFlags.contains(.shift))
        wake()
    }
    @objc func pinched(_ recognizer: UIPinchGestureRecognizer) {
        let scale = recognizer.scale
        recognizer.scale = 1
        guard contacts.isEmpty, [.began, .changed, .ended].contains(recognizer.state) else { return }
        let point = recognizer.location(in: self)
        store.native?.gesture(x: Float(point.x * contentScaleFactor), y: Float(point.y * contentScaleFactor),
            scale: Float(scale), rotation: 0)
        wake()
    }
    @objc func rotated(_ recognizer: UIRotationGestureRecognizer) {
        let rotation = recognizer.rotation
        recognizer.rotation = 0
        guard contacts.isEmpty, [.began, .changed, .ended].contains(recognizer.state) else { return }
        let point = recognizer.location(in: self)
        store.native?.gesture(x: Float(point.x * contentScaleFactor), y: Float(point.y * contentScaleFactor),
            scale: 1, rotation: Float(rotation))
        wake()
    }
    func route(_ touches: Set<UITouch>, event: UIEvent?, phase: Double) {
        if phase == 1 { store.layerSwipe.close(); store.workspace.dismissTransients(at: nil) }
        if let event { updateModifiers(event.modifierFlags, force: phase == 1) }
        let ordered = touches.sorted { $0.timestamp < $1.timestamp }
        for touch in ordered {
            let key = ObjectIdentifier(touch)
            // A fresh contact may reuse an interrupted touch's identity.
            if phase == 1 { ignoredContacts.remove(key) }
            if ignoredContacts.contains(key) {
                if phase >= 3 { ignoredContacts.remove(key) }
                continue
            }
            if phase == 1 {
                let tool: UInt32 = touch.type == .pencil ? 0 : touch.type == .direct ? 3 : 1
                if tool == 3 && contacts.values.contains(where: { $0.tool == 0 }) {
                    ignoredContacts.insert(key)
                    continue
                }
                if tool == 0 {
                    // Reject fingers already resting on the screen as Pencil begins.
                    for (other, contact) in contacts where contact.tool == 3 {
                        var terminal = contact.last
                        if terminal.count == 9 {
                            terminal[7] = touch.timestamp * 1_000_000_000; terminal[8] = 4
                            send(contact, records: terminal, predicted: false)
                        }
                        contacts.removeValue(forKey: other); ignoredContacts.insert(other)
                    }
                }
                nextContact &+= 1
                // Keep the press button through release, when UIKit's mask is
                // empty. Rust owns primary paint, right/middle pan and other buttons.
                var button: UInt32 = 0
                if touch.type == .indirectPointer, let buttons = event?.buttonMask, !buttons.isEmpty {
                    button = buttons.contains(.primary) ? 0
                        : buttons.contains(.secondary) || buttons.contains(.button(3)) ? 1 : 2
                }
                contacts[key] = PencilContact(id: nextContact, tool: tool, button: button)
                cancelPickerHold()
                if tool == 3 && contacts.count == 1 { armPickerHold(key, id: nextContact, at: touch.preciseLocation(in: self)) }
            } else if let hold = pickerHold, hold.key == key {
                let point = touch.preciseLocation(in: self)
                if phase >= 3 || hypot(point.x - hold.start.x, point.y - hold.start.y) > 10 { cancelPickerHold() }
            }
            guard var contact = contacts[key] else { continue }
            let history = event?.coalescedTouches(for: touch) ?? []
            let samples = history.isEmpty ? [touch] : history
            var records: [Double] = []
            var updates: [UInt64] = []
            let revision = store.cameraRevision
            records.reserveCapacity(samples.count * 9)
            for (index, sample) in samples.enumerated() {
                let terminal = phase >= 3 && index == samples.count - 1
                guard sample.timestamp > contact.lastTimestamp || terminal else { continue }
                let samplePhase = terminal ? phase : contact.lastTimestamp < 0 ? 1.0 : 2.0
                let record = pack(sample, phase: samplePhase)
                let capture = estimates.capture(key: sample.estimationUpdateIndex?.uint64Value, contact: contact.id,
                    revision: revision, scale: contentScaleFactor, record: record,
                    expected: contact.tool == 0 ? UInt64(sample.estimatedPropertiesExpectingUpdates.rawValue) : 0)
                if let released = capture.released { sendCorrection(released) }
                updates.append(contentsOf: capture.metadata)
                records.append(contentsOf: record)
                contact.last = record
                contact.lastTimestamp = sample.timestamp
            }
            if !records.isEmpty {
                store.native?.pointer(id: contact.id, tool: contact.tool, button: contact.button, records: records,
                    predicted: false, revision: revision, updates: updates)
            }
            if phase >= 3 {
                contacts.removeValue(forKey: key)
                if phase == 4 { estimates.cancel(contact: contact.id) }
            } else {
                contacts[key] = contact
                if contact.tool == 0, let predicted = event?.predictedTouches(for: touch), !predicted.isEmpty {
                    send(contact, records: predicted.flatMap { pack($0, phase: 2) }, predicted: true)
                }
            }
        }
        wake()
    }
    private func pack(_ touch: UITouch, phase: Double, scale: CGFloat? = nil) -> [Double] {
        let point = touch.preciseLocation(in: self)
        let pencil = touch.type == .pencil
        let pressure = pencil && touch.maximumPossibleForce > 0 ? touch.force / touch.maximumPossibleForce : 1
        let altitude = pencil ? touch.altitudeAngle : .pi / 2
        let azimuth = pencil ? touch.azimuthAngle(in: self) : 0
        let tiltX = atan2(cos(altitude) * cos(azimuth), sin(altitude))
        let tiltY = atan2(cos(altitude) * sin(azimuth), sin(altitude))
        let scale = scale ?? contentScaleFactor
        return [Double(point.x * scale), Double(point.y * scale),
            Double(pressure), Double(tiltX), Double(tiltY), pencil ? Double(touch.rollAngle) : 0, 0,
            touch.timestamp * 1_000_000_000, phase]
    }
    func updateEstimates(_ touches: Set<UITouch>) {
        for touch in touches {
            guard let key = touch.estimationUpdateIndex?.uint64Value, let captured = estimates.pending[key],
                let update = estimates.correct(key: key,
                    record: pack(touch, phase: captured.record[8], scale: captured.scale),
                    expected: UInt64(touch.estimatedPropertiesExpectingUpdates.rawValue)) else { continue }
            sendCorrection(update)
        }
        wake()
    }
    private func sendCorrection(_ sample: EstimatedInput.Sample) {
        store.native?.pointer(id: sample.contact, tool: 0, button: 0, records: sample.record,
            predicted: false, revision: sample.revision, updates: sample.metadata, correction: true)
    }
    func finishEstimates() { for sample in estimates.finish() { sendCorrection(sample) } }
    private func armPickerHold(_ key: ObjectIdentifier, id: UInt64, at start: CGPoint) {
        let work = DispatchWorkItem { [weak self] in
            guard let self, pickerHold?.key == key, let contact = contacts[key], contact.last.count == 9 else { return }
            pickerHold = nil
            store.input(["type": "color_picker_hold", "id": id, "position": [contact.last[0], contact.last[1]],
                "offset": 44 * Double(contentScaleFactor)])
            wake()
        }
        pickerHold = PickerHold(key: key, start: start, work: work)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5, execute: work)
    }
    func cancelPickerHold() {
        pickerHold?.work.cancel()
        pickerHold = nil
    }
    func interruptContacts() {
        cancelPickerHold()
        finishEstimates()
        ignoredContacts.formUnion(contacts.keys)
        contacts.removeAll()
        modifiers = []
    }
    private func send(_ contact: PencilContact, records: [Double], predicted: Bool) {
        store.native?.pointer(id: contact.id, tool: contact.tool, button: contact.button, records: records,
            predicted: predicted, revision: store.cameraRevision)
    }
    @objc func hovered(_ recognizer: UIHoverGestureRecognizer) {
        guard contacts.values.allSatisfy({ $0.tool != 0 }) else { return }
        updateModifiers(recognizer.modifierFlags)
        if recognizer.state == .ended || recognizer.state == .cancelled { leave(); return }
        let point = recognizer.location(in: self)
        let altitude = recognizer.altitudeAngle
        let azimuth = recognizer.azimuthAngle(in: self)
        let record: [Double] = [point.x * contentScaleFactor, point.y * contentScaleFactor, 0,
            atan2(cos(altitude) * cos(azimuth), sin(altitude)),
            atan2(cos(altitude) * sin(azimuth), sin(altitude)),
            recognizer.rollAngle, recognizer.zOffset,
            CACurrentMediaTime() * 1_000_000_000, 0]
        store.native?.pointer(id: 0, tool: 0, button: 0, records: record, predicted: false, revision: store.cameraRevision)
        wake()
    }
    private func leave() {
        store.input(["type": "cursor_leave"])
        wake()
    }
    @objc func mouseHovered(_ recognizer: UIHoverGestureRecognizer) {
        guard contacts.isEmpty else { return }
        updateModifiers(recognizer.modifierFlags)
        if recognizer.state == .ended || recognizer.state == .cancelled { leave(); return }
        let point = recognizer.location(in: self)
        let record: [Double] = [point.x * contentScaleFactor, point.y * contentScaleFactor,
            1, 0, 0, 0, 0, CACurrentMediaTime() * 1_000_000_000, 0]
        store.native?.pointer(id: 0, tool: 1, button: 0, records: record, predicted: false, revision: store.cameraRevision)
        wake()
    }
    func routeKeys(_ presses: Set<UIPress>, pressed: Bool) {
        for press in presses {
            guard let key = press.key else { continue }
            if AppleKeyName.name(key) == "Tab" && key.modifierFlags.contains(.control) {
                if pressed { store.drawingTabs.adjacent(!key.modifierFlags.contains(.shift)) }
                continue
            }
            modifiers = key.modifierFlags
            sendKey(AppleKeyName.name(key), pressed: pressed, flags: modifiers)
        }
    }
    private func updateModifiers(_ next: UIKeyModifierFlags, force: Bool = false) {
        // A modifier may change while another native control owns key focus.
        // Refresh every flag at contact start: other controls can forward keys
        // directly to shared input without updating this canvas's cached flags.
        for (flag, name): (UIKeyModifierFlags, String) in [(.shift, "Shift"), (.control, "Control"), (.alternate, "Alt"), (.command, "Meta")] {
            if force || next.contains(flag) != modifiers.contains(flag) {
                sendKey(name, pressed: next.contains(flag), flags: next)
            }
        }
        modifiers = next
    }
    private func sendKey(_ key: String, pressed: Bool, flags: UIKeyModifierFlags) {
        store.input(["type": "key", "key": key, "pressed": pressed,
            "modifiers": ["command": !flags.intersection([.command, .control]).isEmpty,
                "alt": flags.contains(.alternate), "shift": flags.contains(.shift)]])
    }
}

extension CanvasView: UIGestureRecognizerDelegate {
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive event: UIEvent) -> Bool {
        contacts.isEmpty && (event.type == .scroll || event.type == .transform)
    }
    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer) -> Bool {
        (gestureRecognizer is UIPinchGestureRecognizer && otherGestureRecognizer is UIRotationGestureRecognizer)
            || (gestureRecognizer is UIRotationGestureRecognizer && otherGestureRecognizer is UIPinchGestureRecognizer)
    }
}
