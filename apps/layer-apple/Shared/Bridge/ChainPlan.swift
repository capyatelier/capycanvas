import Foundation

struct ChainPlan {
    struct RenderingTimeout: LocalizedError {
        var errorDescription: String? { "Rendering was still pending after 60 s" }
    }
    struct Refused: LocalizedError {
        let reason: String
        var errorDescription: String? { reason }
    }
    struct Timeout: LocalizedError {
        let reason: String
        var errorDescription: String? { reason }
    }
    @MainActor func run(perform: @MainActor (JSON) async -> String?, notRun: @MainActor (JSON, String) -> Void) async -> String? {
        var stopped: String?
        for step in steps {
            if let reason = stopped { notRun(step, "not run: \(reason)") }
            else { stopped = await perform(step) }
        }
        return stopped
    }

    static let penHz = 200.0
    static let touchHz = 240.0
    let steps: [JSON]
    let documents: JSON
    let motionFrames: Double
    let needs: [String: [String]]

    init(data: Data, only: Set<String>? = nil) throws {
        let value = try JSON.decode(String(decoding: data, as: UTF8.self))
        guard value["schema"].uint == 1, !value["steps"].array.isEmpty else {
            throw HostFailure(message: "The chain file needs schema 1 and at least one step")
        }
        motionFrames = value["motion_frames"].isNull ? 150 : value["motion_frames"].number
        documents = value["documents"]
        steps = value["steps"].array.filter { only?.contains($0["id"].string) ?? true }
        if let missing = only?.subtracting(steps.map { $0["id"].string }).sorted().first {
            throw HostFailure(message: "CAPY_WORKLOAD_STEPS names \(missing), which the chain does not contain")
        }
        var opened: String?, needs: [String: [String]] = [:]
        for step in value["steps"].array {
            let kind = step["do"].string
            if kind == "open" { opened = step["document"].string; continue }
            let named = step["tabs"].array.map(\.string) + (step["document"].isNull ? [] : [step["document"].string])
            needs[step["id"].string] = named + (["launch", "resume", "close"].contains(kind) ? [] : opened.map { [$0] } ?? [])
        }
        self.needs = needs
    }
    func seconds(_ step: JSON, refreshHz: Double) -> Double {
        if !step["duration_ms"].isNull { return step["duration_ms"].number / 1000 }
        return (step["frames"].isNull ? motionFrames : step["frames"].number) / refreshHz
    }
    static func brushSize(_ value: JSON, pixels: Double) -> Double {
        value.string == "guarantee" ? (pixels < 20e6 ? 1024 : pixels < 40e6 ? 1536 : 2048) : value.number
    }
    static func command(_ name: String) -> String {
        ["magic_wand": "auto_select", "transform": "scale_rotate"][name] ?? name
    }
    static func triangle(_ progress: Double) -> Double { 1 - abs(2 * progress - 1) }

    struct Point: Equatable {
        var x: Double, y: Double, pressure = 1.0
    }
    static func stroke(_ path: String, seconds t: Double, progress p: Double) -> Point {
        let pressure = 0.25 + 0.75 * sin(.pi * p)
        if path == "zigzag" {
            return Point(x: 0.5 + 0.3 * (1 - 4 * abs(t.truncatingRemainder(dividingBy: 1) - 0.5)), y: 0.5 + 0.2 * (2 * p - 1), pressure: pressure)
        }
        return Point(x: 0.5 + 0.3 * cos(2 * .pi * t), y: 0.5 + 0.2 * sin(2 * .pi * t), pressure: pressure)
    }
    static func drag(_ path: String, from start: Point, progress p: Double) -> Point {
        if path == "loop" { return Point(x: 0.5 + 0.3 * cos(2 * .pi * p), y: 0.5 + 0.2 * sin(2 * .pi * p)) }
        return Point(x: start.x + 0.15 * p, y: start.y - 0.075 * sin(.pi * p))
    }

    struct Pose: Equatable {
        var scale = 1.0, rotation = 0.0, pan = 0.0
    }
    static func gesture(_ kind: String, progress p: Double, scale: Double = 2.5) -> Pose {
        let amount = triangle(p)
        switch kind {
        case "pinch": return Pose(scale: 1 + (scale - 1) * amount)
        case "rotate": return Pose(rotation: .pi / 3 * amount)
        default: return Pose(pan: 0.3 * amount)
        }
    }
    static func fingers(_ pose: Pose, area: [Double]) -> [Point] {
        let x = area[0] + area[2] * (0.5 + pose.pan), y = area[1] + area[3] * 0.5
        let radius = 0.1 * min(area[2], area[3]) * pose.scale
        return [1.0, -1.0].map { Point(x: x + $0 * radius * cos(pose.rotation), y: y + $0 * radius * sin(pose.rotation)) }
    }
    static func floatAndReturn(from start: Point, center: Point, radius: Double, progress p: Double) -> Point {
        func line(_ a: Point, _ b: Point, _ q: Double) -> Point { Point(x: a.x + (b.x - a.x) * q, y: a.y + (b.y - a.y) * q) }
        if p < 1.0 / 3 { return line(start, center, 3 * p) }
        if p > 2.0 / 3 { return line(center, start, 3 * p - 2) }
        let angle = 2 * .pi * (3 * p - 1)
        return Point(x: center.x + radius * sin(angle), y: center.y + radius * (1 - cos(angle)))
    }

    static func unsupported(_ step: JSON, iPad: Bool) -> String? {
        let target = step["target"].string
        switch step["do"].string {
        case "launch", "open", "stroke", "repeat", "tap", "save", "close": return nil
        case "gesture":
            return ["pan", "pinch", "rotate"].contains(step["gesture"].string) ? nil : "No Apple mapping for gesture \(step["gesture"].string)"
        case "scrub":
            return step["effect"].isNull || step["effect"].string == "curves" ? nil : "No Apple scrub mapping for effect \(step["effect"].string)"
        case "drag":
            if target.hasPrefix("list:") { return "Native SwiftUI lists scroll without a shared action, so the runner cannot fling \(target)" }
            if ["canvas", "slider:size", "grip:layers", "edge:right-column"].contains(target) || target.hasPrefix("object:") { return nil }
            return "No Apple mapping for drag target \(target)"
        case "resume": return iPad ? "An iPadOS app cannot send itself to the background and return" : nil
        default: return "Unknown step kind \(step["do"].string)"
        }
    }
    static func unsupported(item: String, iPad: Bool) -> String? {
        guard item == "menu:main" else { return nil }
        return iPad ? "The iPad main menu is a SwiftUI menu that only a real tap opens"
            : "The Mac main menu is the system menu bar, outside the app's window"
    }
}
