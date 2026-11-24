import Foundation

@main struct ChainPlanChecks {
    static func close(_ a: Double, _ b: Double) -> Bool { abs(a - b) < 1e-9 }
    @MainActor static func main() async throws {
        let source = #"""
        {"schema": 1, "motion_frames": 150, "documents": {"photo": {"kind": "photo"}, "heavy": {"kind": "capy"}},
         "steps": [{"id": "launch", "do": "launch"}, {"id": "draw.wet", "do": "stroke", "duration_ms": 1000},
          {"id": "filter.scrub", "do": "scrub", "effect": "curves", "frames": 90}, {"id": "nav.pan", "do": "gesture", "gesture": "pan"}]}
        """#
        let plan = try ChainPlan(data: Data(source.utf8))
        precondition(plan.steps.map { $0["id"].string } == ["launch", "draw.wet", "filter.scrub", "nav.pan"])
        precondition(plan.documents["photo"]["kind"].string == "photo")
        var performed: [String] = [], blocked: [(String, String)] = []
        let timeout = ChainPlan.RenderingTimeout().localizedDescription
        let stopped = await plan.run(perform: { step in
            let id = step["id"].string; performed.append(id)
            return id == "draw.wet" ? timeout : nil
        }, notRun: { blocked.append(($0["id"].string, $1)) })
        precondition(stopped == timeout && performed == ["launch", "draw.wet"])
        precondition(blocked.map(\.0) == ["filter.scrub", "nav.pan"])
        precondition(blocked.allSatisfy { $0.1 == "not run: " + timeout })
        performed = []; blocked = []
        let completed = await plan.run(perform: { performed.append($0["id"].string); return nil },
            notRun: { blocked.append(($0["id"].string, $1)) })
        precondition(completed == nil && performed == plan.steps.map { $0["id"].string } && blocked.isEmpty)
        performed = []; blocked = []
        let startup = await plan.run(perform: { performed.append($0["id"].string); return timeout },
            notRun: { blocked.append(($0["id"].string, $1)) })
        precondition(startup == timeout && performed == ["launch"])
        precondition(blocked.map(\.0) == ["draw.wet", "filter.scrub", "nav.pan"])
        precondition(blocked.allSatisfy { $0.1 == "not run: " + timeout })

        precondition(close(plan.seconds(plan.steps[1], refreshHz: 120), 1))
        precondition(close(plan.seconds(plan.steps[2], refreshHz: 120), 0.75))
        precondition(close(plan.seconds(plan.steps[3], refreshHz: 60), 2.5))
        let only = try ChainPlan(data: Data(source.utf8), only: ["launch", "nav.pan"])
        precondition(only.steps.map { $0["id"].string } == ["launch", "nav.pan"])
        for invalid in [#"{"schema": 2, "steps": [{"id": "a"}]}"#, #"{"schema": 1, "steps": []}"#, "[]"] {
            do { _ = try ChainPlan(data: Data(invalid.utf8)); preconditionFailure("Invalid chain accepted: \(invalid)") } catch {}
        }
        do { _ = try ChainPlan(data: Data(source.utf8), only: ["launch", "missing"]); preconditionFailure("Unknown step accepted") } catch {}
        let documents = try ChainPlan(data: Data(#"""
        {"schema": 1, "steps": [{"id": "launch", "do": "launch"}, {"id": "nav.pan", "do": "gesture"}, {"id": "open.a", "do": "open", "document": "a"},
         {"id": "a.draw", "do": "stroke"}, {"id": "open.b", "do": "open", "document": "b"}, {"id": "switch", "do": "repeat", "tabs": ["a", "b"]},
         {"id": "save.a", "do": "save", "document": "a"}, {"id": "reopen.a", "do": "open", "document": "a", "reopen": true}, {"id": "close", "do": "close"}]}
        """#.utf8))
        precondition(documents.needs == ["launch": [], "nav.pan": [], "a.draw": ["a"], "switch": ["a", "b", "b"], "save.a": ["a", "b"], "close": []])

        precondition(ChainPlan.brushSize(JSON("guarantee"), pixels: 12e6) == 1024)
        precondition(ChainPlan.brushSize(JSON("guarantee"), pixels: 24e6) == 1536)
        precondition(ChainPlan.brushSize(JSON("guarantee"), pixels: 61e6) == 2048)
        precondition(ChainPlan.brushSize(JSON(512), pixels: 61e6) == 512)
        precondition(ChainPlan.command("magic_wand") == "auto_select" && ChainPlan.command("transform") == "scale_rotate")
        precondition(ChainPlan.command("lasso") == "lasso")

        for path in ["ellipse", "zigzag"] {
            for index in 0...200 {
                let progress = Double(index) / 200, point = ChainPlan.stroke(path, seconds: progress * 1.25, progress: progress)
                precondition(point.x >= 0.2 - 1e-9 && point.x <= 0.8 + 1e-9 && point.y >= 0.3 - 1e-9 && point.y <= 0.7 + 1e-9)
                precondition(point.pressure >= 0.25 - 1e-9 && point.pressure <= 1)
            }
            precondition(close(ChainPlan.stroke(path, seconds: 0, progress: 0).pressure, 0.25))
            precondition(close(ChainPlan.stroke(path, seconds: 0.5, progress: 0.5).pressure, 1))
        }
        precondition(ChainPlan.stroke("ellipse", seconds: 0, progress: 0) == ChainPlan.Point(x: 0.8, y: 0.5, pressure: 0.25))
        let zigzag = ChainPlan.stroke("zigzag", seconds: 0, progress: 0)
        precondition(close(zigzag.x, 0.2) && close(zigzag.y, 0.3))

        let target = ChainPlan.Point(x: 0.5, y: 0.5)
        precondition(ChainPlan.drag("arc", from: target, progress: 0) == target)
        let arc = ChainPlan.drag("arc", from: target, progress: 1)
        precondition(close(arc.x, 0.65) && close(arc.y, 0.5) && ChainPlan.drag("arc", from: target, progress: 0.5).y < 0.5)
        let loopStart = ChainPlan.drag("loop", from: target, progress: 0), loopEnd = ChainPlan.drag("loop", from: target, progress: 1)
        precondition(close(loopStart.x, loopEnd.x) && close(loopStart.y, loopEnd.y))

        precondition(ChainPlan.gesture("pinch", progress: 0) == ChainPlan.Pose())
        precondition(close(ChainPlan.gesture("pinch", progress: 0.5).scale, 2.5) && close(ChainPlan.gesture("pinch", progress: 1).scale, 1))
        precondition(ChainPlan.gesture("pinch", progress: 1.05).scale < 1)
        precondition(close(ChainPlan.gesture("pinch", progress: 0.5, scale: 0.4).scale, 0.4))
        precondition(close(ChainPlan.gesture("rotate", progress: 0.5).rotation, .pi / 3))
        precondition(close(ChainPlan.gesture("pan", progress: 0.5).pan, 0.3) && close(ChainPlan.gesture("pan", progress: 1).pan, 0))
        let area = [100.0, 50, 2000, 1000]
        let resting = ChainPlan.fingers(ChainPlan.Pose(), area: area), pinched = ChainPlan.fingers(ChainPlan.Pose(scale: 2.5), area: area)
        precondition(resting.count == 2 && close((resting[0].x + resting[1].x) / 2, 1100) && close(resting[0].y, 550))
        precondition(close(resting[0].x - resting[1].x, 200) && close(pinched[0].x - pinched[1].x, 500))
        let turned = ChainPlan.fingers(ChainPlan.Pose(rotation: .pi / 2), area: area)
        precondition(close(turned[0].x, 1100) && close(turned[0].y - turned[1].y, 200))
        let panned = ChainPlan.fingers(ChainPlan.Pose(pan: 0.3), area: area)
        precondition(close((panned[0].x + panned[1].x) / 2, 1700))

        let start = ChainPlan.Point(x: 900, y: 40), center = ChainPlan.Point(x: 400, y: 300)
        precondition(ChainPlan.floatAndReturn(from: start, center: center, radius: 50, progress: 0) == start)
        precondition(ChainPlan.floatAndReturn(from: start, center: center, radius: 50, progress: 1) == start)
        for progress in [1.0 / 3, 2.0 / 3] {
            let point = ChainPlan.floatAndReturn(from: start, center: center, radius: 50, progress: progress)
            precondition(abs(point.x - center.x) < 1e-6 && abs(point.y - center.y) < 1e-6)
        }
        precondition(close(ChainPlan.floatAndReturn(from: start, center: center, radius: 50, progress: 0.5).y, 400))
        precondition(close(ChainPlan.triangle(0), 0) && close(ChainPlan.triangle(0.5), 1) && close(ChainPlan.triangle(1), 0))

        func step(_ text: String) -> JSON { try! JSON.decode(text) }
        for text in [#"{"do": "launch"}"#, #"{"do": "drag", "target": "canvas"}"#, #"{"do": "drag", "target": "object:Image"}"#,
            #"{"do": "drag", "target": "grip:layers"}"#, #"{"do": "drag", "target": "edge:right-column"}"#,
            #"{"do": "drag", "target": "slider:size"}"#, #"{"do": "scrub", "effect": "curves"}"#,
            #"{"do": "scrub", "layer_opacity": [1, 0.2]}"#, #"{"do": "gesture", "gesture": "rotate"}"#] {
            precondition(ChainPlan.unsupported(step(text), iPad: true) == nil, text)
        }
        precondition(ChainPlan.unsupported(step(#"{"do": "drag", "target": "list:brushes"}"#), iPad: false) != nil)
        precondition(ChainPlan.unsupported(step(#"{"do": "drag", "target": "pane:x"}"#), iPad: false) != nil)
        precondition(ChainPlan.unsupported(step(#"{"do": "scrub", "effect": "levels"}"#), iPad: false) != nil)
        precondition(ChainPlan.unsupported(step(#"{"do": "gesture", "gesture": "swipe"}"#), iPad: false) != nil)
        precondition(ChainPlan.unsupported(step(#"{"do": "teleport"}"#), iPad: false) != nil)
        precondition(ChainPlan.unsupported(step(#"{"do": "resume"}"#), iPad: false) == nil)
        precondition(ChainPlan.unsupported(step(#"{"do": "resume"}"#), iPad: true) != nil)
        precondition(ChainPlan.unsupported(item: "menu:main", iPad: false) != nil && ChainPlan.unsupported(item: "menu:main", iPad: true) != nil)
        precondition(ChainPlan.unsupported(item: "panel:brushes", iPad: true) == nil)
        print("PASS: chain parsing, durations, brush sizes, stroke, drag, gesture and panel paths, and Apple step mapping")
    }
}
