import QuartzCore
import Metal

/// wgpu acquires drawables through this same CAMetalLayer. Its real presentation
/// callback is independent of Rust submission and display-link completion.
final class ObservedMetalLayer: CAMetalLayer {
    // Read/written only by the serial render owner, including nextDrawable.
    var observation: (trace: FrameTrace, frame: UInt64)?
    override func nextDrawable() -> (any CAMetalDrawable)? {
        #if targetEnvironment(simulator)
        // The simulator SDK has no drawable ID or presentation callback.
        // Never fabricate physical presentation evidence for simulator tests.
        return super.nextDrawable()
        #else
        guard let observation else { return super.nextDrawable() }
        let start = FrameTrace.now()
        let drawable = super.nextDrawable()
        observation.trace.record(FrameTraceEvent(kind: .drawable, a: observation.frame,
            b: start, c: FrameTrace.now(), d: drawable.map { UInt64($0.drawableID) } ?? 0, e: drawable == nil ? 0 : 1))
        drawable?.addPresentedHandler { [trace = observation.trace, frame = observation.frame] drawable in
            trace.record(FrameTraceEvent(kind: .presented, a: frame,
                b: FrameTrace.timestamp(drawable.presentedTime * 1_000_000_000),
                c: FrameTrace.now(), d: UInt64(drawable.drawableID)))
        }
        return drawable
        #endif
    }
}
