import AppKit
import SwiftUI
import QuartzCore
import ImageIO

@MainActor private final class HDRBubbleProbe: ObservableObject {
    @Published var docked = false
    @Published var component = false
}
private struct HDRBubbleRoot: View {
    @ObservedObject var store: EditorStore
    @ObservedObject var probe: HDRBubbleProbe
    var body: some View {
        Group {
            if probe.component {
                WorkspaceBubbleLayer(store: store, bubbles: store.bubbles)
                    .background(EditorPalette(source: store.state["palette"])["panel"])
                    .coordinateSpace(name: "editor-workspace")
            } else { EditorView(store: store) { MacMetalCanvas(store: store) } }
        }
            .overlay(alignment: .topLeading) {
                if probe.docked {
                    let palette = EditorPalette(source: store.state["palette"])
                    ColorPanel(store: store).frame(width: 264, height: 400)
                        .foregroundStyle(palette["text"]).tint(palette.accent).background(palette["panel"])
                        .padding(.leading, 180).padding(.top, 180).allowsHitTesting(false)
                }
            }
    }
}

@main final class HDRToolBubbleChecks: NativeWorkspaceInputFixture {
    @MainActor static func marker(_ view: NSView) -> ReorderInputView? {
        if let input = view as? ReorderInputView, input.model?.bubbleInput != nil { return input }
        return view.subviews.lazy.compactMap(marker).first
    }
    @MainActor static func stableCapture(_ window: NSWindow, to destination: URL) async throws {
        var previous = Data(), stable = 0
        for _ in 0..<40 {
            try await drain(0.025)
            try await capture(window, to: destination)
            let pixels = try await Task.detached(priority: .utility) {
                guard let source = CGImageSourceCreateWithURL(destination as CFURL, nil),
                    let image = CGImageSourceCreateImageAtIndex(source, 0, nil), let data = image.dataProvider?.data else {
                    throw HostFailure(message: "The actual HDR capture must expose its pixels")
                }
                return data as Data
            }.value
            stable = pixels == previous ? stable + 1 : 0; previous = pixels
            if stable >= 2 { return }
        }
        throw HostFailure(message: "The actual HDR wheel pixels did not settle")
    }
    @MainActor static func pixel(_ window: NSWindow, marker: NSView, point: CGPoint, to destination: URL) async throws -> [Int] {
        try await pixels(window, marker: marker, points: [point], to: destination)[0]
    }
    @MainActor static func pixels(_ window: NSWindow, marker: NSView, points: [CGPoint], to destination: URL) async throws -> [[Int]] {
        let samples = points.map { marker.convert($0, to: nil) }, size = window.frame.size
        try await stableCapture(window, to: destination)
        return try await Task.detached(priority: .utility) {
            guard let source = CGImageSourceCreateWithURL(destination as CFURL, nil),
                let image = CGImageSourceCreateImageAtIndex(source, 0, nil), let space = CGColorSpace(name: CGColorSpace.sRGB) else {
                throw HostFailure(message: "The owned triangle capture must decode")
            }
            return try samples.map { sample in
                let x = Int(sample.x / size.width * Double(image.width)), y = Int((size.height - sample.y) / size.height * Double(image.height))
                guard x >= 0 && y >= 0 && x < image.width && y < image.height else { throw HostFailure(message: "The HDR sample must belong to its window") }
                var rgba = [UInt8](repeating: 0, count: 4)
                try rgba.withUnsafeMutableBytes { bytes in
                    guard let context = CGContext(data: bytes.baseAddress, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                        space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
                        throw HostFailure(message: "The HDR pixel must be readable")
                    }
                    context.translateBy(x: -CGFloat(x), y: -CGFloat(image.height - y - 1))
                    context.draw(image, in: CGRect(x: 0, y: 0, width: CGFloat(image.width), height: CGFloat(image.height)))
                }
                return rgba.map(Int.init)
            }
        }.value
    }
    @MainActor static func run(_ platform: UInt32, theme: String, depth: String) async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("hdr-bubbles-\(UUID())")
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: directory))
        try await wait("HDR workspace startup", failure: { store.failure }) { !store.state.isNull }
        try await store.workspaces!.started()
        try await store.workspaces!.perform(["type": "switch", "id": "builtin:workspace:painter"])
        try await store.apply(["type": "set_theme", "theme": theme])
        let window = NSWindow(contentRect: CGRect(x: 60, y: 80, width: 1200, height: 870),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "HDR bubbles \(platform) \(theme) \(depth)"
        window.isReleasedWhenClosed = false; window.animationBehavior = .none
        let probe = HDRBubbleProbe()
        let host = NSHostingView(rootView: HDRBubbleRoot(store: store, probe: probe).frame(width: 1200, height: 870))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { window.contentView = nil; window.close() }
        try await wait("Mounted HDR canvas", failure: { store.failure }) { store.canvasReady }
        guard let input = marker(host) else { throw HostFailure(message: "The actual bubble input must mount") }
        let bubbles = store.bubbles, interaction = bubbles.input
        interaction.viewport = CGSize(width: 1200, height: 870)
        func tick() async throws {
            await withCheckedContinuation { done in store.query(["type": "catalog"]) { _ in done.resume() } }
            bubbles.tick(CACurrentMediaTime())
            try await drain(0.02)
        }
        func apply(_ action: [String: Any]) async throws { try await store.apply(action); try await tick() }
        func checkPaintHistory(_ steps: [(String, String)]) async throws {
            for (op, expected) in steps {
                try await apply(["type": "color", "action": ["op": op]])
                try require(store.state["colors"]["foreground"].stableKey == expected,
                    "Each completed color scrub must add exactly one paint Undo step")
            }
        }
        func bubble(_ action: [String: Any]) async throws { try await apply(["type": "bubble", "action": action]) }
        func ready(_ name: String, _ predicate: () -> Bool) async throws {
            try await wait(name, seconds: 60, failure: { store.failure ?? store.projectFiles.error }, step: tick, predicate)
        }
        store.projectFiles = ProjectFiles(store: store, dialogs: .init(open: { _, done in done([]) },
            save: { _, _, done in done(nil) }, create: { _, done in done(newSheet([640, 480], depth: depth)) }))
        try await apply(["type": "invoke", "command": "new_document"])
        try await ready("Actual \(depth) drawing installed") {
            !store.projectFiles.busy && store.state["requests"].array.isEmpty
                && store.snapshot["proof_panel"]["depth"].string == depth && store.snapshot["shaders_ready"].bool
        }
        try await apply(["type": "invoke", "command": "fit_canvas"])
        let swatches: [[Any]] = (0..<24).map { index in
            ["HDR color \(index)", ["space": "Srgb", "rgba": [Double(index % 6) / 5, Double(index / 6) / 3, 0.4, 1]]]
        }
        try await apply(["type": "color", "action": ["op": "library", "action": ["op": "import", "name": "HDR bubbles", "swatches": swatches]]])
        for index in 0..<5 {
            try await apply(["type": "set_color", "rgba": [0.15 + Double(index) * 0.12, 0.3, 0.6, 1]])
            let start = CGPoint(x: 620 + index * 12, y: 460), end = CGPoint(x: start.x + 10, y: start.y + 8)
            try event(.leftMouseDown, at: start, marker: input, number: 10 + index); try await tick()
            try event(.leftMouseDragged, at: end, marker: input, number: 10 + index); try await tick()
            try event(.leftMouseUp, at: end, marker: input, number: 10 + index); try await tick()
        }
        try await apply(["type": "set_color", "rgba": [0.2, 0.8, 0.4, 1]])
        let precedingDefinition = store.state["colors"]["foreground"].stableKey
        try await apply(["type": "color", "action": ["op": "hdr_intensity", "stops": -1]])
        guard let source = store.snapshot["header"]["items"].array.first(where: { $0["resolved_control"]["kind"].string == "color" }) else {
            throw HostFailure(message: "The workspace must expose its real color source")
        }
        for phase in ["down", "up"] {
            try await bubble(["type": "drag", "grab": ["kind": "header", "id": source["id"].raw],
                "phase": phase, "position": [650, 390], "viewport": [1200, 870]])
        }
        try await ready("HDR bubble presentation") {
            bubbles.items.contains { !$0.model["color"]["intensity"].isNull && $0.placement.positioned } && !bubbles.needsFrame
        }
        guard let item = bubbles.items.first(where: { $0.view["kind"].string == "color" }) else {
            throw HostFailure(message: "The source must create the actual color bubble")
        }
        func intensity() -> JSON { item.model["color"]["intensity"] }
        func save(_ name: String) async throws {
            guard let path = ProcessInfo.processInfo.environment["CAPY_HDR_CAPTURE_DIR"] else { return }
            let output = URL(fileURLWithPath: path, isDirectory: true), stem = "\(platform)-\(theme)-\(depth)-\(name)"
            try await stableCapture(window, to: output.appendingPathComponent(stem + ".png"))
            let content = host.bounds, pixels = window.convertToBacking(content)
            let context = JSON(["platform": platform, "theme": theme, "depth": depth, "component": probe.component,
                "window": ["content_bounds": [content.minX, content.minY, content.width, content.height],
                    "pixel_size": [pixels.width, pixels.height], "backing_scale": window.backingScaleFactor],
                "center": [item.placement.center.x, item.placement.center.y],
                "reference_state": ["colors": store.state["colors"].raw, "color_panel": store.colorPanel.raw,
                    "color_viewing": store.colorViewing.raw, "brush": store.state["brush"].raw,
                    "theme": store.state["theme"].raw, "palette": store.state["palette"].raw,
                    "camera": store.state["camera"].raw, "workspace": store.state["workspace"].raw,
                    "tabs": store.state["tabs"].raw, "document_file": store.state["document_file"].raw,
                    "screen": store.state["screen"].raw], "snapshot": store.snapshot.json.raw])
            let bytes = Data(item.model.stableKey.utf8), metadata = Data(context.stableKey.utf8)
            try await Task.detached(priority: .utility) {
                try bytes.write(to: output.appendingPathComponent(stem + ".json"))
                try metadata.write(to: output.appendingPathComponent(stem + "-context.json"))
            }.value
        }
        func point(_ fraction: Double) -> CGPoint {
            let path = intensity()["path"].array
            return path[Int((fraction * Double(path.count - 1)).rounded())].point
        }
        func global(_ local: CGPoint) -> CGPoint { CGPoint(x: item.placement.center.x + local.x, y: item.placement.center.y + local.y) }
        func ev(_ expected: Double, _ name: String) async throws {
            try await ready(name) { abs(intensity()["stops"].number - expected) < 0.035 }
        }
        func set(_ value: Double) async throws { try await bubble(["type": "set_intensity", "id": item.id, "stops": value]); try await ev(value, "Set intensity") }
        func checkArcPixels(_ name: String) async throws {
            let zero = intensity()["zero"].array.map(\.point)
            try require(zero.count == 2 && abs(hypot(zero[1].x - zero[0].x, zero[1].y - zero[0].y) - 3) < 0.01,
                "The owner publishes GTK's exact three-pixel radial zero tick")
            let mid = CGPoint(x: (zero[0].x + zero[1].x) / 2, y: (zero[0].y + zero[1].y) / 2)
            let normal = CGPoint(x: -(zero[1].y - zero[0].y) / 3, y: (zero[1].x - zero[0].x) / 3)
            var samples = [-0.25, 0, 0.25].flatMap { dx in [-0.25, 0, 0.25].map { dy in global(CGPoint(x: mid.x + dx, y: mid.y + dy)) } }
            samples += [-3.5, 3.5].map { distance in global(CGPoint(x: mid.x + normal.x * distance, y: mid.y + normal.y * distance)) }
            let indices = [12, 28, 45, 55]
            samples += indices.map { global(intensity()["path"][$0].point) }
            let rgba = try await pixels(window, marker: input, points: samples, to: directory.appendingPathComponent("arc-\(name).png"))
            let backdrop = (0..<3).map { (rgba[9][$0] + rgba[10][$0]) / 2 }
            let contrast = rgba.prefix(9).map { sample in zip(sample.prefix(3), backdrop).map { abs($0.0 - $0.1) }.max() ?? 0 }.max() ?? 0
            try require(contrast > 24, "The actual native zero tick must be visible outside its color band")
            var maximum = 0
            for (offset, index) in indices.enumerated() {
                let expected = intensity()["ramp"][index].array.prefix(3).map { Int(($0.number * 255).rounded()) }
                maximum = max(maximum, zip(rgba[11 + offset].prefix(3), expected).map { abs($0.0 - $0.1) }.max() ?? 0)
            }
            note("Actual HDR arc \(name): zero tick contrast=\(contrast), mapped ramp maximumRGBerror=\(maximum)")
            try require(maximum <= 8, "The actual native conic ramp follows the owner colors and angle order")
        }
        func native(_ type: NSEvent.EventType, _ fraction: Double, _ number: Int) async throws {
            try event(type, at: global(point(fraction)), marker: input, number: number); try await tick()
        }
        let base = store.colorPanel["base"].stableKey
        try require(intensity()["path"].array.count == 65 && intensity()["ramp"].array.count == 65,
            "The host receives the complete owner path and mapped ramp")
        try require(item.model["color"]["outline"].array.count == 4 && intensity()["enabled"].bool,
            "An HDR document has its shared extended outline and enabled intensity arc")
        let definition = store.state["colors"]["foreground"].stableKey
        try await native(.leftMouseDown, 0.25, 1)
        try await native(.leftMouseDragged, 0.75, 1)
        try await ev(4, "Actual native HDR drag")
        try require(store.colorPanel["base"].stableKey == base, "Intensity preserves the picker base")
        try key("\u{1b}", code: 53, window: window)
        try await ev(-1, "Native Escape rolls intensity back")
        try event(.leftMouseUp, at: global(point(0.75)), marker: input, number: 1)
        try await tick()
        try require(store.state["colors"]["foreground"].stableKey == definition && !interaction.active,
            "Escape restores the exact paint definition and retires the contact")
        try await native(.leftMouseDown, 0.25, 2)
        try await native(.leftMouseDragged, 0.75, 2)
        try await native(.leftMouseUp, 0.75, 2)
        try await ev(4, "Native intensity commit")
        let committedDefinition = store.state["colors"]["foreground"].stableKey
        try await checkPaintHistory([("undo", definition), ("undo", precedingDefinition),
            ("redo", definition), ("redo", committedDefinition)])
        let wheel = global(CGPoint(x: 20, y: -10))
        try event(.leftMouseDown, at: wheel, marker: input, number: 3); try await tick()
        for x in [10.0, 0.0] {
            try event(.leftMouseDragged, at: global(CGPoint(x: x, y: -10)), marker: input, number: 3); try await tick()
        }
        try event(.leftMouseUp, at: global(CGPoint(x: 0, y: -10)), marker: input, number: 3); try await tick()
        try await ev(4, "Wheel edits retain committed intensity")
        let wheelDefinition = store.state["colors"]["foreground"].stableKey
        try require(wheelDefinition != committedDefinition, "The native wheel scrub must change its paint definition")
        try await checkPaintHistory([("undo", committedDefinition), ("undo", definition),
            ("redo", committedDefinition), ("redo", wheelDefinition)])
        try await set(-1)
        let beforePair = store.state["colors"]["foreground"].stableKey
        let start = global(point(0.25)), end = global(point(0.75))
        try require(interaction.down(start, id: 100, device: .touch, clicks: 1, doubleTime: 0.4, time: 100) == .capture,
            "The first intensity touch captures")
        _ = interaction.move(end, id: 100); try await ev(4, "Touch intensity edit before pair")
        let second = CGPoint(x: item.placement.center.x, y: item.placement.center.y - 70)
        try require(interaction.down(second, id: 101, device: .touch, clicks: 1, doubleTime: 0.4, time: 101) == .capture,
            "The second touch begins the bubble pair")
        try await ev(-1, "Pair rolls intensity back before pinch")
        try require(store.state["colors"]["foreground"].stableKey == beforePair, "Pair restores the exact paint definition")
        _ = interaction.up(end, id: 100, cancelled: true)
        _ = interaction.up(second, id: 101, cancelled: false)
        try await tick(); try await ev(-1, "Pair release cannot commit the retired edit")
        _ = interaction.down(global(point(0.5)), id: 102, device: .mouse, clicks: 2, doubleTime: 0.4, time: 102)
        _ = interaction.up(global(point(0.5)), id: 102, cancelled: false)
        try await ev(0, "Intensity double click resets EV")
        try key("\u{f703}", code: 124, window: window)
        try await ev(0.1, "Native Right Arrow nudges intensity")
        try key("\u{f729}", code: 115, window: window)
        try await ev(0, "Native Home resets intensity")
        let held = global(point(0.25))
        _ = interaction.down(held, id: 103, device: .touch, clicks: 1, doubleTime: 0.4, time: 104)
        interaction.recognizeHold()
        try await ready("Intensity hold opens the bubble menu") { !bubbles.menu.isNull }
        _ = interaction.move(global(point(0.75)), id: 103)
        try await ev(4, "Moving a held intensity contact resumes its edit")
        try require(bubbles.menu.isNull, "Resuming an intensity edit closes its hold menu")
        _ = interaction.up(global(point(0.75)), id: 103, cancelled: true)
        try await ev(0, "Cancelling a resumed hold restores its original EV")
        interaction.cancel()
        let adjacentArc = global(point(0.5)), adjacentWheel = global(CGPoint(x: 20, y: -10))
        for (id, time, point) in [(UInt64(120), 120.0, adjacentArc), (121, 120.1, adjacentWheel), (122, 120.2, adjacentArc)] {
            _ = interaction.down(point, id: id, device: .touch, clicks: 1, doubleTime: 0.4, time: time)
            _ = interaction.up(point, id: id, cancelled: false)
            try await tick()
            try require(abs(intensity()["stops"].number - 2) < 0.035, "An intervening wheel contact separates intensity taps")
        }
        try await set(12)
        try require(intensity()["range"][1].number == 12
            && intensity()["point"].stableKey == intensity()["path"].array.last?.stableKey,
            "Sampled highlights expand the owner range and keep its endpoint")
        let rampEnd = intensity()["ramp"].array.last!.array.map(\.number), marker = intensity()["marker"].array.map(\.number)
        try require(zip(rampEnd.prefix(3), marker.prefix(3)).allSatisfy { abs($0.0 - $0.1) < 0.001 },
            "The expanded mapped ramp meets the selected highlight")
        _ = interaction.down(global(point(0.5)), id: 104, device: .touch, clicks: 1, doubleTime: 0.4, time: 122)
        try await tick()
        try await apply(["type": "color", "action": ["op": "select", "slot": "background"]])
        let background = store.state["colors"]["background"].stableKey
        _ = interaction.move(global(point(0.75)), id: 104)
        _ = interaction.up(global(point(0.75)), id: 104, cancelled: true)
        try await tick()
        try require(store.state["colors"]["background"].stableKey == background,
            "A retired intensity contact cannot mutate a newly selected paint slot")
        try await apply(["type": "color", "action": ["op": "select", "slot": "transparent"]])
        try await ready("Transparent paint disables HDR intensity") { !intensity()["enabled"].bool }
        let transparent = store.state["colors"].stableKey
        try await native(.leftMouseDown, 0.5, 4)
        try await native(.leftMouseUp, 0.5, 4)
        try require(store.state["colors"].stableKey == transparent, "A disabled intensity arc cannot edit paint")
        try await save("transparent")
        try await apply(["type": "color", "action": ["op": "select", "slot": "foreground"]])
        try await apply(["type": "invoke", "command": "quick_mask"])
        try await ready("Quick Mask removes HDR intensity") { item.model["color"].isNull && item.view["color"]["intensity"].isNull }
        try await save("quick-mask")
        try await apply(["type": "invoke", "command": "quick_mask"])
        try await ready("Leaving Quick Mask restores HDR intensity") { !item.model["color"]["intensity"].isNull }
        try await apply(["type": "set_color", "rgba": [0.2, 0.8, 0.4, 1]])
        try await set(0)
        for size in [136, 216, 344] {
            try await bubble(["type": "option", "id": item.id, "option": ["kind": "size", "size": size]])
            var exterior: [Int]?
            for shape in ["circle", "triangle", "square"] {
                try await apply(["type": "color", "action": ["op": "shape", "shape": shape]])
                try await ready("HDR size and shape settle") { !bubbles.needsFrame && item.view["diameter"].number == Double(size) }
                for fraction in [0.0, 0.5, 1.0] {
                    try require(item.hit(global(point(fraction))).bubbleHit == "intensity", "Every rendered HDR arc sample has matching shared input geometry")
                }
                if shape != "square" {
                    let offset = Double(size - 14) * 0.46
                    let sample = global(CGPoint(x: offset, y: -offset))
                    let rgba = try await pixel(window, marker: input, point: sample, to: directory.appendingPathComponent("clip-\(size)-\(shape).png"))
                    if let exterior {
                        let difference = zip(exterior.prefix(3), rgba.prefix(3)).map { abs($0.0 - $0.1) }.max() ?? 0
                        note("Triangle clip size=\(size) outside-field circle=\(exterior) triangle=\(rgba) maxdiff=\(difference)")
                        try require(difference <= 3, "An actual HDR triangle field must not paint outside its shared contour")
                    } else { exterior = rgba }
                }
                try await save("\(size)-\(shape)")
            }
        }
        for stops in [-4.0, 6, 12] {
            try await set(stops)
            try await checkArcPixels("ev-\(Int(stops))")
            try await save("ev-\(Int(stops))")
        }
        var options = item.view["options"].object
        for key in ["recent", "palette", "pair"] { options[key] = true }
        try await bubble(["type": "set_options", "id": item.id, "options": options])
        try await ready("Populated HDR beads settle") { !bubbles.needsFrame }
        try require(item.view["beads"].array.filter { $0["placement"]["bead"]["kind"].string == "swatch" }.count >= 12,
            "The imported palette surrounds the HDR outline")
        try require(item.view["beads"].array.filter { $0["placement"]["bead"]["kind"].string == "recent" }.count >= 3,
            "Actual strokes populate recent paint beads around the HDR outline")
        try await save("all-beads")
        probe.docked = true
        try await save("docked-color-panel")
        probe.docked = false
        probe.component = true
        try await drain(0.1)
        let originalHeadroom = store.displayHeadroom
        defer { store.displayHeadroom = originalHeadroom }
        let arcSamples = [12, 28, 45, 55].map { global(intensity()["path"][$0].point) }
        store.displayHeadroom = 1; try await tick()
        let sdr = try await pixels(window, marker: host, points: arcSamples, to: directory.appendingPathComponent("headroom-1.png"))
        store.displayHeadroom = 4; try await tick()
        try require(store.colorViewing["headroom"].number == 4, "The real store supplies its current display headroom")
        let viewing = store.colorViewing, color = store.colorPanel
        let hdr = ColorUIResourceCache().resolve(["type": "intensity_arc", "size": item.view["color"]["stage"].number,
            "stops": intensity()["stops"].number, "base": color["base"].raw, "document_space": viewing["document_space"].raw,
            "depth": color["document_depth"].raw, "recipe": viewing["recipe"].raw, "headroom": viewing["headroom"].number])
        let maximum = hdr["colors"].array.flatMap { $0.array.prefix(3).map(\.number) }.max() ?? 0
        let markerMaximum = HDRIntensityTrack.linearColor(hdr["marker_color"]).cgColor?.components?.prefix(3).max() ?? 0
        try require(maximum > 1 && markerMaximum > 1, "The actual native ramp color and marker retain linear HDR values")
        let edr = try await pixels(window, marker: host, points: arcSamples, to: directory.appendingPathComponent("headroom-4.png"))
        var difference = 0
        for (a, b) in zip(sdr, edr) { for channel in 0..<3 { difference = max(difference, abs(a[channel] - b[channel])) } }
        note("Logical EDR route headroom4: ramp maximum=\(maximum), native marker maximum=\(markerMaximum), actual bandRGBdifference=\(difference)")
        try require(difference > 8, "The actual bubble band responds to the store's display headroom")
        try await save("component-headroom-4")
        store.displayHeadroom = originalHeadroom; try await tick()
        try await store.workspaces!.closed()
        let saved = await withCheckedContinuation { done in store.recovery.close { done.resume(returning: $0) } }
        try require(saved, store.recovery.error ?? "The private HDR drawing must close before storage cleanup")
        note("PASS HDR actual \(depth) document, native drag/commit/Escape, wheel EV, touch pair, reset/keys/hold, expanded ramp, Quick Mask: platform=\(platform) theme=\(theme)")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do {
                let environment = ProcessInfo.processInfo.environment
                for platform: UInt32 in [0, 1] { for theme in ["light", "dark"] { for depth in ["F16", "F32"] {
                    if let filter = environment["CAPY_HDR_PLATFORM"], filter != String(platform) { continue }
                    if let filter = environment["CAPY_HDR_THEME"], filter != theme { continue }
                    if let filter = environment["CAPY_HDR_DEPTH"], filter != depth { continue }
                    try await run(platform, theme: theme, depth: depth)
                } } }
                exit(0)
            } catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
