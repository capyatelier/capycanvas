import Foundation
import CoreGraphics

enum EdgeReachContact: String { case free, pickup, input }

struct EdgeReachSource {
    let contact: EdgeReachContact
    let select: (() -> Void)?
    let containsControls: Bool
    init(contact: EdgeReachContact, select: (() -> Void)? = nil, containsControls: Bool = false) {
        self.contact = contact; self.select = select; self.containsControls = containsControls
    }
}

struct EdgeReachAdmission {
    let vertical: Bool
}

struct EdgeReachState: Equatable {
    let pending: Bool
    let hold: Bool
    let claimed: Bool
    let selectOnRelease: Bool
    let cursor: String?
}

@MainActor final class EdgeReachCoordinator {
    private struct Contact {
        let generation: UInt64
        let id: UInt64
        let device: ReorderDevice
        let contact: EdgeReachContact
        let viewport: Any
        var point: CGPoint
        var sequence: UInt64 = 0
        var applied: UInt64 = 0
        var state: EdgeReachState
        var select: (() -> Void)?
        let update: (EdgeReachState?) -> Void
        let claim: (String) -> Void
    }
    private weak var store: EditorStore?
    private var generation: UInt64 = 0
    private var current: Contact?

    init(store: EditorStore) { self.store = store }

    func admission(at point: CGPoint, device: ReorderDevice, contact: EdgeReachContact) -> EdgeReachAdmission? {
        guard let layout = store?.snapshot["layout"], !layout["viewport"].isNull,
              let request = try? JSON(["layout": layout.raw, "position": [point.x, point.y],
                  "kind": device.rawValue, "contact": contact.rawValue]).encoded() else { return nil }
        let result = request.withCString { capy_apple_edge_reach($0) }
        return result & 1 == 0 ? nil : EdgeReachAdmission(vertical: result & 2 != 0)
    }

    @discardableResult func begin(id: UInt64, admission: EdgeReachAdmission, device: ReorderDevice,
        contact: EdgeReachContact, at point: CGPoint, timeNs: UInt64, select: (() -> Void)? = nil,
        update: @escaping (EdgeReachState?) -> Void = { _ in }, claim: @escaping (String) -> Void) -> EdgeReachState {
        cancel()
        generation &+= 1
        let initial = EdgeReachState(pending: true, hold: admission.vertical, claimed: false,
            selectOnRelease: false, cursor: nil)
        current = Contact(generation: generation, id: id, device: device, contact: contact,
            viewport: store?.snapshot["layout"]["viewport"].raw ?? [0, 0], point: point,
            state: initial, select: select, update: update, claim: claim)
        update(initial)
        send(phase: "down", timeNs: timeNs)
        return initial
    }

    @discardableResult func move(id: UInt64, to point: CGPoint, timeNs: UInt64) -> EdgeReachState? {
        guard var value = current, value.id == id else { return nil }
        value.point = point; current = value
        if value.state.pending || value.state.claimed { send(phase: "move", timeNs: timeNs) }
        return current?.state
    }

    func end(id: UInt64, at point: CGPoint, timeNs: UInt64, cancelled: Bool = false) {
        guard var value = current, value.id == id else { return }
        value.point = point; current = value
        send(phase: cancelled ? "cancel" : "up", timeNs: timeNs)
    }

    func cancel(id: UInt64? = nil) {
        guard let value = current, id == nil || value.id == id else { return }
        current = nil; generation &+= 1
        value.update(nil)
        store?.semanticInput(input(value, phase: "cancel", timeNs: DispatchTime.now().uptimeNanoseconds))
    }

    func cancelPending() {
        guard let value = current, value.state.pending && !value.state.claimed else { return }
        cancel(id: value.id)
    }

    func state(for id: UInt64) -> EdgeReachState? {
        current.flatMap { $0.id == id ? $0.state : nil }
    }

    func doublePress(device: ReorderDevice, contact: EdgeReachContact, at point: CGPoint) {
        guard let store, !store.snapshot["layout"]["viewport"].isNull else { return }
        store.semanticInput(JSON(["type": "edge_double_press", "kind": device.rawValue,
            "contact": contact.rawValue, "position": [point.x, point.y],
            "viewport": store.snapshot["layout"]["viewport"].raw]))
    }

    private func send(phase: String, timeNs: UInt64) {
        guard var value = current else { return }
        value.sequence &+= 1; current = value
        let sequence = value.sequence, generation = value.generation
        store?.semanticInput(input(value, phase: phase, timeNs: timeNs)) { [weak self] reply in
            self?.apply(reply, phase: phase, generation: generation, sequence: sequence)
        }
    }

