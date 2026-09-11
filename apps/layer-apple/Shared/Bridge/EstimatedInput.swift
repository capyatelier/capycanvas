import Foundation

/// Numeric observations only. No platform touch/event object survives its
/// callback. The opaque source index is combined with the original timestamp
/// so driver reuse cannot redirect an old update to a newer contact.
struct EstimatedInput {
    struct Key: Hashable { let index: UInt64; let timestamp: TimeInterval }
    struct Sample {
        let contact: UInt64
        let token: UInt64
        let revision: UInt64
        let scale: CGFloat
        var record: [Double]
        var expected: UInt64
        var metadata: [UInt64] { [token, expected == 0 ? 0 : 1] }
    }
    private(set) var pending: [Key: Sample] = [:]
    private var nextToken: UInt64 = 0
    private(set) var expired: UInt64 = 0
    let capacity: Int
    init(capacity: Int = 4096) { self.capacity = max(1, capacity) }

    mutating func capture(key: Key?, contact: UInt64, revision: UInt64, scale: CGFloat,
                          record: [Double], expected: UInt64) -> (metadata: [UInt64], released: Sample?) {
        guard let key else { return ([0, 0], nil) }
        if pending[key]?.contact == contact, let update = correct(key: key, record: record, expected: expected) {
            return (update.expected == 0 ? [0, 0] : update.metadata, update)
        }
        guard expected != 0 else { return ([0, 0], nil) }
        var released = pending.removeValue(forKey: key)
        if pending.count >= capacity, let oldest = pending.min(by: { $0.value.token < $1.value.token }) {
            released = pending.removeValue(forKey: oldest.key)
            expired &+= 1
        }
        // Finalize the last known estimate when bounded retention is exhausted.
        // This frees its Rust token; a very late callback cannot alias new ink.
        released?.expected = 0
        nextToken &+= 1
        precondition(nextToken != 0, "Input token space exhausted")
        let sample = Sample(contact: contact, token: nextToken, revision: revision,
                            scale: scale, record: record, expected: expected)
        pending[key] = sample
        return (sample.metadata, released)
    }

    mutating func correct(key: Key, record: [Double], expected: UInt64) -> Sample? {
        guard var sample = pending[key] else { return nil }
        // UITouch.Properties: force=1, azimuth=2, altitude=4, location=8, roll=16.
        // Only previously estimated fields may change; contact phase and the
        // observation timestamp belong to the original delivery.
        for (mask, fields): (UInt64, [Int]) in [(1, [2]), (6, [3, 4]), (8, [0, 1]), (16, [5])] {
            if sample.expected & mask != 0 { for index in fields { sample.record[index] = record[index] } }
        }
        sample.expected &= expected
        pending[key] = sample.expected == 0 ? nil : sample
        return sample
    }
    mutating func cancel(contact: UInt64) { pending = pending.filter { $0.value.contact != contact } }
    mutating func finish() -> [Sample] {
        let result = pending.values.map { value in var value = value; value.expected = 0; return value }
        pending.removeAll(keepingCapacity: true)
        return result
    }
}
