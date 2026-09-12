// Host-independent lifecycle/race checks for the real Metal presentation gate.
import Foundation

@main struct PresentationGateChecks {
    static func main() {
        let gate = FramePresentationGate()
        let first = (0..<3).map { _ in gate.acquired() }
        assert(!gate.hasCapacity, "Submission alone cannot recycle all drawables")
        gate.retired(first[0]); assert(gate.hasCapacity)
        let replacement = gate.acquired(); assert(!gate.hasCapacity)
        gate.retired(first[0]); assert(!gate.hasCapacity, "Duplicate callbacks must not release a replacement")
        gate.reset(capacity: 2)
        let next = (0..<2).map { _ in gate.acquired() }
        for old in first + [replacement] { gate.retired(old) }
        assert(!gate.hasCapacity, "Old-surface callbacks must not alter new-surface capacity")
        gate.retired(next[0]); assert(gate.hasCapacity, "Cancelling an unsubmitted frame must release its ticket")
        let final = gate.acquired(); assert(!gate.hasCapacity)
        DispatchQueue.concurrentPerform(iterations: 64) { index in
            gate.retired(index.isMultiple(of: 2) ? next[1] : final)
        }
        assert(gate.hasCapacity)
        _ = gate.acquired(); assert(gate.hasCapacity)
        _ = gate.acquired(); assert(!gate.hasCapacity)
        gate.reset()
        _ = gate.acquired(); assert(gate.hasCapacity)
        _ = gate.acquired(); assert(!gate.hasCapacity, "Resuming preserves the configured drawable limit")
        print("Presentation gate checks passed: capacity, completion, cancellation, replacement and concurrent duplicate callbacks")
    }
}
