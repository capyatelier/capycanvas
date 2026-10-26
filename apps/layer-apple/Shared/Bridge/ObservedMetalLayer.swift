import QuartzCore
import Metal
import Foundation

final class CanvasFrameRateObserver: @unchecked Sendable {
    private let handle: OpaquePointer
    init?(_ app: UInt64) {
        guard let handle = capy_apple_frame_rate_observer(app) else { return nil }
        self.handle = handle
    }
    deinit { capy_apple_frame_rate_observer_release(handle) }
    func presented(at time: CFTimeInterval) {
        let presented = FrameTrace.timestamp(time * 1_000_000_000)
        let observed = FrameTrace.now()
        guard presented > 0, observed >= presented else { return }
        capy_apple_frame_rate_presented(handle, presented, observed)
    }
}

/// wgpu acquires drawables through this same CAMetalLayer. Its real presentation
/// callback is diagnostic only: missing notifications must not stop rendering.
/// CAMetalLayer owns drawable availability; the serial owner bounds submission.
final class ObservedMetalLayer: CAMetalLayer {
    // Read/written only by the serial render owner, including nextDrawable.
    var observation: (trace: FrameTrace, frame: UInt64)?
    var frameRate: CanvasFrameRateObserver?
    override func nextDrawable() -> (any CAMetalDrawable)? {
        #if targetEnvironment(simulator)
        // The simulator SDK has no drawable ID or presentation callback.
        // Never fabricate physical presentation evidence for simulator tests.
        return super.nextDrawable()
        #else
        let observation = self.observation
        let frameRate = self.frameRate
        let start = observation == nil ? 0 : FrameTrace.now()
        let drawable = super.nextDrawable()
        if let observation {
            observation.trace.record(FrameTraceEvent(kind: .drawable, a: observation.frame,
                b: start, c: FrameTrace.now(), d: drawable.map { UInt64($0.drawableID) } ?? 0, e: drawable == nil ? 0 : 1))
        }
        drawable?.addPresentedHandler { drawable in
            let received = observation == nil ? 0 : FrameTrace.now()
            frameRate?.presented(at: drawable.presentedTime)
            if let observation {
                observation.trace.record(FrameTraceEvent(kind: .presented, a: observation.frame,
                    b: FrameTrace.timestamp(drawable.presentedTime * 1_000_000_000),
                    c: received, d: UInt64(drawable.drawableID)))
            }
        }
        return drawable
        #endif
    }
}
