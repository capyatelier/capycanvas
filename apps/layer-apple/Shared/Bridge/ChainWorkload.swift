import Foundation
import Metal
import CryptoKit
#if os(macOS)
import AppKit
#else
import UIKit
import os
#endif

@MainActor final class ChainWorkload {
    private final class Step {
        let id: String, surface: String
        var status = "ok", reason: String?, stroke = false
        var begin: UInt64 = 0, end: UInt64 = 0, settled: UInt64?
        var motion: [[String: UInt64]] = [], contacts: [[String: UInt64]] = [], completions: [[String: UInt64]] = []
        var latency: [[String: Any]] = [], extra: [String: Any] = [:]
        init(id: String, surface: String) { self.id = id; self.surface = surface }
        var json: [String: Any] {
            ["id": id, "status": status, "reason": reason as Any? ?? NSNull(), "input": "dispatch", "begin_ns": begin, "end_ns": end,
             "surface": surface, "motion": motion, "contacts": contacts, "completions": completions,
             "settled_ns": settled as Any? ?? NSNull(), "latency": latency, "extra": extra]
        }
    }
    private struct Skip: Error { let reason: String }
    #if os(macOS)
    private let iPad = false
    #else
    private let iPad = true
    #endif
    private weak var store: EditorStore?
    private let plan: DrawingWorkloadPlan
    private let chain: DrawingWorkloadPlan.Chain
    private let created = FrameTrace.now()
    private let device = MTLCreateSystemDefaultDevice()
    private var records: [Step] = []
    private var memory: [[String: Any]] = []
    private var memoryTimer: Timer?
    private var launched: (input: UInt64?, ready: UInt64)?
    private var tabs: [String: UInt64] = [:]
    private var paintLayers: [UInt64: UInt64] = [:]
    private var canvas: [UInt64]?
    private var refreshHz = 60.0
    private var nextContact: UInt64 = 1 << 32

    static var saveURL: URL { FileManager.default.temporaryDirectory.appendingPathComponent("capy-bench-save.capy") }
    static var dialogs: ProjectFiles.Dialogs {
        .init(open: { _, done in done([]) }, save: { _, _, done in done(saveURL) }, export: { staging, done in
            let destination = saveURL
            NativeProjectTask.io.async {
                do {
                    try FileManager.default.copyItem(at: staging, to: destination)
                    DispatchQueue.main.async { done(destination) }
                } catch { DispatchQueue.main.async { done(nil) } }
            }
        })
    }

    init(store: EditorStore, plan: DrawingWorkloadPlan) {
        self.store = store; self.plan = plan; chain = plan.chain!
        let timer = Timer(timeInterval: 1, repeats: true) { [weak self] _ in MainActor.assumeIsolated { self?.sampleMemory() } }
        memoryTimer = timer
        RunLoop.main.add(timer, forMode: .common)
        sampleMemory()
        Task { await self.run() }
    }
    private var editor: EditorStore {
        get throws {
            guard let store else { throw HostFailure(message: "The editor window closed") }
            return store
        }
    }

    private func run() async {
        var failure: String?, digest = ""
        do {
            let data = try Data(contentsOf: chain.file)
            digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            let steps = try ChainPlan(data: data, only: chain.steps)
            failure = await steps.run(perform: { await self.perform($0, steps) }, notRun: { self.notRun($0, reason: $1) })
        } catch { failure = error.localizedDescription }
        await finish(failure: failure, digest: digest)
    }
    private func record(_ step: JSON) -> Step {
        let target = step["target"].string
        let record = Step(id: step["id"].string, surface: !step["surface"].isNull ? step["surface"].string
            : target.contains(":") && !target.hasPrefix("object:") ? "window" : "canvas")
        records.append(record)
        record.begin = FrameTrace.now()
        return record
    }
    private func notRun(_ step: JSON, reason: String) {
        let record = record(step)
        record.status = "error"; record.reason = reason; record.end = record.begin
    }
    private func perform(_ step: JSON, _ steps: ChainPlan) async -> String? {
        let record = record(step), revision = store?.state["document_file"]["revision"].uint ?? 0
        var stopped: String?
        do {
            if launched == nil { launched = try await launch() }
            if let reason = ChainPlan.unsupported(step, iPad: iPad) { throw Skip(reason: reason) }
            guard Double(FrameTrace.now() - created) < plan.seconds * 1e9 else {
                throw HostFailure(message: "The pass ran past CAPY_WORKLOAD_SECONDS (\(Int(plan.seconds)) s)")
            }
            try await execute(step, steps, record)
        } catch let skip as Skip {
            record.status = "skipped"; record.reason = skip.reason
        } catch {
            record.status = "failed"; record.reason = error.localizedDescription
            if launched == nil || error is ChainPlan.RenderingTimeout { stopped = error.localizedDescription }
            store?.failure = nil
        }
        if record.end == 0 { record.end = FrameTrace.now() }
        record.extra["document_revision"] = [revision, store?.state["document_file"]["revision"].uint ?? 0]
        NSLog("Capy chain %@ %@ %@", record.id, record.status, record.reason ?? "")
        return stopped
    }
    private func execute(_ step: JSON, _ steps: ChainPlan, _ record: Step) async throws {
        switch step["do"].string {
        case "launch":
            guard let launched, let input = launched.input else { throw HostFailure(message: "The process start time is unavailable") }
            record.begin = input; record.end = launched.ready
            record.latency.append(["name": "launch", "input_ns": input, "ready_ns": launched.ready, "surface": "canvas"])
        case "open": try await open(step["document"].string, steps, record)
        case "stroke": try await stroke(step, steps, record)
        case "gesture": try await gesture(step, steps, record)
        case "drag": try await drag(step, steps, record)
        case "tap": try await tap(step, record)
        case "repeat": try await repeatItems(step, record)
        case "scrub": try await scrub(step, steps, record)
        case "save": try await save(step["document"].string, record)
        case "resume": try await resume(record)
        default: try await close(record)
        }
    }

