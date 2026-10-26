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
        let exported = FrameTrace(duration: 30, capacity: 4, platform: 1, output: output)
        exported.record(FrameTraceEvent(kind: .consumedPaint, a: 42, b: 21, c: 4_294_967_321,
            d: 4_294_967_322, e: 2))
        exported.record(FrameTraceEvent(kind: .consumedPaint, a: 43, b: 22, c: 4_294_967_323, d: 0, e: 2))
        exported.record(FrameTraceEvent(kind: .consumedPaint, a: 44, b: 23, c: 4_294_967_324, d: 0, e: 1))
        exported.record(FrameTraceEvent(kind: .consumedPaint, a: 45, b: 24, c: 4_294_967_325))
        exported.finish()
        let deadline = Date().addingTimeInterval(5)
        while !FileManager.default.fileExists(atPath: output.path) && Date() < deadline {
            Thread.sleep(forTimeInterval: 0.01)
        }
        let lines = try String(contentsOf: output, encoding: .utf8).split(separator: "\n")
        let header = try JSONSerialization.jsonObject(with: Data(lines[0].utf8)) as! [String: Any]
        assert(header["process_identifier"] as? Int32 == ProcessInfo.processInfo.processIdentifier)
        assert(header["clock"] as? String == "CACurrentMediaTime nanoseconds")
        assert(header["raster_update_counter"] as? Bool == true)
        assert(header["encoded_camera_revision"] as? Bool == true,
            "Trace headers must advertise encoded-camera revision and result columns")
        assert(lines.count == 5, "Atomic export must pair the process header with its frame records")
        let event = try JSONSerialization.jsonObject(with: Data(lines[1].utf8)) as! [UInt64]
        assert(Array(event.prefix(3)) == [15, 42, 21], "Consumed-paint records must preserve their wire kind and timestamps")
        assert(event[3] == 4_294_967_321, "Consumed-paint records must preserve the submitted canvas-update counter")
        assert(event == [15, 42, 21, 4_294_967_321, 4_294_967_322, 2, 0, 0, 0, 0, 0],
            "Consumed-paint records must preserve the full encoded-camera revision and successful result")
        let cameraEvents = try lines.dropFirst(2).map {
            try JSONSerialization.jsonObject(with: Data($0.utf8)) as! [UInt64]
        }
        assert(cameraEvents == [
            [15, 43, 22, 4_294_967_323, 0, 2, 0, 0, 0, 0, 0],
            [15, 44, 23, 4_294_967_324, 0, 1, 0, 0, 0, 0, 0],
            [15, 45, 24, 4_294_967_325, 0, 0, 0, 0, 0, 0, 0]
        ], "A valid zero camera revision must retain its frame outcome and remain distinct from unknown metadata")
        print("Frame trace checks passed: concurrent capacity, freeze, late presentation, process-tagged export and encoded-camera columns")
    }
}
