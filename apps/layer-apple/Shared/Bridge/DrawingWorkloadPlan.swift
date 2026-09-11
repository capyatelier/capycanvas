import Foundation

/// Fixed, versioned synthetic input. Delivery is driven by wall time, independently
/// of frame admission, so slow rendering cannot silently reduce the input load.
struct DrawingWorkloadPlan {
    static let samplesPerSecond = 240
    static let warmupSeconds = 10.0
    static let strokeSamples = 360
    static let cycleSamples = 384 // 1.5 s drawing, then 0.1 s with the pen lifted.
    let name: String
    let identifier: UInt64
    let extent: UInt32
    let paintLayers: Int
    let brush: UInt32
    let diameter: Double
    let predicts: Bool
    let seconds: Double

    init(name: String, seconds: Double) throws {
        guard seconds.isFinite, seconds >= 1, seconds <= 1800 else {
            throw PlanError(errorDescription: "CAPY_WORKLOAD_SECONDS must be between 1 and 1800")
        }
        self.name = name; self.seconds = seconds
        switch name {
        case "ink": identifier = 1; extent = 2048; paintLayers = 1; brush = 1; diameter = 24; predicts = false
        case "ink-predicted": identifier = 2; extent = 2048; paintLayers = 1; brush = 1; diameter = 24; predicts = true
        case "wet-watercolor": identifier = 3; extent = 2048; paintLayers = 1; brush = 21; diameter = 320; predicts = true
        case "layered-4k": identifier = 4; extent = 4096; paintLayers = 8; brush = 1; diameter = 24; predicts = true
        case "wet-watercolor-4k": identifier = 5; extent = 4096; paintLayers = 8; brush = 21; diameter = 320; predicts = true
        default: throw PlanError(errorDescription: "Unknown CAPY_WORKLOAD profile: \(name)")
        }
    }
    static func configured() throws -> DrawingWorkloadPlan? {
        let environment = ProcessInfo.processInfo.environment
        guard let name = environment["CAPY_WORKLOAD"] else { return nil }
        guard let seconds = Double(environment["CAPY_WORKLOAD_SECONDS"] ?? "600") else {
            throw PlanError(errorDescription: "CAPY_WORKLOAD_SECONDS must be a number")
        }
        return try DrawingWorkloadPlan(name: name, seconds: seconds)
    }
    private struct PlanError: LocalizedError { let errorDescription: String? }
    var metadata: [String: Any] {
        ["version": 1, "name": name, "document_pixels": [extent, extent], "paint_layers": paintLayers,
         "brush_id": brush, "diameter": diameter, "prediction": predicts, "sample_hz": Self.samplesPerSecond,
         "warmup_seconds": Self.warmupSeconds, "measurement_seconds": seconds, "input_source": "synthetic"]
    }
    struct Sample {
        let contact: UInt64
        let x: Double, y: Double, pressure: Double, phase: Double
    }
    func sample(_ index: Int) -> Sample? {
        let point = index % Self.cycleSamples
        guard point < Self.strokeSamples else { return nil }
        let stroke = index / Self.cycleSamples
        let t = Double(point) / Double(Self.strokeSamples - 1)
        let shift = Double(stroke % 13) * 0.37
        return Sample(contact: UInt64(stroke + 1),
            x: Double(extent) * (0.5 + 0.32 * sin(t * .pi * 2 + shift)),
            y: Double(extent) * (0.5 + 0.27 * sin(t * .pi * 3 + shift * 0.7)),
            pressure: 0.25 + 0.75 * sin(t * .pi),
            phase: point == 0 ? 1 : point == Self.strokeSamples - 1 ? 3 : 2)
    }
}