    private func poll(_ seconds: Double, _ condition: () async -> Bool) async throws -> Bool {
        let deadline = FrameTrace.now() + UInt64(seconds * 1e9)
        while true {
            if let failure = try editor.failure { throw HostFailure(message: failure) }
            if await condition() { return true }
            if FrameTrace.now() >= deadline { return false }
            try await Task.sleep(for: .milliseconds(4))
        }
    }
    private func wait(_ what: String, seconds: Double = 60, _ condition: () async -> Bool) async throws {
        guard try await poll(seconds, condition) else { throw HostFailure(message: "Timed out after \(Int(seconds)) s waiting for \(what)") }
    }
    private func pending() async -> Bool {
        guard let native = store?.native else { return true }
        return await withCheckedContinuation { continuation in native.renderingPending { continuation.resume(returning: $0) } }
    }
    private func settled(within seconds: Double) async throws -> UInt64? {
        let idle = try await poll(seconds) { await !pending() }
        return idle ? FrameTrace.now() : nil
    }
    @discardableResult private func settle() async throws -> UInt64 {
        guard let time = try await settled(within: 60) else { throw ChainPlan.RenderingTimeout() }
        return time
    }
    private func measure(_ record: Step, _ name: String, since input: UInt64) async throws {
        let ready = try await settle()
        record.latency.append(["name": name, "input_ns": input, "ready_ns": ready, "surface": record.surface])
    }
    private func edit(_ value: [String: Any]) async throws { try await editor.workloadEdit(value) }
    private func invoke(_ command: String) async throws {
        let state = try editor.command(command)
        if !state["enabled"].isNull && !state["enabled"].bool { throw HostFailure(message: "Command \(command) is disabled") }
        try await edit(["type": "invoke", "command": command])
    }
    private func selectLayer(_ label: String?) async throws {
        let store = try editor, id: UInt64
        if let label {
            let layers = store.state["layers"].array
            guard let layer = layers.first(where: { $0["label"].string == label }) else {
                throw HostFailure(message: "No layer is labelled \(label); layers: \(layers.map { $0["label"].string }.joined(separator: ", "))")
            }
            id = layer["id"].uint
        } else {
            guard let paint = paintLayers[store.snapshot["document_tabs"]["selected"].uint] else { return }
            id = paint
        }
        try await edit(["type": "select_layer", "id": id])
    }
    private func area() throws -> [Double] {
        let area = try editor.state["camera"]["work_area"].array.map(\.number)
        guard area.count == 4, area[2] > 0, area[3] > 0 else { throw HostFailure(message: "The canvas has no work area") }
        return area
    }
    private func pixel(_ point: ChainPlan.Point, _ area: [Double]) -> ChainPlan.Point {
        ChainPlan.Point(x: area[0] + point.x * area[2], y: area[1] + point.y * area[3], pressure: point.pressure)
    }

    private func paced(_ count: Int, hz: Double, _ deliver: (Range<Int>, UInt64) throws -> Void) async throws -> (down: UInt64, up: UInt64) {
        let start = FrameTrace.now()
        var next = 0
        while next <= count {
            let store = try editor
            if let failure = store.failure { throw HostFailure(message: failure) }
            let due = min(count, Int(Double(FrameTrace.now() - start) / 1e9 * hz))
            if due >= next { try deliver(next..<(due + 1), start); next = due + 1; store.wake?() }
            if next <= count { try await Task.sleep(for: .milliseconds(2)) }
        }
        return (start, start + UInt64(Double(count) / hz * 1e9))
    }
    private func contact(tool: UInt32 = 0, fingers: Int = 1, hz: Double, seconds: Double,
        _ points: (Double, Double) -> [ChainPlan.Point]) async throws -> (down: UInt64, up: UInt64) {
        var ids: [UInt64] = []
        for _ in 0..<fingers { nextContact += 1; ids.append(nextContact) }
        let count = max(1, Int((seconds * hz).rounded()))
        return try await paced(count, hz: hz) { range, start in
            var batches = [[Double]](repeating: [], count: fingers)
            for index in range {
                let time = start + UInt64(Double(index) / hz * 1e9)
                for (finger, point) in points(Double(index) / hz, Double(index) / Double(count)).enumerated() {
                    batches[finger] += EditorStore.workloadSample(x: point.x, y: point.y, pressure: point.pressure, time: time,
                        phase: index == 0 ? 1 : index == count ? 3 : 2)
                }
            }
            for (finger, batch) in batches.enumerated() { store?.workloadPointer(ids[finger], tool: tool, records: batch) }
        }
    }

