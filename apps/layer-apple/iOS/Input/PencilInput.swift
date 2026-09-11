import UIKit

struct PencilContact {
    let id: UInt64
    let tool: UInt32
    var lastTimestamp: TimeInterval = -1
    var last: [Double] = []
}

extension CanvasView {
    func route(_ touches: Set<UITouch>, event: UIEvent?, phase: Double) {
        let ordered = touches.sorted { $0.timestamp < $1.timestamp }
        for touch in ordered {
            let key = ObjectIdentifier(touch)
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
                contacts[key] = PencilContact(id: nextContact, tool: tool)
            }
            guard var contact = contacts[key] else { continue }
            let history = event?.coalescedTouches(for: touch) ?? []
            let samples = history.isEmpty ? [touch] : history
            var records: [Double] = []
            records.reserveCapacity(samples.count * 9)
            for (index, sample) in samples.enumerated() {
                let terminal = phase >= 3 && index == samples.count - 1
                guard sample.timestamp > contact.lastTimestamp || terminal else { continue }
                let samplePhase = terminal ? phase : contact.lastTimestamp < 0 ? 1.0 : 2.0
                let record = pack(sample, phase: samplePhase)
                records.append(contentsOf: record)
                contact.last = record
                contact.lastTimestamp = sample.timestamp
            }
            if !records.isEmpty { send(contact, records: records, predicted: false) }
            if phase >= 3 {
                contacts.removeValue(forKey: key)
            } else {
                contacts[key] = contact
                if contact.tool == 0, let predicted = event?.predictedTouches(for: touch), !predicted.isEmpty {
                    send(contact, records: predicted.flatMap { pack($0, phase: 2) }, predicted: true)
                }
            }
        }
        wake()
    }
    private func pack(_ touch: UITouch, phase: Double) -> [Double] {
        let point = touch.preciseLocation(in: self)
        let pencil = touch.type == .pencil
        let pressure = pencil && touch.maximumPossibleForce > 0 ? touch.force / touch.maximumPossibleForce : 1
        let altitude = pencil ? touch.altitudeAngle : .pi / 2
        let azimuth = pencil ? touch.azimuthAngle(in: self) : 0
        let tiltX = atan2(cos(altitude) * cos(azimuth), sin(altitude))
        let tiltY = atan2(cos(altitude) * sin(azimuth), sin(altitude))
        return [Double(point.x * contentScaleFactor), Double(point.y * contentScaleFactor),
            Double(pressure), Double(tiltX), Double(tiltY), pencil ? Double(touch.rollAngle) : 0, 0,
            touch.timestamp * 1_000_000_000, phase]
    }
    private func send(_ contact: PencilContact, records: [Double], predicted: Bool) {
        store.native?.pointer(id: contact.id, tool: contact.tool, button: 0, records: records,
            predicted: predicted, revision: store.cameraRevision)
    }
    @objc func hovered(_ recognizer: UIHoverGestureRecognizer) {
        guard contacts.values.allSatisfy({ $0.tool != 0 }) else { return }
        let point = recognizer.location(in: self)
        let altitude = recognizer.altitudeAngle
        let azimuth = recognizer.azimuthAngle(in: self)
        let record: [Double] = [point.x * contentScaleFactor, point.y * contentScaleFactor, 0,
            atan2(cos(altitude) * cos(azimuth), sin(altitude)),
            atan2(cos(altitude) * sin(azimuth), sin(altitude)),
            recognizer.rollAngle, recognizer.zOffset,
            CACurrentMediaTime() * 1_000_000_000, recognizer.state == .ended ? 4 : 0]
        store.native?.pointer(id: 0, tool: 0, button: 0, records: record, predicted: false, revision: store.cameraRevision)
        wake()
    }
    func routeKeys(_ presses: Set<UIPress>, pressed: Bool) {
        for press in presses {
            guard let key = press.key else { continue }
            let flags = key.modifierFlags
            store.input(["type": "key", "key": key.charactersIgnoringModifiers, "pressed": pressed,
                "modifiers": ["command": flags.contains(.command), "alt": flags.contains(.alternate), "shift": flags.contains(.shift)]])
        }
    }
}
