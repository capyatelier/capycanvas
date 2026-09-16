import QuartzCore
import Metal
import Foundation

/// wgpu acquires drawables through this same CAMetalLayer. Its real presentation
/// callback is diagnostic only: missing notifications must not stop rendering.
/// CAMetalLayer owns drawable availability; the serial owner bounds submission.
final class ObservedMetalLayer: CAMetalLayer {
    // Read/written only by the serial render owner, including nextDrawable.
    var observation: (trace: FrameTrace, frame: UInt64)?
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
        if let observation {
            drawable?.addPresentedHandler { drawable in
                observation.trace.record(FrameTraceEvent(kind: .presented, a: observation.frame,
                    b: FrameTrace.timestamp(drawable.presentedTime * 1_000_000_000),
                    c: FrameTrace.now(), d: UInt64(drawable.drawableID)))
            }
        }
        return drawable
        #endif
    }
}