    private func launch() async throws -> (input: UInt64?, ready: UInt64) {
        try await wait("the editor, brush and shaders to become ready", seconds: 180) {
            guard let store = self.store else { return false }
            return store.workloadReady && store.snapshot["brush_ready"].bool && !store.launchVisible
                && !store.recovery.restoring && !store.projectFiles.busy
        }
        let ready = try await settle()
        let store = try editor
        try await store.applyWorkloadAppearance(theme: plan.theme)
        #if os(macOS)
        let screen = store.projectFiles.presentationWindow?.screen ?? NSScreen.main
        #else
        let screen = store.projectFiles.presentationScreen
        #endif
        refreshHz = Double(screen?.maximumFramesPerSecond ?? 60)
        return (Self.processStart(), ready)
    }
    private static func processStart() -> UInt64? {
        var info = kinfo_proc(), size = MemoryLayout<kinfo_proc>.stride
        var name: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&name, 4, &info, &size, nil, 0) == 0 else { return nil }
        let start = info.kp_proc.p_un.__p_starttime
        let age = Date().timeIntervalSince1970 - (Double(start.tv_sec) + Double(start.tv_usec) / 1e6)
        let now = FrameTrace.now()
        guard age > 0, age < 3600, UInt64(age * 1e9) < now else { return nil }
        return now - UInt64(age * 1e9)
    }

    private func open(_ name: String, _ steps: ChainPlan, _ record: Step) async throws {
        let store = try editor, files = store.projectFiles
        guard let url = chain.document(name) else { throw Skip(reason: "CAPY_WORKLOAD_\(name.uppercased()) names no \(name) document") }
        guard FileManager.default.fileExists(atPath: url.path) else { throw HostFailure(message: "The \(name) document is missing at \(url.path)") }
        let epoch = store.state["document_file"]["epoch"].uint, tab = store.snapshot["document_tabs"]["selected"].uint
        let input = FrameTrace.now()
        files.openURL(url)
        try await wait("the \(name) document to open", seconds: 180) {
            files.error != nil || files.pendingProfile != nil || files.packageSummary != nil || (!files.busy
                && (store.state["document_file"]["epoch"].uint != epoch || store.snapshot["document_tabs"]["selected"].uint != tab))
        }
        if let error = files.error { files.error = nil; throw HostFailure(message: error) }
        if files.pendingProfile != nil { files.cancel(); throw HostFailure(message: "Opening \(name) asks for a color profile choice") }
        if files.packageSummary != nil { files.closePackage(); throw HostFailure(message: "The \(name) file opened as a package summary") }
        if steps.documents[name]["kind"].string == "photo" {
            guard let layer = store.state["layer_tools"]["editing_layer"]["id"].optionalUInt else {
                throw HostFailure(message: "The imported photo has no editable layer")
            }
            try await edit(["type": "layer", "action": ["op": "rename", "id": layer, "name": "Photo"]])
            try await invoke("add_layer"); try await settle()
        }
        try await measure(record, "open." + name, since: input)
        let selected = store.snapshot["document_tabs"]["selected"].uint
        tabs[name] = selected
        paintLayers[selected] = store.state["layer_tools"]["editing_layer"]["id"].optionalUInt
        let active = store.state["tabs"].array.first { $0["active"].bool } ?? JSON()
        record.extra["canvas"] = [active["width"].uint, active["height"].uint]
        if canvas == nil { canvas = [active["width"].uint, active["height"].uint] }
    }
    private func stroke(_ step: JSON, _ steps: ChainPlan, _ record: Step) async throws {
        let store = try editor, preset = step["preset"].uint
        record.stroke = true
        try await selectLayer(step["layer"].isNull ? nil : step["layer"].string)
        try await edit(["type": "select_brush", "id": preset])
        guard store.state["brush"]["preset"].uint == preset else { throw HostFailure(message: "Brush preset \(preset) is unavailable") }
        let active = store.state["tabs"].array.first { $0["active"].bool } ?? JSON()
        try await edit(["type": "set_brush_size", "value": ChainPlan.brushSize(step["size"], pixels: active["width"].number * active["height"].number)])
        if step["zoom"].string == "fit" { try await invoke("fit_canvas") }
        else if !step["zoom"].isNull { try await edit(["type": "set_zoom", "zoom": step["zoom"].number]) }
        if ["clone", "heal", "spot_heal"].contains(store.state["brush"]["tool"].string) {
            let reference = store.command("use_reference_below")["enabled"].bool
            record.extra["reference_below"] = reference
            if reference { try await invoke("use_reference_below") }
        }
        try await wait("brush \(preset) to be ready", seconds: 120) { store.snapshot["brush_ready"].bool }
        try await settle()
        record.extra["brush"] = ["preset": preset, "diameter": store.state["brush"]["diameter"].number, "zoom": store.state["camera"]["zoom"].number]
        let area = try area(), path = step["path"].string
        let point = { [self] (t: Double, p: Double) in [pixel(ChainPlan.stroke(path, seconds: t, progress: p), area)] }
        _ = try await contact(hz: ChainPlan.penHz, seconds: 0.1, point)
        try await settle()
        let contacts = max(1, Int(step["contacts"].number))
        let seconds = steps.seconds(step, refreshHz: refreshHz) / Double(contacts)
        for index in 0..<contacts {
            if index > 0 { try await Task.sleep(for: .milliseconds(100)) }
            let window = try await contact(hz: ChainPlan.penHz, seconds: seconds, point)
            record.contacts.append(["down_ns": window.down, "up_ns": window.up])
            record.motion.append(["begin_ns": window.down, "end_ns": window.up])
        }
        let cap = step["catch_up_cap_ms"].isNull ? 3.0 : step["catch_up_cap_ms"].number / 1000
        record.settled = try await settled(within: cap)
        if record.settled == nil { record.extra["catch_up_capped"] = true; try await settle() }
    }
    private func gesture(_ step: JSON, _ steps: ChainPlan, _ record: Step) async throws {
        let store = try editor, kind = step["gesture"].string, area = try area()
        let seconds = steps.seconds(step, refreshHz: refreshHz), quick = step["release"].string == "quick"
        let camera = store.cameraRevision
        #if os(macOS)
        let scale = Double(store.projectFiles.presentationWindow?.backingScaleFactor ?? 2)
        let count = max(1, Int(((seconds + (quick ? 0.1 : 0)) * ChainPlan.touchHz).rounded()))
        var last = ChainPlan.gesture(kind, progress: 0)
        let window = try await paced(count, hz: ChainPlan.touchHz) { range, _ in
            for index in range {
                let pose = ChainPlan.gesture(kind, progress: Double(index) / ChainPlan.touchHz / seconds)
                let x = Float(area[0] + area[2] * (0.5 + pose.pan)), y = Float(area[1] + area[3] / 2)
                if kind == "pan" {
                    store.native?.scroll(x: x, y: y, dx: Float((last.pan - pose.pan) * area[2] / scale), dy: 0,
                        scale: Float(scale), zoom: false, horizontal: false)
                } else {
                    store.native?.gesture(x: x, y: y, scale: Float(pose.scale / last.scale), rotation: Float(pose.rotation - last.rotation),
                        began: index == 0)
                }
                last = pose
            }
        }
        record.extra["apple_input"] = kind == "pan" ? "trackpad scroll" : "trackpad gesture"
        #else
        let window = try await contact(tool: 3, fingers: 2, hz: ChainPlan.touchHz, seconds: seconds + (quick ? 0.1 : 0)) { t, _ in
            ChainPlan.fingers(ChainPlan.gesture(kind, progress: t / seconds), area: area)
        }
        record.extra["apple_input"] = "touch"
        #endif
        record.motion.append(["begin_ns": window.down, "end_ns": window.up])
        if quick { record.settled = try await cameraSettled() } else { try await settle() }
        let revisions = store.cameraRevision &- camera
        record.extra["camera_revisions"] = revisions
        if revisions == 0 { throw HostFailure(message: "The camera did not move during the \(kind) gesture") }
    }
    private func cameraSettled() async throws -> UInt64 {
        var revision = try editor.cameraRevision, stable = FrameTrace.now()
        try await wait("the camera to stop", seconds: 10) { [self] in
            let current = store?.cameraRevision ?? 0
            if current != revision { revision = current; stable = FrameTrace.now(); return false }
            guard FrameTrace.now() - stable > 50_000_000 else { return false }
            return await !pending()
        }
        return stable
    }
    private func drag(_ step: JSON, _ steps: ChainPlan, _ record: Step) async throws {
        let target = step["target"].string, seconds = steps.seconds(step, refreshHz: refreshHz)
        if step["select"].string == "all" { try await invoke("select_all") }
        for key in ["tool", "command"] where !step[key].isNull { try await invoke(ChainPlan.command(step[key].string)) }
        if step["command"].string == "transform" {
            try await wait("the transform controls to become ready") {
                guard let store = self.store else { return false }
                return store.state["tool_settings"].array.contains { $0["id"].string == "transform_x" }
                    && store.snapshot["brush_ready"].bool
            }
        }
        let window: (down: UInt64, up: UInt64)
        switch target {
        case "slider:size": window = try await slider(seconds)
        case "grip:layers": window = try await panelDrag(seconds, record)
        case "edge:right-column": window = try await dividerDrag(seconds, record)
        default:
            if target.hasPrefix("object:") { try await selectLayer(String(target.dropFirst(7))); try await invoke("fit_canvas") }
            try await settle()
            let area = try area(), path = step["path"].string
            window = try await contact(hz: ChainPlan.penHz, seconds: seconds) { [self] _, p in
                [pixel(ChainPlan.drag(path, from: ChainPlan.Point(x: 0.5, y: 0.5), progress: p), area)]
            }
            record.contacts.append(["down_ns": window.down, "up_ns": window.up])
        }
        record.motion.append(["begin_ns": window.down, "end_ns": window.up])
        try await settle()
        if step["then"].string == "apply" { try await invoke("apply_transform"); try await settle() }
    }
    private func slider(_ seconds: Double) async throws -> (down: UInt64, up: UInt64) {
        let store = try editor
        let controls = store.workspace.sourceInstances.values.filter { $0.surface == .control && !$0.bounds.isEmpty }
            .sorted { ($0.bounds.minY, $0.bounds.minX, $0.item) < ($1.bounds.minY, $1.bounds.minX, $1.item) }
        let component = controls.lazy.compactMap { source -> JSON? in
            guard let item = try? JSON.decode(source.item), item["kind"].string == "tile",
                let tile = store.panel(item["panel"].string)["tiles"].array.first(where: { $0["id"].uint == item["tile"].uint }),
                tile["control"]["kind"].string == "brush_size_slider" else { return nil }
            let center = CGPoint(x: source.bounds.midX, y: source.bounds.midY)
            let model = tile["component"]
            guard store.workspace.hitSource(at: center)?.1 == source,
                !model["context"].isNull, !model["numeric"].isNull else { return nil }
            return model
        }.first
        guard let component else { throw Skip(reason: "No enabled brush size slider is visible in this workspace") }
        let field = component["numeric"], spec = field["numeric"]
        let size = field["value"].number, maximum = spec["soft_max"].number
        let peak = size < maximum ? min(size + 120, maximum) : min(maximum, max(spec["soft_min"].number, size - 120))
        var moveFailure: String?
        let count = max(1, Int((seconds * refreshHz).rounded()))
        let window = try await paced(count, hz: refreshHz) { range, _ in
            for index in range {
                let progress = ChainPlan.triangle(Double(index) / Double(count))
                let value = size + (peak - size) * progress
                store.toolbarEdit(component, ["type": "set_tool_setting", "id": field["id"].string, "value": value], moving: true) { error in
                    if let error, moveFailure == nil { moveFailure = error }
                }
            }
        }
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            store.toolbarEdit(component, ["type": "set_tool_setting", "id": field["id"].string, "value": size], moving: false) { error in
                if let failure = moveFailure ?? error { continuation.resume(throwing: HostFailure(message: failure)) }
                else { continuation.resume() }
            }
        }
        return window
    }
    private func panelDrag(_ seconds: Double, _ record: Step) async throws -> (down: UInt64, up: UInt64) {
        let store = try editor
        func layersItem() -> JSON? {
            let layout = store.snapshot["layout"]
            if let group = layout["groups"].array.first(where: { $0["panels"].array.contains { $0.string == "layers" } }) {
                return JSON(["kind": "group", "group": group["id"].uint])
            }
            if let column = layout["collapsed"].array.first(where: { column in
                column["groups"].array.contains { $0["icons"].array.contains { $0["panel"].string == "layers" } }
            }) {
                return JSON(["kind": "column", "column": column["id"].uint])
            }
            return nil
        }
        let temporary = layersItem() == nil
        let window: (down: UInt64, up: UInt64)
        do {
            if temporary { try await edit(["type": "customize", "action": ["type": "set_panel_visible", "panel": "layers", "visible": true]]) }
            var grip: (target: ReorderTarget, origin: CGPoint)?
            var item: JSON?
            try await wait("a visible Layers grip", seconds: 5) {
                item = layersItem()
                grip = item.flatMap { self.workspaceGrip($0, store: store) }
                return grip != nil
            }
            guard let grip, let item else { throw HostFailure(message: "The Layers grip is unavailable") }
            let area = store.snapshot["layout"]["work_area"].rect
            let start = ChainPlan.Point(x: grip.origin.x, y: grip.origin.y)
            let center = ChainPlan.Point(x: area.midX, y: area.midY), radius = 0.1 * min(area.width, area.height)
            let kind = item["kind"].string
            record.extra[kind] = item[kind].uint
            window = try await workspaceDrag(grip.target, from: grip.origin, seconds, record) {
                ChainPlan.floatAndReturn(from: start, center: center, radius: radius, progress: $0)
            }
        } catch {
            if temporary { try? await showPanel("layers", false) }
            throw error
        }
        if temporary { try await showPanel("layers", false) }
        return window
    }
    private func dividerDrag(_ seconds: Double, _ record: Step) async throws -> (down: UInt64, up: UInt64) {
        let store = try editor
        guard let divider = store.snapshot["layout"]["dividers"].array
            .filter({ !$0["fixed"].bool && $0["bounds"].rect.height > $0["bounds"].rect.width })
            .max(by: { $0["bounds"].rect.minX < $1["bounds"].rect.minX }),
            let grip = workspaceGrip(JSON(["type": "drag_divider", "id": divider["id"].uint]), store: store) else {
            throw HostFailure(message: "No resizable column edge is visible")
        }
        record.extra["divider"] = divider["id"].uint
        return try await workspaceDrag(grip.target, from: grip.origin, seconds, record) {
            ChainPlan.Point(x: grip.origin.x - 120 * ChainPlan.triangle($0), y: grip.origin.y)
        }
    }
    private func workspaceGrip(_ item: JSON, store: EditorStore) -> (target: ReorderTarget, origin: CGPoint)? {
        let key = item.stableKey, workspace = store.workspace
        let handles = workspace.sourceInstances.filter { _, source in
            source.item == key && source.surface == .handle && source.canDrag && !source.bounds.isEmpty
        }.sorted { $0.value.bounds.width * $0.value.bounds.height < $1.value.bounds.width * $1.value.bounds.height }
        for (id, source) in handles {
            let point = CGPoint(x: source.bounds.midX, y: source.bounds.midY)
            if workspace.hitSource(at: point)?.0 == id, let target = workspace.input.source(at: point), target.id == id, target.valid(false) {
                return (target, point)
            }
        }
        return nil
    }
    private func workspaceDrag(_ target: ReorderTarget, from origin: CGPoint, _ seconds: Double, _ record: Step,
        _ path: (Double) -> ChainPlan.Point) async throws -> (down: UInt64, up: UInt64) {
        let store = try editor, layout = store.state["workspace"].stableKey
        func acknowledge() async throws {
            await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
                store.query(["type": "renderer_stats"]) { _ in continuation.resume() }
            }
            if let failure = store.failure { throw HostFailure(message: failure) }
        }
        guard target.valid(false) else { throw HostFailure(message: "The workspace grip is no longer available") }
        target.begin(origin)
        var released = false
        defer { if !released { target.cancel() } }
        try await acknowledge()
        let count = max(1, Int((seconds * ChainPlan.touchHz).rounded()))
        let window = try await paced(count, hz: ChainPlan.touchHz) { range, _ in
            guard target.valid(true) else { throw HostFailure(message: "The workspace drag was interrupted") }
            for index in range where index < count {
                let point = path(Double(index) / Double(count))
                target.move(CGPoint(x: point.x, y: point.y))
            }
        }
        try await acknowledge()
        guard target.valid(true) else { throw HostFailure(message: "The workspace drag was interrupted") }
        let finish = path(1), up = FrameTrace.now()
        target.finish(CGPoint(x: finish.x, y: finish.y))
        released = true
        try await acknowledge()
        try await settle()
        if store.state["workspace"].stableKey != layout {
            try await invoke("undo_workspace"); try await settle()
            record.extra["layout_restored"] = store.state["workspace"].stableKey == layout
        }
        return (window.down, up)
    }
    private func tap(_ step: JSON, _ record: Step) async throws {
        try await invoke(ChainPlan.command(step["tool"].string))
        try await settle()
        let area = try area()
        for value in step["points"].array {
            let point = pixel(ChainPlan.Point(x: value[0].number, y: value[1].number), area)
            let window = try await contact(hz: ChainPlan.penHz, seconds: 1 / ChainPlan.penHz) { _, _ in [point] }
            record.contacts.append(["down_ns": window.down, "up_ns": window.up])
            try await measure(record, step["tool"].string, since: window.down)
        }
    }
    private func repeatItems(_ step: JSON, _ record: Step) async throws {
        let store = try editor
        guard let kind = ["commands", "open", "workspaces", "tabs"].first(where: { !step[$0].isNull }) else {
            throw HostFailure(message: "The repeat step lists no commands, open, workspaces or tabs")
        }
        let start = store.workspaces?.view["id"].string ?? ""
        var skipped: [String: String] = [:], shown: [String: Bool] = [:]
        for _ in 0..<max(1, Int(step["count"].number)) {
            for item in step[kind].array.map(\.string) {
                if let reason = ChainPlan.unsupported(item: item, iPad: iPad) { skipped[item] = reason; continue }
                let panel = item.hasPrefix("panel:") ? String(item.dropFirst(6)) : nil
                if let panel {
                    if shown[panel] == nil { shown[panel] = panelVisible(panel) }
                    try await showPanel(panel, false); try await settle()
                }
                let input = FrameTrace.now()
                switch kind {
                case "commands": try await invoke(item)
                case "workspaces": try await switchWorkspace(item, start: start)
                case "tabs": try await switchTab(item)
                default:
                    guard let panel else { throw HostFailure(message: "No Apple mapping opens \(item)") }
                    try await showPanel(panel, true)
                }
                try await measure(record, item, since: input)
            }
        }
        for (panel, visible) in shown { try await showPanel(panel, visible) }
        if !skipped.isEmpty { record.extra["skipped_items"] = skipped }
        if record.latency.isEmpty, let reason = skipped.values.first { throw Skip(reason: reason) }
    }
    private func panelVisible(_ panel: String) -> Bool {
        store?.snapshot["layout"]["groups"].array.contains { $0["panels"].array.contains { $0.string == panel } } == true
    }
    private func showPanel(_ panel: String, _ visible: Bool) async throws {
        try await edit(["type": "customize", "action": ["type": "set_panel_visible", "panel": panel, "visible": visible]])
        try await wait("the \(panel) panel to \(visible ? "show" : "hide")") { panelVisible(panel) == visible }
    }
    private func switchWorkspace(_ name: String, start: String) async throws {
        guard let workspaces = try editor.workspaces else { throw Skip(reason: "This editor has no workspace library") }
        let ids = (workspaces.view["order"].array + workspaces.view["switcher_display"].array.map { $0["id"] }).map(\.string)
        guard let id = name == "start" ? start : ids.first(where: { $0.split(separator: ":").last?.hasPrefix(name) == true }) else {
            throw HostFailure(message: "No workspace matches \(name); workspaces: \(ids.joined(separator: ", "))")
        }
        workspaces.switchTo(id)
        try await wait("workspace \(id)") { workspaces.view["id"].string == id && !workspaces.busy && !workspaces.switcherBusy }
    }
    private func switchTab(_ name: String) async throws {
        let store = try editor
        guard let id = tabs[name] else { throw HostFailure(message: "The \(name) drawing was not opened in this pass") }
        let switched = await withCheckedContinuation { continuation in store.drawingTabs.select(id) { continuation.resume(returning: $0) } }
        guard switched else { throw HostFailure(message: store.projectFiles.error ?? "Switching to the \(name) drawing failed") }
        try await wait("the \(name) drawing to show") { store.snapshot["document_tabs"]["selected"].uint == id }
    }
    private func scrub(_ step: JSON, _ steps: ChainPlan, _ record: Step) async throws {
        let store = try editor
        let count = max(1, Int((steps.seconds(step, refreshHz: refreshHz) * refreshHz).rounded()))
        var layer: UInt64 = 0, epoch: UInt64 = 0
        if !step["effect"].isNull {
            try await selectLayer("Photo")
            try await edit(["type": "effect", "action": ["op": "insert", "effect": step["effect"].string]])
            layer = store.state["layer_tools"]["editing_layer"]["id"].uint; epoch = store.state["document_file"]["epoch"].uint
            try await settle()
        }
        let ramp = step["layer_opacity"].array.map(\.number)
        let window = try await paced(count, hz: refreshHz) { range, _ in
            for index in range {
                let progress = Double(index) / Double(count)
                if ramp.count == 2 {
                    store.dispatch(["type": "set_layer_opacity", "opacity": ramp[0] + (ramp[1] - ramp[0]) * progress], moving: true)
                } else {
                    store.effect(layer, epoch: epoch, key: "rgb", action: ["op": "set", "value": ["kind": "curve",
                        "value": [[0.0, 0.0], [0.5, 0.5 + 0.25 * ChainPlan.triangle(progress)], [1.0, 1.0]]]],
                        phase: index == 0 ? "down" : index == count ? "up" : "move")
                }
            }
        }
        record.motion.append(["begin_ns": window.down, "end_ns": window.up])
        record.settled = try await settle()
        if !step["effect"].isNull { try await invoke("delete_layer"); try await settle() }
    }
    private func save(_ name: String, _ record: Step) async throws {
        let store = try editor, files = store.projectFiles
        guard let id = tabs[name] else { throw HostFailure(message: "The \(name) drawing was not opened in this pass") }
        if store.snapshot["document_tabs"]["selected"].uint != id { try await switchTab(name); try await settle() }
        let url = Self.saveURL
        try? FileManager.default.removeItem(at: url)
        defer { try? FileManager.default.removeItem(at: url) }
        let input = FrameTrace.now()
        try await invoke("save_document_as")
        try await wait("the save to finish") {
            !files.busy && !store.state["requests"].array.contains { $0["kind"]["type"].string == "document" }
        }
        if let error = files.error { throw HostFailure(message: error) }
        try await measure(record, "save", since: input)
        record.extra["bytes"] = (try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.uint64Value ?? 0
    }
    private func resume(_ record: Step) async throws {
        #if os(macOS)
        let store = try editor
        NSApp.hide(nil)
        try await wait("the app to hide", seconds: 5) { NSApp.isHidden }
        try await Task.sleep(for: .seconds(1))
        let input = FrameTrace.now()
        NSApp.unhide(nil); store.focusWindow?()
        try await wait("the window to show", seconds: 10) { store.projectFiles.presentationWindow?.occlusionState.contains(.visible) == true }
        try await measure(record, "resume", since: input)
        #endif
    }
    private func close(_ record: Step) async throws {
        for id in try editor.drawingTabs.rows.map({ $0["id"].uint }).dropLast() {
            let input = FrameTrace.now()
            try await closeTab(id)
            try await measure(record, "close", since: input)
        }
        record.extra["last_drawing"] = "closed after the pass files are written, because closing it closes the window"
    }
    private func closeTab(_ id: UInt64) async throws {
        let store = try editor, files = store.projectFiles
        store.drawingTabs.close(id)
        try await wait("drawing \(id) to close") {
            if files.confirming { files.choose("discard") }
            return store.drawingTabs.closingWindow
                || !store.drawingTabs.rows.contains { $0["id"].uint == id } && !store.drawingTabs.busy && !files.busy
        }
    }

    private func sampleMemory() {
        var sample: [String: Any] = ["t_ns": FrameTrace.now(), "thermal": ProcessInfo.processInfo.thermalState.rawValue]
        let task = FrameTrace.taskMemory()
        if task.status == KERN_SUCCESS { sample["app_rss_kib"] = task.footprint / 1024 }
        if let device { sample["gpu_kib"] = device.currentAllocatedSize / 1024 }
        #if os(iOS)
        sample["os_available_kib"] = os_proc_available_memory() / 1024
        #endif
        memory.append(sample)
    }
    private func finish(failure: String?, digest: String) async {
        memoryTimer?.invalidate(); memoryTimer = nil
        sampleMemory()
        let trace: ([FrameTraceEvent], UInt64) = await withCheckedContinuation { continuation in
            guard let native = store?.native else { continuation.resume(returning: ([], 0)); return }
            native.finishTrace { continuation.resume(returning: ($0, $1)) }
        }
        var presented: [UInt64: UInt64] = [:]
        for event in trace.0 where event.kind == .presented && event.b > 0 { presented[event.a] = min(presented[event.a] ?? .max, event.b) }
        let consumed = trace.0.filter { $0.kind == .consumedPaint }
        for record in records where record.stroke {
            guard let first = record.contacts.first?["down_ns"] else { continue }
            var input: UInt64 = 0, counter: UInt64 = 0
            for event in consumed where event.a <= (record.settled ?? record.end) {
                if event.a >= first && event.b > input && event.c > counter, let time = presented[event.a] {
                    record.completions.append(["complete_ns": time, "input_ns": event.b])
                }
                input = event.a < first ? event.b : max(input, event.b); counter = event.c
            }
        }
        let frames = Set(presented.values).sorted()
        let last = memory.last ?? [:]
        var end: [String: Any] = ["app_rss_kib": last["app_rss_kib"] ?? NSNull(),
            "gpu_allocated_bytes": device.map { $0.currentAllocatedSize as Any } ?? NSNull(),
            "gpu_peak_bytes": memory.compactMap { $0["gpu_kib"] as? Int }.max().map { $0 * 1024 as Any } ?? NSNull()]
        if let available = last["os_available_kib"] { end["os_available_kib"] = available }
        let pass: [String: Any] = ["schema": 1, "host": "apple", "platform": iPad ? "ipados" : "macos",
            "device": chain.label("DEVICE") ?? (iPad ? "ipad" : "mac"),
            "version": chain.label("VERSION") ?? Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown",
            "commit": chain.label("COMMIT") as Any? ?? NSNull(), "app_id": Bundle.main.bundleIdentifier ?? "",
            "pass": chain.pass, "cold": chain.cold, "root": chain.root, "pid": Int(getpid()), "clock": "mach",
            "refresh_hz": refreshHz, "canvas": canvas as Any? ?? NSNull(), "chain_sha256": digest,
            "steps": records.map(\.json), "memory_end": end, "error": failure as Any? ?? NSNull(),
            "trace": ["events": trace.0.count, "dropped_records": trace.1, "presented_frames": frames.count]]
        do {
            try FileManager.default.createDirectory(at: chain.output, withIntermediateDirectories: true)
            try Data(frames.map { "{\"t_ns\":\($0),\"surface\":\"canvas\"}\n" }.joined().utf8)
                .write(to: chain.output.appendingPathComponent("frames.jsonl"), options: .atomic)
            try Data(try memory.map { String(decoding: try JSONSerialization.data(withJSONObject: $0, options: [.sortedKeys]), as: UTF8.self) + "\n" }
                .joined().utf8).write(to: chain.output.appendingPathComponent("memory.jsonl"), options: .atomic)
            try JSONSerialization.data(withJSONObject: pass, options: [.prettyPrinted, .sortedKeys])
                .write(to: chain.output.appendingPathComponent("pass.json"), options: .atomic)
        } catch { NSLog("Capy chain could not write its pass files: %@", error.localizedDescription) }
        NSLog("Capy chain pass %ld finished: %@", chain.pass, failure ?? "ok")
        if failure == nil, launched != nil, let drawing = store?.drawingTabs.rows.last?["id"].uint { try? await closeTab(drawing) }
        #if os(macOS)
        NSApp.terminate(nil)
        #endif
    }
}
