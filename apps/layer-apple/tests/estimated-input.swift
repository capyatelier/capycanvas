import Foundation

@main struct EstimatedInputChecks {
    static func main() {
        var ledger = EstimatedInput(capacity: 2)
        let key = EstimatedInput.Key(index: 4, timestamp: 1)
        let original: [Double] = [20, 40, 0.2, 0.1, 0.2, 0.3, 0, 1_000_000_000, 1]
        let capture = ledger.capture(key: key, contact: 8, revision: 10, scale: 2, record: original, expected: 1 | 16)
        assert(capture.metadata[0] != 0 && capture.metadata[1] == 1 && capture.released == nil)
        let repeated = ledger.capture(key: key, contact: 8, revision: 10, scale: 2, record: original, expected: 1 | 16)
        assert(repeated.metadata == capture.metadata && ledger.pending.count == 1)
        var updated = original
        updated[0] = 900; updated[2] = 0.8; updated[5] = 1.7; updated[7] = 90; updated[8] = 3
        let partial = ledger.correct(key: key, record: updated, expected: 16)!
        assert(partial.record[0] == 20 && partial.record[2] == 0.8 && partial.record[5] == 1.7)
        assert(partial.record[7] == original[7] && partial.record[8] == 1)
        assert(partial.contact == 8 && partial.revision == 10 && partial.scale == 2)
        updated[2] = 0; updated[5] = 2.1
        let final = ledger.correct(key: key, record: updated, expected: 0)!
        assert(final.record[2] == 0.8 && final.record[5] == 2.1 && final.metadata[1] == 0)
        assert(ledger.pending.isEmpty && ledger.correct(key: key, record: updated, expected: 0) == nil)
        for timestamp in 2...4 {
            let added = ledger.capture(key: .init(index: 4, timestamp: Double(timestamp)), contact: UInt64(timestamp),
                revision: 10, scale: 2, record: original, expected: 1)
            if timestamp == 4 { assert(added.released?.contact == 2 && added.released?.metadata[1] == 0) }
        }
        assert(ledger.pending.count == 2 && ledger.expired == 1)
        assert(ledger.correct(key: key, record: updated, expected: 0) == nil, "Reused source IDs cannot redirect old callbacks")
        ledger.cancel(contact: 3)
        let remaining = ledger.finish()
        assert(remaining.count == 1 && remaining[0].contact == 4 && remaining[0].metadata[1] == 0)
        assert(ledger.pending.isEmpty)
        print("Estimated-input checks passed: partial/final properties, captured coordinates, token reuse, bounded retention and teardown")
    }
}
