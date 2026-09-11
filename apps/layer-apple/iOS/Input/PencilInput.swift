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
            var updates: [UInt64] = []
            let revision = store.cameraRevision
            records.reserveCapacity(samples.count * 9)
            for (index, sample) in samples.enumerated() {
                let terminal = phase >= 3 && index == samples.count - 1
                guard sample.timestamp > contact.lastTimestamp || terminal else { continue }
                let samplePhase = terminal ? phase : contact.lastTimestamp < 0 ? 1.0 : 2.0
                let record = pack(sample, phase: samplePhase)
                let capture = estimates.capture(key: estimateKey(sample), contact: contact.id,
                    revision: revision, scale: contentScaleFactor, record: record,
                    expected: contact.tool == 0 ? UInt64(sample.estimatedPropertiesExpectingUpdates.rawValue) : 0)
                if let released = capture.released { sendCorrection(released) }
                updates.append(contentsOf: capture.metadata)
                records.append(contentsOf: record)
                contact.last = record
                contact.lastTimestamp = sample.timestamp
            }
            if !records.isEmpty {
                store.native?.pointer(id: contact.id, tool: contact.tool, button: 0, records: records,
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
    private func estimateKey(_ touch: UITouch) -> EstimatedInput.Key? {
        touch.estimationUpdateIndex.map { EstimatedInput.Key(index: $0.uint64Value, timestamp: touch.timestamp) }
    }
    func updateEstimates(_ touches: Set<UITouch>) {
        for touch in touches {
            guard let key = estimateKey(touch), let captured = estimates.pending[key],
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
    func interruptContacts() {
        finishEstimates()
        ignoredContacts.formUnion(contacts.keys)
        contacts.removeAll()
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
            store.input(["type": "key", "key": AppleKeyName.name(key), "pressed": pressed,
                "modifiers": ["command": !flags.intersection([.command, .control]).isEmpty, "alt": flags.contains(.alternate), "shift": flags.contains(.shift)]])
        }
    }
}
