// Host-independent lifecycle/race checks for the real Metal presentation gate.
import Foundation

private final class CallbackCount: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    func increment() { lock.lock(); count += 1; lock.unlock() }
    var value: Int { lock.lock(); defer { lock.unlock() }; return count }
}

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
        checkNotification()
        print("Presentation gate checks passed: capacity, completion, cancellation, replacement and concurrent duplicate callbacks")
    }
    static func checkNotification() {
        let gate = FramePresentationGate(), count = CallbackCount()
        gate.reset(capacity: 1)
        let first = gate.acquired()
        gate.whenAvailable { assert(gate.hasCapacity); count.increment() }
        assert(count.value == 0)
        DispatchQueue.concurrentPerform(iterations: 64) { _ in gate.retired(first) }
        assert(count.value == 1, "Retirement notifies exactly once, outside the lock")
        gate.whenAvailable { count.increment() }
        assert(count.value == 2, "Retirement before registration must not lose a wake")
        let second = gate.acquired()
        gate.whenAvailable { count.increment() }
        gate.whenAvailable(nil)
        gate.retired(second)
        assert(count.value == 2, "Explicit cancellation retires the waiter")
        let third = gate.acquired()
        gate.whenAvailable { count.increment() }
        gate.reset()
        let fourth = gate.acquired()
        gate.whenAvailable { count.increment() }
        gate.retired(third)
        assert(count.value == 2, "Surface reset discards the old waiter and ticket")
        gate.retired(fourth)
        assert(count.value == 3)
        print("Capacity notification checks passed: registration race, duplicate callbacks, cancellation and reset")
    }
}
