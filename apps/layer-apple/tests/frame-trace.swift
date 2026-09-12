// Concurrent completion callbacks must stay bounded and survive the grace period.
import Foundation
import Dispatch

@main struct TraceChecks {
    static func main() throws {
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
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("capy-trace-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let output = directory.appendingPathComponent("frames.jsonl")
        let exported = FrameTrace(duration: 30, capacity: 2, platform: 1, output: output)
        exported.record(FrameTraceEvent(kind: .frame, a: 42))
        exported.finish()
        let deadline = Date().addingTimeInterval(5)
        while !FileManager.default.fileExists(atPath: output.path) && Date() < deadline {
            Thread.sleep(forTimeInterval: 0.01)
        }
        let lines = try String(contentsOf: output, encoding: .utf8).split(separator: "\n")
        let header = try JSONSerialization.jsonObject(with: Data(lines[0].utf8)) as! [String: Any]
        assert(header["process_identifier"] as? Int32 == ProcessInfo.processInfo.processIdentifier)
        assert(header["clock"] as? String == "CACurrentMediaTime nanoseconds")
        assert(lines.count == 2, "Atomic export must pair the process header with its frame records")
        print("Frame trace checks passed: concurrent capacity, freeze, late presentation and process-tagged export")
    }
}