    private func input(_ value: Contact, phase: String, timeNs: UInt64) -> JSON {
        JSON(["type": "edge_contact", "id": value.id, "phase": phase, "kind": value.device.rawValue,
            "contact": value.contact.rawValue, "position": [value.point.x, value.point.y],
            "viewport": value.viewport, "time_ns": timeNs])
    }

    private func apply(_ reply: JSON, phase: String, generation: UInt64, sequence: UInt64) {
        guard var value = current, value.generation == generation, sequence > value.applied else { return }
        let wasClaimed = value.state.claimed
        value.applied = sequence
        let cursor = reply["edge_cursor"].isNull ? nil : reply["edge_cursor"].string
        let claimed = wasClaimed || cursor != nil
        var deferred = value.state.selectOnRelease
        if phase == "down" {
            deferred = reply["edge_select_on_release"].bool
            if !deferred && cursor == nil { value.select?(); value.select = nil }
        }
        if claimed { value.select = nil }
        let pending = reply["edge_pending"].bool
        value.state = EdgeReachState(pending: pending,
            hold: reply["edge_hold"].bool || (pending && deferred), claimed: claimed,
            selectOnRelease: deferred, cursor: cursor ?? value.state.cursor)
        current = value
        if let cursor, !wasClaimed {
            value.claim(cursor)
            guard current?.generation == value.generation else { return }
        }
        if phase == "up" || phase == "cancel" {
            finish(value, select: phase == "up" && !reply["handled"].bool)
        } else if !pending && cursor == nil && !claimed {
            finish(value, select: false)
        } else {
            value.update(value.state)
        }
    }

    private func finish(_ value: Contact, select: Bool) {
        guard current?.generation == value.generation else { return }
        current = nil
        if select { value.select?() }
        value.update(nil)
    }
}

/// The host supplies native timing, device identity and contact delivery; each
/// editor projection supplies measured sources and shared transaction callbacks.
@MainActor protocol NativeReorderModel: AnyObject {
    var contact: ReorderContact { get }
    var viewport: CGRect { get set }
    var enabled: Bool { get }
    var bubbleInput: BubbleInteraction? { get }
    var layerTouchModel: (any NativeLayerTouchModel)? { get }
    var edgeReach: EdgeReachCoordinator? { get }
    func edgeReachSource(at point: CGPoint, device: ReorderDevice) -> EdgeReachSource?
    func source(at point: CGPoint) -> ReorderTarget?
    func acceptsContext(at point: CGPoint) -> Bool
    func context(at point: CGPoint)
    func recognizeHold()
    func cancel()
    func cancelReorder()
    func nativeInputDetached()
    var swiping: Bool { get }
    func beginSwipe(at point: CGPoint) -> Bool
    func moveSwipe(to point: CGPoint)
    func finishSwipe(cancelled: Bool)
    var edgeScroll: ReorderEdgeScroll? { get }
}

@MainActor protocol NativeLayerTouchModel: NativeReorderModel {
    func touchRow(at point: CGPoint) -> UInt64?
    func layerTouch(_ id: UInt64, phase: String, point: CGPoint, row: UInt64, timestamp: TimeInterval, scale: CGFloat)
}

struct ReorderEdgeScroll {
    var inside: CGFloat = 20
    var outside: CGFloat = 12
    var speed: CGFloat = 240
    func delta(at point: CGPoint, in viewport: CGRect, elapsed: TimeInterval) -> CGFloat {
        guard point.x >= viewport.minX, point.x <= viewport.maxX else { return 0 }
        let step = speed * CGFloat(min(0.05, max(0, elapsed)))
        if point.y < viewport.minY + inside && point.y >= viewport.minY - outside { return -step }
        if point.y > viewport.maxY - inside && point.y <= viewport.maxY + outside { return step }
        return 0
    }
}

extension NativeReorderModel {
    var bubbleInput: BubbleInteraction? { nil }
    var layerTouchModel: (any NativeLayerTouchModel)? { nil }
    var edgeReach: EdgeReachCoordinator? { nil }
    func edgeReachSource(at point: CGPoint, device: ReorderDevice) -> EdgeReachSource? { nil }
    func cancelReorder() { cancel() }
    var swiping: Bool { false }
    func beginSwipe(at point: CGPoint) -> Bool { false }
    func moveSwipe(to point: CGPoint) {}
    func finishSwipe(cancelled: Bool) {}
    var edgeScroll: ReorderEdgeScroll? { nil }
    func recognizeHold() { if !swiping { contact.recognizeHold() } }
    func nativeInputDetached() {
        // Native recognizers are already detached. SwiftUI can still own its
        // graph exclusively; defer publication and preserve a remounted contact.
        let generation = contact.generation
        let bubbleGeneration = bubbleInput?.generation
        DispatchQueue.main.async { [self] in
            if contact.generation == generation && bubbleInput?.generation == bubbleGeneration { cancel() }
        }
    }
}
