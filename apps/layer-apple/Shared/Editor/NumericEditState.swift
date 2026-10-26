import Foundation

struct NumericAdmissions {
    enum Change {
        case mounted(UUID, (Bool) -> Bool)
        case updated(UUID, (Bool) -> Bool)
        case unmounted(UUID)
    }
    private var entries: [String: (owner: UUID, admit: (Bool) -> Bool)] = [:]

    mutating func receive(_ key: String, _ change: Change) {
        switch change {
        case let .mounted(owner, admit): entries[key] = (owner, admit)
        case let .updated(owner, admit):
            if entries[key]?.owner == owner { entries[key] = (owner, admit) }
        case let .unmounted(owner):
            if entries[key]?.owner == owner { entries.removeValue(forKey: key) }
        }
    }
    func finish(_ discard: Bool = false, only key: String? = nil) -> Bool {
        entries.filter { key == nil || $0.key == key }.values.map { $0.admit(discard) }.allSatisfy { $0 }
    }
}

/// Presentation state only. Rust owns expressions, units, stepping and ranges.
/// Keep optimistic edits ahead of queued snapshot echoes without losing drafts.
struct NumericEditState {
    private(set) var value: Double = 0
    private var confirmed: Double = 0
    private var sequence: UInt64 = 0
    private var acknowledged: UInt64 = 0
    private var pending: [UInt64: Double] = [:]
    var text = ""
    var dirty = false
    var error: String?

    mutating func receive(_ value: Double) {
        confirmed = value
        if pending.isEmpty { self.value = value }
    }
    mutating func submit(_ value: Double) -> UInt64 {
        sequence &+= 1
        // The shared model stores f32. Retain the exact transported value.
        let value = Double(Float(value))
        pending[sequence] = value
        self.value = value
        error = nil
        return sequence
    }
    mutating func complete(_ token: UInt64, error: String?) {
        guard let accepted = pending.removeValue(forKey: token), token > acknowledged else { return }
        acknowledged = token
        if error == nil { confirmed = accepted }
        guard token == sequence else { return }
        self.error = error
        value = error == nil ? accepted : confirmed
    }
}
