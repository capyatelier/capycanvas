import QuartzCore
import Metal
import Foundation

/// Admission observes completed presentation, not CPU submission. This only
/// prevents acquiring when every drawable is known to be in flight; Core
/// Animation can still delay recycling a drawable after its callback.
final class FramePresentationGate: @unchecked Sendable {
    private let lock = NSLock()
    private var capacity = 3
    private var serial: UInt64 = 0
    private var pending = Set<UInt64>()
    var hasCapacity: Bool {
        lock.lock(); defer { lock.unlock() }
        return pending.count < capacity
    }
    func reset(capacity: Int? = nil) {
        lock.lock(); defer { lock.unlock() }
        if let capacity { self.capacity = max(1, capacity) }
        pending.removeAll(keepingCapacity: true)
    }
    func acquired() -> UInt64 {
        lock.lock(); defer { lock.unlock() }
        serial &+= 1; pending.insert(serial)
        return serial
    }
    func retired(_ ticket: UInt64) {
        lock.lock(); defer { lock.unlock() }
        // Old-surface, duplicate and cancelled-frame callbacks are harmless.
        pending.remove(ticket)
    }
}

/// wgpu acquires drawables through this same CAMetalLayer. Its real presentation
/// callback is independent of Rust submission and display-link completion.
final class ObservedMetalLayer: CAMetalLayer {
    // Read/written only by the serial render owner, including nextDrawable.
    var observation: (trace: FrameTrace, frame: UInt64)?
    var presentationGate: FramePresentationGate?
    private var acquired: (gate: FramePresentationGate, ticket: UInt64)?
    func finishFrame(submitted: Bool) {
        if !submitted, let acquired { acquired.gate.retired(acquired.ticket) }
        acquired = nil
    }
    override func nextDrawable() -> (any CAMetalDrawable)? {
        #if targetEnvironment(simulator)
        // The simulator SDK has no drawable ID or presentation callback.
        // Never fabricate physical presentation evidence for simulator tests.
        return super.nextDrawable()
        #else
        let observation = self.observation
        let start = observation == nil ? 0 : FrameTrace.now()
        let drawable = super.nextDrawable()
        if let observation {
            observation.trace.record(FrameTraceEvent(kind: .drawable, a: observation.frame,
                b: start, c: FrameTrace.now(), d: drawable.map { UInt64($0.drawableID) } ?? 0, e: drawable == nil ? 0 : 1))
        }
        if drawable != nil, let presentationGate {
            acquired = (presentationGate, presentationGate.acquired())
        }
        let acquired = self.acquired
        if observation != nil || acquired != nil {
            drawable?.addPresentedHandler { drawable in
                if let acquired { acquired.gate.retired(acquired.ticket) }
                if let observation {
                    observation.trace.record(FrameTraceEvent(kind: .presented, a: observation.frame,
                        b: FrameTrace.timestamp(drawable.presentedTime * 1_000_000_000),
                        c: FrameTrace.now(), d: UInt64(drawable.drawableID)))
                }
            }
        }
        return drawable
        #endif
    }
}
