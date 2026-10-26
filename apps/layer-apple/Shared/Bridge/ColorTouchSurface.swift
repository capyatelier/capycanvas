import Foundation

@MainActor final class ColorTouchSurface {
    private static var nextSurface: UInt64 = 0
    let surface: UInt64
    var send: ([String: Any]) -> Void = { _ in }
    private var contacts: [ObjectIdentifier: UInt64] = [:]
    private var nextID: UInt64 = 0
    private var retirements: [ObjectIdentifier: () -> Void] = [:]
    private(set) var claimed = false
    init() { Self.nextSurface &+= 1; surface = Self.nextSurface }
    func onClaim(_ owner: AnyObject, retire: @escaping () -> Void) { retirements[ObjectIdentifier(owner)] = retire }
    func remove(_ owner: AnyObject) { retirements.removeValue(forKey: ObjectIdentifier(owner)) }
    func tracks(_ key: ObjectIdentifier) -> Bool { contacts[key] != nil }
    func contact(_ key: ObjectIdentifier, phase: String, position: CGPoint, timestamp: TimeInterval) {
        if phase == "down" { nextID &+= 1; contacts[key] = nextID }
        guard let id = contacts[key] else { return }
        if phase == "up" || phase == "cancel" { contacts.removeValue(forKey: key) }
        send(Self.input(surface: surface, id: id, phase: phase, position: position, timestamp: timestamp))
        if phase == "down" && contacts.count >= 2 && !claimed {
            claimed = true
            for retire in retirements.values { retire() }
        }
        if contacts.isEmpty { claimed = false }
    }
    nonisolated static func input(surface: UInt64, id: UInt64, phase: String, position: CGPoint, timestamp: TimeInterval) -> [String: Any] {
        ["type": "color_touch", "surface": surface, "id": id, "phase": phase,
         "position": [Double(position.x), Double(position.y)], "time_ns": UInt64(max(0, timestamp) * 1_000_000_000)]
    }
}
