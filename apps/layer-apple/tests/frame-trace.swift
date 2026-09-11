// Concurrent completion callbacks must stay bounded and survive the grace period.
import Foundation
import Dispatch

@main struct TraceChecks {
    static func main() {
        let trace = FrameTrace(duration: 30, capacity: 300, platform: 1)
        DispatchQueue.concurrentPerform(iterations: 1000) { index in
            trace.record(FrameTraceEvent(kind: .presented, a: UInt64(index)))
        }
        let result = trace.freeze()
        assert(result.events.count == 300 && result.dropped == 700)
        assert(Set(result.events.map(\.a)).count == 300, "Concurrent writers must not corrupt records")
        trace.record(FrameTraceEvent(kind: .tick))
        assert(trace.freeze().events.isEmpty && !trace.isRecording && !trace.acceptsCompletions)

        let expired = FrameTrace(duration: 0, capacity: 2, platform: 0)
        assert(!expired.isRecording && expired.acceptsCompletions)
        expired.record(FrameTraceEvent(kind: .presented, a: 42))
        assert(expired.freeze().events.first?.a == 42, "An admitted frame can present after recording ends")
        assert(FrameTrace.timestamp(.nan) == 0 && FrameTrace.timestamp(.infinity) == 0)
        assert(FrameTrace.timestamp(-1) == 0 && FrameTrace.timestamp(Double(UInt64.max)) == 0)
        print("Frame trace checks passed: concurrent capacity, freeze and late presentation")
    }
}
