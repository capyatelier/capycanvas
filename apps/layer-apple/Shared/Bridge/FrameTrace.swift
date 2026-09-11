import Foundation
import QuartzCore
import Darwin

/// Opt-in local observations. Fixed-size records and a hard cap bound memory;
/// no document names, pixels, coordinates or hardware/account IDs are collected.
struct FrameTraceEvent {
    enum Kind: UInt8 { case tick, frame, input, drawable, presented, memory, display, gpu, gpuStatus, state, activity, workload }
    let kind: Kind
    var a: UInt64 = 0, b: UInt64 = 0, c: UInt64 = 0, d: UInt64 = 0, e: UInt64 = 0
    var f: UInt64 = 0, g: UInt64 = 0, h: UInt64 = 0, i: UInt64 = 0, j: UInt64 = 0
    var columns: [UInt64] { [a, b, c, d, e, f, g, h, i, j] }
}

final class FrameTrace: @unchecked Sendable {
    static func now() -> UInt64 { UInt64(CACurrentMediaTime() * 1_000_000_000) }
    static func timestamp(_ value: Double) -> UInt64 {
        value.isFinite && value > 0 && value < Double(UInt64.max) ? UInt64(value) : 0
    }
    let started: UInt64
    let duration: TimeInterval
    let capacity: Int
    private let lock = NSLock()
    private var events: [FrameTraceEvent] = []
    private var dropped: UInt64 = 0
    private var frozen = false
    private var finishing = false
    private var timer: DispatchSourceTimer?
    private let output: URL?
    private let platform: UInt32
    private let workload: [String: Any]?

    init(duration: TimeInterval, capacity: Int, platform: UInt32, output: URL? = nil, started: UInt64 = FrameTrace.now(), workload: [String: Any]? = nil) {
        self.duration = duration; self.capacity = max(1, capacity)
        self.platform = platform; self.output = output; self.started = started; self.workload = workload
        events.reserveCapacity(self.capacity)
    }
    static func configured(platform: UInt32, defaultDuration: TimeInterval? = nil, workload: [String: Any]? = nil) -> FrameTrace? {
        let environment = ProcessInfo.processInfo.environment
        guard let text = environment["CAPY_TRACE_SECONDS"] ?? defaultDuration.map(String.init(describing:)), let seconds = Double(text),
            seconds.isFinite, seconds > 0, seconds <= 3600 else { return nil }
        let directory = environment["CAPY_TRACE_DIRECTORY"].map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("Performance", isDirectory: true)
        let trace = FrameTrace(duration: seconds, capacity: min(1_000_000, max(2048, Int(ceil(seconds * 1200)))),
            platform: platform, output: directory.appendingPathComponent("frames-\(UUID().uuidString).jsonl"), workload: workload)
        trace.startTimer()
        return trace
    }
    var isRecording: Bool {
        let elapsed = Self.now() &- started
        lock.lock(); defer { lock.unlock() }
        return !frozen && !finishing && Double(elapsed) < duration * 1_000_000_000
    }
    var acceptsCompletions: Bool {
        lock.lock(); defer { lock.unlock() }
        return !frozen && Double(Self.now() &- started) < (duration + 2) * 1_000_000_000
    }
    /// Completion callbacks for an admitted frame may arrive after the capture
    /// interval. Keep them during the grace period, then count missing evidence
    /// in the analyzer instead of treating it as a zero-duration presentation.
    func record(_ event: FrameTraceEvent) {
        lock.lock(); defer { lock.unlock() }
        guard !frozen else { return }
        if events.count < capacity { events.append(event) } else { dropped &+= 1 }
    }
    func freeze() -> (events: [FrameTraceEvent], dropped: UInt64) {
        lock.lock(); defer { lock.unlock() }
        frozen = true
        let result = events; events = []
        return (result, dropped)
    }
    func finish() {
        lock.lock()
        guard !finishing && !frozen else { lock.unlock(); return }
        finishing = true
        lock.unlock()
        timer?.cancel(); timer = nil
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 2) { [self] in export() }
    }
    private func startTimer() {
        let timer = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
        self.timer = timer
        timer.schedule(deadline: .now(), repeating: 1)
        timer.setEventHandler { [weak self] in
            guard let self else { return }
            self.sampleMemory()
            if !self.isRecording {
                self.finish()
            }
        }
        timer.resume()
    }
    private func sampleMemory() {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let status = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        record(FrameTraceEvent(kind: .memory, a: Self.now(), b: status == KERN_SUCCESS ? info.phys_footprint : 0,
            c: status == KERN_SUCCESS ? info.resident_size : 0,
            d: UInt64(ProcessInfo.processInfo.thermalState.rawValue), e: UInt64(UInt32(bitPattern: status))))
    }
    private func export() {
        let snapshot = freeze()
        guard let output else { return }
        do {
            try FileManager.default.createDirectory(at: output.deletingLastPathComponent(), withIntermediateDirectories: true)
            #if DEBUG
            let configuration = "debug"
            #else
            let configuration = "release"
            #endif
            let header: [String: Any] = ["schema": 1, "clock": "CACurrentMediaTime nanoseconds", "platform": platform,
                "configuration": configuration, "duration_seconds": duration, "started_ns": started,
                "capacity": capacity, "dropped_records": snapshot.dropped, "record_stride_bytes": MemoryLayout<FrameTraceEvent>.stride,
                "input_source": workload == nil ? "platform" : "synthetic", "workload": workload as Any? ?? NSNull(),
                "input_association": "received by owner before frame; pixel inclusion is not established"]
            // Freeze once, then stream JSONL outside both UI and render queues.
            // There is no full-trace JSON allocation or copy on the hot path.
            let temporary = output.appendingPathExtension("partial")
            guard FileManager.default.createFile(atPath: temporary.path, contents: nil) else { throw CocoaError(.fileWriteUnknown) }
            let file = try FileHandle(forWritingTo: temporary)
            defer { try? file.close() }
            var bytes = try JSONSerialization.data(withJSONObject: header, options: [.sortedKeys]); bytes.append(10)
            for event in snapshot.events {
                bytes.append(contentsOf: "[\(event.kind.rawValue),\(event.columns.map(String.init).joined(separator: ","))]\n".utf8)
                if bytes.count >= 65536 { try file.write(contentsOf: bytes); bytes.removeAll(keepingCapacity: true) }
            }
            if !bytes.isEmpty { try file.write(contentsOf: bytes) }
            try file.synchronize()
            try FileManager.default.moveItem(at: temporary, to: output)
        } catch {
            // Recording errors do not interrupt the drawing session.
            NSLog("Capy performance trace export failed: %@", error.localizedDescription)
        }
    }
}
