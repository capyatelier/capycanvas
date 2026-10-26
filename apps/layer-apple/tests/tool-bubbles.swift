import AppKit
import SwiftUI
import Combine
import QuartzCore
import ImageIO

@MainActor private final class BubbleGlassProbe: ObservableObject {
    @Published var strength = 1.0
    @Published var scale = 1.0
}
private struct BubbleGlassProbeView: View {
    @ObservedObject var probe: BubbleGlassProbe
    var body: some View {
        ZStack {
            Color.clear.frame(width: 44, height: 44)
                .modifier(GlassRegistration(shape: SquircleShape(22), circular: true, strength: probe.strength))
                .modifier(GlassArcRegistration(arc: JSON(["radius": 9, "band": 3, "start": 0, "sweep": 90]), strength: probe.strength))
                .scaleEffect(probe.scale).position(x: 1150, y: 820)
            Color.clear.frame(width: 40, height: 50)
                .modifier(GlassRegistration(shape: SquircleShape(6), strength: probe.strength))
                .scaleEffect(probe.scale).position(x: 1080, y: 790)
        }.allowsHitTesting(false)
    }
}

@main final class ToolBubbleChecks: NativeWorkspaceInputFixture {
    @MainActor static func checkWhiteDab(_ item: BubblePresentation, window: NSWindow, marker: NSView, destination: URL) async throws {
        guard let image = item.stamp else { throw HostFailure(message: "The white dab needs its real alpha mask") }
        let bitmap = SliderStampImage(image: image), source = item.view["stamp"], side = source["diameter"].number * item.stampExtent
        let center = source["center"].point, thumbs = item.model["rings"].array.map { $0["thumb"].point }
        let edge = item.view["rings"].array.map { $0["arc"]["radius"].number + $0["arc"]["band"].number / 2 }.max() ?? 0
        let sample = try await Task.detached(priority: .utility) { () throws -> (CGPoint, Int) in
            guard let mask = bitmap.image, let data = mask.dataProvider?.data, let bytes = CFDataGetBytePtr(data), mask.bitsPerPixel == 32 else {
                throw HostFailure(message: "The actual dab alpha mask must be readable")
            }
            var best: (CGPoint, Int)?
            for y in 0..<mask.height { for x in 0..<mask.width {
                let point = CGPoint(x: center.x + (Double(x) + 0.5) / Double(mask.width) * side - side / 2,
                    y: center.y + (Double(y) + 0.5) / Double(mask.height) * side - side / 2)
                guard hypot(point.x, point.y) > edge + 1,
                    thumbs.allSatisfy({ hypot(point.x - $0.x, point.y - $0.y) > 14 }) else { continue }
                let alpha = Int(bytes[y * mask.bytesPerRow + x * 4 + 3])
                if alpha > (best?.1 ?? -1) { best = (point, alpha) }
            } }
            guard let best, best.1 >= 180 else { throw HostFailure(message: "The white dab needs high alpha outside its halo and thumbs") }
            return best
        }.value
        let point = CGPoint(x: item.placement.center.x + sample.0.x, y: item.placement.center.y + sample.0.y)
        let windowPoint = marker.convert(point, to: nil), bounds = window.frame.size
        try require(marker.bounds.contains(point), "The white dab sample belongs to the owned editor")
        try await capture(window, to: destination)
        let pixel = try await Task.detached(priority: .utility) { () throws -> [Int] in
            guard let source = CGImageSourceCreateWithURL(destination as CFURL, nil), let captured = CGImageSourceCreateImageAtIndex(source, 0, nil),
                let space = CGColorSpace(name: CGColorSpace.sRGB) else { throw HostFailure(message: "The owned-window capture must decode") }
            let x = Int(windowPoint.x / bounds.width * Double(captured.width)), y = Int((bounds.height - windowPoint.y) / bounds.height * Double(captured.height))
            guard x >= 0 && y >= 0 && x < captured.width && y < captured.height else { throw HostFailure(message: "The dab sample must lie inside its capture") }
            var rgba = [UInt8](repeating: 0, count: 4)
            try rgba.withUnsafeMutableBytes { bytes in
                guard let context = CGContext(data: bytes.baseAddress, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                    space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { throw HostFailure(message: "The captured pixel must be readable") }
                context.translateBy(x: -CGFloat(x), y: -CGFloat(captured.height - y - 1))
                context.draw(captured, in: CGRect(x: 0, y: 0, width: CGFloat(captured.width), height: CGFloat(captured.height)))
            }
            return rgba.map(Int.init)
        }.value
        note("White SDR dab alpha=\(sample.1) rgba=\(pixel) source=\(source.stableKey) side=\(side) sample=\(point)")
        try require(pixel.prefix(3).allSatisfy { $0 >= 245 }, "The actual white SDR dab must remain near white")
    }
    @MainActor static func marker(_ view: NSView) -> ReorderInputView? {
        if let input = view as? ReorderInputView, input.model?.bubbleInput != nil { return input }
        return view.subviews.lazy.compactMap(marker).first
    }
    @MainActor static func run(_ platform: UInt32, theme: String) async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bubble-settings-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
        try await wait("Initial bubble models", failure: { store.failure }) { !store.state.isNull }
        try await store.workspaces!.started()
        try await store.workspaces!.perform(["type": "switch", "id": "builtin:workspace:painter"])
        store.native?.resize(width: 1200, height: 870, scale: 1)
        try await store.apply(["type": "set_theme", "theme": theme])
        let window = NSWindow(contentRect: CGRect(x: 60, y: 80, width: 1200, height: 870),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Tool bubbles \(platform) \(theme)"; window.isReleasedWhenClosed = false
        window.animationBehavior = .none
        let glassProbe = BubbleGlassProbe()
        let host = NSHostingView(rootView: EditorView(store: store) {
            MacMetalCanvas(store: store).overlay { BubbleGlassProbeView(probe: glassProbe) }
        }.frame(width: 1200, height: 870))
        window.contentView = host; window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        defer { window.contentView = nil; window.close() }
        try await drain(0.3)
        try await wait("Mounted Metal canvas", failure: { store.failure }) { store.canvasReady }
        guard let input = marker(host) else { throw HostFailure(message: "Native bubble marker did not mount") }
        let bubbles = store.bubbles
        func capture(_ name: String) async throws {
            guard let path = ProcessInfo.processInfo.environment["CAPY_BUBBLE_CAPTURE_DIR"] else { return }
            let directory = URL(fileURLWithPath: path, isDirectory: true)
            try await Self.capture(window, to: directory.appendingPathComponent("\(platform)-\(theme)-\(name).png"))
        }
        func glass() -> JSON {
            let text = Mirror(reflecting: store.glass).descendant("sent") as? String ?? "null"
            return (try? JSON.decode(text)) ?? JSON()
        }
        func probeRegions() -> (JSON, JSON) {
            let current = glass()
            return (current["regions"].array.first { $0[2].number == 44 && $0[3].number == 44 && $0[8].number == 0 } ?? JSON(),
                current["arcs"].array.first { $0[2].number == 9 } ?? JSON())
        }
        try await wait("Initial glass probe") { !probeRegions().0.isNull && !probeRegions().1.isNull }
        let glassBefore = probeRegions()
        glassProbe.strength = 0.25
        try await wait("Fade-only glass transport") { probeRegions().0[9].number == 0.25 && probeRegions().1[6].number == 0.25 }
        let glassAfter = probeRegions()
        try require(Array(glassBefore.0.array.dropLast()).map(\.number) == Array(glassAfter.0.array.dropLast()).map(\.number)
            && Array(glassBefore.1.array.dropLast()).map(\.number) == Array(glassAfter.1.array.dropLast()).map(\.number),
            "Strength-only glass publication must preserve all geometry")
        glassProbe.strength = 1
        glassProbe.scale = 1.5
        try await wait("Transformed glass lengths") {
            let current = glass()
            return current["regions"].array.contains { $0[2].number == 60 && $0[3].number == 75 && $0[4].number == 9 && $0[7].number == 9 }
                && current["arcs"].array.contains { $0[2].number == 13.5 && $0[3].number == 4.5
                    && $0[4].number == glassBefore.1[4].number && $0[5].number == glassBefore.1[5].number }
        }
        glassProbe.scale = 1
        func tick() async throws {
            await withCheckedContinuation { done in store.query(["type": "catalog"]) { _ in done.resume() } }
            bubbles.tick(CACurrentMediaTime())
            try await drain(0.02)
        }
        func action(_ value: [String: Any]) async throws {
            try await store.apply(["type": "bubble", "action": value]); try await tick()
            try require(store.failure == nil, store.failure ?? "")
        }
        func settled(_ label: String) async throws {
            try await wait(label, failure: { store.failure }, step: tick) { !bubbles.needsFrame && bubbles.items.allSatisfy { $0.placement.positioned } }
        }
        func checkOwnerGlass(_ item: BubblePresentation) async throws {
            guard let header = store.header.geometry["items"].array.map({ $0["bounds"].rect }).first(where: { !$0.isEmpty }) else {
                throw HostFailure(message: "The glass homing check needs actual header geometry")
            }
            let origin = item.placement.center, target = CGPoint(x: header.midX, y: header.midY)
            let authoredLayout = store.state["workspace"]["layout"].stableKey
            let initialMoving = bubbles.needsFrame
            let model = item.model.stableKey, progress = [item.cardProgress, item.faceProgress]
            let restingHeight = item.card.isNull ? 22 : item.card["size"][1].number
            let expectedHeight = restingHeight * 0.82
            let grab: [String: Any] = ["kind": "bubble", "id": item.id]
            try await action(["type": "drag", "grab": grab, "phase": "down", "position": [origin.x, origin.y], "viewport": [1200, 870]])
            try await action(["type": "drag", "grab": grab, "phase": "move", "position": [target.x, target.y], "viewport": [1200, 870]])
            try await wait("Actual Layers homing glass", failure: { store.failure }, step: tick) {
                item.placement.homing && glass()["regions"].array.contains {
                    abs($0[3].number - expectedHeight) < 0.5 && abs($0[9].number - 0.55) < 0.001
                }
            }
            try require(item.model.stableKey == model && [item.cardProgress, item.faceProgress] == progress,
                "Actual Layers glass follows placement without model or ramp changes")
            note("PASS actual Layers \(item.card.isNull ? "count" : "card") glass strength .55 at scaled height \(expectedHeight), unchanged model/ramp")
            try await action(["type": "drag", "grab": grab, "phase": "cancel", "position": [target.x, target.y], "viewport": [1200, 870]])
            do {
                try await wait("Actual Layers glass cancellation restores placement", failure: { store.failure }, step: tick) {
                    !item.placement.homing && !item.placement.lifted && hypot(item.placement.center.x - origin.x, item.placement.center.y - origin.y) < 0.1
                        && glass()["regions"].array.contains { abs($0[3].number - restingHeight) < 0.5 && abs($0[9].number - 1) < 0.001 }
                }
            } catch {
                note("Layers cancellation diagnostic platform=\(platform) theme=\(theme) id=\(item.id) origin=\(origin) final=\(item.placement.center) lifted=\(item.placement.lifted) homing=\(item.placement.homing) initiallyMoving=\(initialMoving) moving=\(bubbles.needsFrame) authoredLayoutEqual=\(store.state["workspace"]["layout"].stableKey == authoredLayout) restingHeight=\(restingHeight) glass=\(glass().stableKey)")
                throw error
            }
            try await settled("Actual Layers glass homing cancellation")
        }
        func take(_ control: (JSON) -> Bool, at point: CGPoint) async throws -> BubblePresentation {
            guard let source = store.snapshot["header"]["items"].array.first(where: control) else { throw HostFailure(message: "No eligible header source") }
            let id = source["id"].uint
            let grab = JSON(["kind": "header", "id": id])
            for phase in ["down", "up"] { try await action(["type": "drag", "grab": grab.raw, "phase": phase, "position": [point.x, point.y], "viewport": [1200, 870]]) }
            try await settled("New bubble motion")
            guard let item = bubbles.items.first(where: { $0.view["source"]["control"].stableKey == source["resolved_control"].stableKey }) ?? bubbles.items.last else { throw HostFailure(message: "Taken source has no retained presentation") }
            return item
        }
        if CommandLine.arguments.contains("--blocked-contacts") {
            let model = bubbles.input, blocker = UUID()
            model.viewport = CGSize(width: 1200, height: 870)
            func interrupt() async throws {
                store.workspace.popover(blocker, open: true)
                model.cancel()
                try await tick()
                store.workspace.popover(blocker, open: false)
                try await tick()
                try require(!model.active, "A blocked native contact retires after shared cancellation")
            }
            defer { store.workspace.popover(blocker, open: false) }
            try await store.apply(["type": "set_color", "rgba": [0.2, 0.45, 0.8, 1]])
            let color = try await take({ $0["resolved_control"]["kind"].string == "color" }, at: CGPoint(x: 760, y: 400))
            let center = color.placement.center
            let start = CGPoint(x: center.x, y: center.y - 85), end = CGPoint(x: center.x + 85, y: center.y)
            let before = store.state["colors"].stableKey
            for id: UInt64 in [900, 901] {
                try require(model.down(start, id: id, device: .touch, clicks: 1, doubleTime: 0.4, time: Double(id)) == .capture,
                    "An admitted wheel contact captures before a native popup opens")
                _ = model.move(end, id: id)
                try await wait("The wheel contact changes paint", failure: { store.failure }, step: tick) {
                    store.state["colors"].stableKey != before
                }
                try await interrupt()
                try require(store.state["colors"].stableKey == before && store.failure == nil,
                    "A native popup must cancel and restore the wheel before retiring its contact")
            }
            note("PASS blocked wheel and repeated contact: platform=\(platform) theme=\(theme)")
            let brush = try await take({ $0["resolved_control"]["command"].string == "drawing_brush" }, at: CGPoint(x: 420, y: 340))
            guard let ringIndex = brush.view["rings"].array.lastIndex(where: { $0["setting"].string != "size" }) else {
                throw HostFailure(message: "The blocked-contact check needs a real brush ring")
            }
            let thumb = brush.model["rings"][ringIndex]["thumb"].point
            let point = CGPoint(x: brush.placement.center.x + thumb.x, y: brush.placement.center.y + thumb.y)
            let arc = brush.view["rings"][ringIndex]["arc"]
            let angle = (arc["start"].number + arc["sweep"].number * 0.8) * .pi / 180
            let destination = CGPoint(x: brush.placement.center.x + cos(angle) * arc["radius"].number,
                y: brush.placement.center.y + sin(angle) * arc["radius"].number)
            let settings = store.state["brush"].stableKey
            let toolSettings = store.state["tool_settings"].stableKey
            _ = model.down(point, id: 902, device: .mouse, clicks: 1, doubleTime: 0.4, time: 902)
            _ = model.move(destination, id: 902)
            try await wait("The ring contact changes its setting", failure: { store.failure }, step: tick) {
                store.state["tool_settings"].stableKey != toolSettings
            }
            try await interrupt()
            try require(store.state["brush"].stableKey == settings && store.state["tool_settings"].stableKey == toolSettings,
                "A native popup restores the complete ring setting")
            note("PASS blocked ring exact rollback: platform=\(platform) theme=\(theme)")
            let layout = store.state["workspace"]["layout"].stableKey, origin = brush.placement.center
            _ = model.down(origin, id: 903, device: .mouse, clicks: 1, doubleTime: 0.4, time: 903)
            _ = model.move(CGPoint(x: origin.x + 80, y: origin.y + 40), id: 903)
            try await wait("The bubble contact lifts", failure: { store.failure }, step: tick) { brush.placement.lifted }
            try await interrupt(); try await settled("Blocked drag cancellation")
            try require(store.state["workspace"]["layout"].stableKey == layout && !brush.placement.lifted,
                "A native popup restores the exact layout of an interrupted bubble drag")
            let closed = await withCheckedContinuation { done in store.prepareClose { done.resume(returning: $0) } }
            try require(closed, "Close private blocked-contact storage")
            note("PASS blocked wheel, repeated contact, ring and drag cancellation: platform=\(platform) theme=\(theme)")
            return
        }
        guard let brushEntry = store.snapshot["header"]["model"]["zones"].array.flatMap(\.array).first(where: {
            $0["item"]["control"]["command"].string == "drawing_brush"
        }) else { throw HostFailure(message: "Missing shared drawing brush source: \(store.snapshot["header"]["model"].stableKey)") }
        let brushSource = brushEntry["id"].uint
        func drawerVariants() -> [BubbleChoice] {
            bubbles.input.choiceSources.values.filter {
                $0.grab["kind"].string == "variant" && $0.bounds.width > 20 && $0.bounds.height > 20
            }
        }
        for cancelled in [false, true] {
            let before = store.state["workspace"]["layout"].stableKey
            let count = bubbles.items.count
            try await store.apply(["type": "customize", "action": ["type": "toggle_header_drawer", "id": brushSource]])
            try await wait("Mounted variant drawer", failure: { store.failure }, step: tick) {
                guard let drawer = store.contentDrawers.items["tool"], drawer.interactive else { return false }
                let bounds = drawer.geometry["placement"]["bounds"].rect
                return drawerVariants().contains { bounds.contains($0.bounds) }
            }
            try await drain(store.catalog["panel_expansion_ms"].number / 1000 + 0.1)
            guard let drawer = store.contentDrawers.items["tool"],
                  let choice = drawerVariants().filter({ drawer.geometry["placement"]["bounds"].rect.contains($0.bounds) })
                    .sorted(by: { a, b in a.bounds.minY == b.bounds.minY ? a.bounds.minX < b.bounds.minX : a.bounds.minY < b.bounds.minY }).first else {
                throw HostFailure(message: "The real drawer must publish a mounted variant source")
            }
            let identity = choice.grab.stableKey, bounds = drawer.geometry["placement"]["bounds"].rect
            let start = CGPoint(x: choice.bounds.midX, y: choice.bounds.midY)
            let groups = store.snapshot["layout"]["groups"].array.map { $0["bounds"].rect }
            guard let outside = [CGPoint(x: 1000, y: 650), CGPoint(x: 800, y: 650), CGPoint(x: 400, y: 650)].first(where: { point in
                input.bounds.contains(point) && !bounds.insetBy(dx: -100, dy: -100).contains(point)
                    && !groups.contains(where: { $0.contains(point) })
            }) else { throw HostFailure(message: "The variant pickup needs canvas beyond its drawer") }
            let number = cancelled ? 501 : 500
            try event(.leftMouseDown, at: start, marker: input, number: number)
            try await drain(0.03)
            try require(bubbles.input.active && !bubbles.input.captures, "The native drawer row observes its initial contact")
            try event(.leftMouseDragged, at: outside, marker: input, number: number)
            try await wait("Variant pickup closes and withdraws its drawer", failure: { store.failure }, step: tick) {
                store.state["customization"]["drawer"].isNull && store.contentDrawers.items["tool"] == nil
                    && !bubbles.input.choiceSources.values.contains { $0.grab.stableKey == identity }
            }
            try require(bubbles.input.active && bubbles.input.captures,
                "A pulled variant retains native capture after its drawer source disappears")
            guard let lifted = bubbles.items.first(where: { $0.placement.lifted }) else {
                throw HostFailure(message: "The retained native contact must own a shared lifted bubble")
            }
            let next = CGPoint(x: outside.x - 30, y: outside.y + 25)
            try event(.leftMouseDragged, at: next, marker: input, number: number)
            try await wait("Variant follows after drawer removal", failure: { store.failure }, step: tick) {
                hypot(lifted.placement.center.x - next.x, lifted.placement.center.y - next.y) < 1
            }
            if cancelled {
                try key("\u{1b}", code: 53, window: window)
                try await wait("Native variant cancellation", failure: { store.failure }, step: tick) { !bubbles.input.active }
            }
            try event(.leftMouseUp, at: next, marker: input, number: number)
            try await tick(); try await settled("Variant pickup terminal event")
            try require(!bubbles.input.active && bubbles.items.allSatisfy { !$0.placement.lifted },
                "Native release or cancellation retires the lifted owner")
            if cancelled {
                try require(store.state["workspace"]["layout"].stableKey == before && bubbles.items.count == count,
                    "Cancelling a drawer variant restores the exact preceding layout")
            } else {
                let committed = store.state["workspace"]["layout"].stableKey
                try require(committed != before && bubbles.items.count == count + 1,
                    "Native release commits the drawer variant after source withdrawal")
                try await store.apply(["type": "invoke", "command": "undo_workspace"])
                try await wait("Drawer pickup layout Undo", failure: { store.failure }, step: tick) {
                    store.state["workspace"]["layout"].stableKey == before
                }
                try require(store.state["workspace"]["layout"].stableKey == before, "Drawer pickup is one layout Undo step")
                try await store.apply(["type": "invoke", "command": "redo_workspace"])
                try await wait("Drawer pickup layout Redo", failure: { store.failure }, step: tick) {
                    store.state["workspace"]["layout"].stableKey == committed
                }
                try require(store.state["workspace"]["layout"].stableKey == committed, "Layout Redo restores the completed drawer pickup")
                try await settled("Restored drawer variant")
            }
        }
        try await action(["type": "put_back_all"]); try await settled("Drawer pickup cleanup")
        try require(bubbles.items.isEmpty, "Drawer pickup checks leave the original empty desk")
        note("PASS native drawer variant release, next pickup and cancellation: platform=\(platform) theme=\(theme)")
        try await wait("Native header geometry") { store.header.geometry["items"].array.contains { $0["id"].uint == brushSource } }
        let source = store.header.geometry["items"].array.first { $0["id"].uint == brushSource }!["bounds"].rect
        let pickup = CGPoint(x: source.midX, y: source.midY), destination = CGPoint(x: 420, y: 340)
        try event(.leftMouseDown, at: pickup, marker: input, number: 1)
        try await drain(0.03)
        for fraction in [0.25, 0.5, 0.75, 1.0] {
            try event(.leftMouseDragged, at: CGPoint(x: pickup.x + (destination.x - pickup.x) * fraction,
                y: pickup.y + (destination.y - pickup.y) * fraction), marker: input, number: 1)
            try await drain(0.03)
        }
        try event(.leftMouseUp, at: destination, marker: input, number: 1)
        try await wait("Native header tear-out", failure: { store.failure }) { bubbles.items.count == 1 }
        try await settled("Native tear-out settles")
        let brush = bubbles.items[0]
        try require(brush.view["selected"].bool && !brush.view["rings"].array.isEmpty, "A torn brush opens its halo")
        bubbles.input.hover(brush.placement.center)
        try require(!brush.hover.isNull, "The native tracking exit starts with a hovered bubble")
        guard let exit = NSEvent.enterExitEvent(with: .mouseExited, location: .zero, modifierFlags: [],
            timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
            context: nil, eventNumber: 1, trackingNumber: 1, userData: nil) else {
            throw HostFailure(message: "No native tracking exit")
        }
        NSApp.postEvent(exit, atStart: false)
        try await wait("Native tracking exit clears hover") { brush.hover.isNull }
        try await capture("brush-halo")
        let original = ObjectIdentifier(brush), modelKey = brush.model.stableKey
        var modelChanges = 0
        let observation = brush.objectWillChange.sink { modelChanges += 1 }
        let origin = brush.placement.center
        try await action(["type": "drag", "grab": ["kind": "bubble", "id": brush.id], "phase": "down", "position": [origin.x, origin.y], "viewport": [1200, 870]])
        modelChanges = 0
        for index in 0..<30 {
            try await action(["type": "drag", "grab": ["kind": "bubble", "id": brush.id], "phase": "move", "position": [origin.x + CGFloat(index), origin.y + 10], "viewport": [1200, 870]])
        }
        try require(modelChanges == 0 && brush.model.stableKey == modelKey && ObjectIdentifier(bubbles.items.first { $0.id == brush.id }!) == original,
            "Steady movement must retain the bubble model and publish placement alone")
        try await action(["type": "drag", "grab": ["kind": "bubble", "id": brush.id], "phase": "cancel", "position": [origin.x, origin.y], "viewport": [1200, 870]])
        observation.cancel()
        try await settled("Cancelled bubble motion")
        let ring = brush.model["rings"][0]["thumb"].point
        let p = CGPoint(x: brush.placement.center.x + ring.x, y: brush.placement.center.y + ring.y)
        let settings = store.state["brush"].stableKey
        let model = bubbles.input
        model.viewport = CGSize(width: 1200, height: 870)
        try require(model.down(p, id: 100, device: .touch, clicks: 1, doubleTime: 0.4, time: 10) == .capture, "Native ring contact must capture")
        _ = model.move(CGPoint(x: p.x + 14, y: p.y + 14), id: 100)
        try await tick()
        _ = model.down(CGPoint(x: p.x + 40, y: p.y + 40), id: 101, device: .touch, clicks: 1, doubleTime: 0.4, time: 10.1)
        try await tick()
        try require(store.state["brush"].stableKey == settings, "Second finger must roll back the ring before moving")
        _ = model.up(p, id: 100, cancelled: true)
        let open = brush.view["open"].bool
        _ = model.up(p, id: 101, cancelled: false)
        try await tick()
        try require(brush.view["open"].bool == open, "A cancelled pair's remaining release must not tap")
        try await settled("Pair rollback")
        let resetPoint = CGPoint(x: brush.placement.center.x + ring.x, y: brush.placement.center.y + ring.y)
        _ = model.down(resetPoint, id: 102, device: .mouse, clicks: 2, doubleTime: 0.4, time: 11)
        _ = model.up(resetPoint, id: 102, cancelled: false)
        try await tick()
        try require(store.failure == nil, "Native double click must dispatch the shared ring reset")
        let gaps = (0..<16).map { index in
            let angle = Double(index) * .pi / 8, radius = brush.model["reach"].number * 0.75
            return CGPoint(x: brush.placement.center.x + cos(angle) * radius, y: brush.placement.center.y + sin(angle) * radius)
        }
        guard let gap = gaps.first(where: { bubbles.hit($0) == nil }) else { throw HostFailure(message: "The halo must have undrawn canvas gaps") }
        try require(model.down(gap, id: 103, device: .mouse, clicks: 1, doubleTime: 0.4, time: 12) == .ignore,
            "An undrawn gap must retain native canvas input")
        let beforePen = brush.placement.center
        try event(.leftMouseDown, at: pickup, marker: input, number: 2, tablet: true)
        try await drain(0.03)
        try event(.leftMouseDragged, at: CGPoint(x: 540, y: 340), marker: input, number: 2, tablet: true)
        try await drain(0.03)
        try event(.leftMouseUp, at: CGPoint(x: 540, y: 340), marker: input, number: 2, tablet: true)
        try await drain(0.1); try await settled("Native pen tear-out settles")
        try require(bubbles.items.count == 1 && bubbles.items[0] === brush
            && hypot(brush.placement.center.x - beforePen.x, brush.placement.center.y - beforePen.y) > 20,
            "A supplied native tablet event moves the existing source owner")
        try require(model.hidesPointer(at: brush.placement.center, pen: true), "Native pen capture hides the pointer over the face")
        try await store.apply(["type": "set_color", "rgba": [1, 1, 1, 1]])
        try await store.apply(["type": "set_brush_size", "value": 150])
        try await store.apply(["type": "invoke", "command": "fit_canvas"])
        let activeRing = brush.view["rings"].array.lastIndex { $0["setting"].string != "size" } ?? 0
        let setting = brush.view["rings"][activeRing]["setting"].string
        try require(setting != "size", "The white dab edit must preserve its authored brush size")
        try await store.apply(["type": "set_tool_setting", "id": setting, "value": 1])
        try await tick()
        let nativeRing = brush.model["rings"][activeRing]["thumb"].point
        let nativePoint = CGPoint(x: brush.placement.center.x + nativeRing.x, y: brush.placement.center.y + nativeRing.y)
        let arc = brush.view["rings"][activeRing]["arc"], angle = (arc["start"].number + arc["sweep"].number * 0.8) * .pi / 180
        let editedPoint = CGPoint(x: brush.placement.center.x + cos(angle) * arc["radius"].number,
            y: brush.placement.center.y + sin(angle) * arc["radius"].number)
        let beforeEscape = store.state["brush"].stableKey
        let beforeEscapeSettings = store.state["tool_settings"].stableKey
        try event(.leftMouseDown, at: nativePoint, marker: input, number: 3)
        try event(.leftMouseDragged, at: editedPoint, marker: input, number: 3)
        try await wait("Active halo dab", failure: { store.failure }, step: tick) { brush.stamp != nil && !brush.stampNeedsFrame }
        guard let retainedStamp = brush.stamp else { throw HostFailure(message: "The active arc needs a brush dab") }
        try require(store.state["tool_settings"].stableKey != beforeEscapeSettings, "The native arc changes its setting before Escape restores it")
        let dabDirectory = ProcessInfo.processInfo.environment["CAPY_BUBBLE_CAPTURE_DIR"].map { URL(fileURLWithPath: $0, isDirectory: true) } ?? root
        try await checkWhiteDab(brush, window: window, marker: input, destination: dabDirectory.appendingPathComponent("\(platform)-\(theme)-white-dab.png"))
        brush.receive(brush.model)
        for _ in 0..<12 { try await tick() }
        try require(brush.stamp === retainedStamp, "Metadata-only stamp replies retain the decoded bitmap")
        try await settled("Unchanged dab preview idle")
        try require(!brush.stampNeedsFrame, "A ready unchanged dab stops polling")
        try key("\u{1b}", code: 53, window: window)
        try await drain(0.03)
        try event(.leftMouseUp, at: nativePoint, marker: input, number: 3)
        try await tick()
        try require(!model.active && store.state["brush"].stableKey == beforeEscape
            && store.state["tool_settings"].stableKey == beforeEscapeSettings, "Native Escape restores the ring edit before release")
        try await store.apply(["type": "set_color", "rgba": [0.2, 0.45, 0.8, 1]])
        let color = try await take({ $0["resolved_control"]["kind"].string == "color" }, at: CGPoint(x: 760, y: 400))
        let center = color.placement.center
        try await capture("color-ring")
        let wheel = CGPoint(x: center.x, y: center.y - 85), beforeColor = store.state["colors"]["foreground"].stableKey
        try require(model.down(wheel, id: 200, device: .touch, clicks: 1, doubleTime: 0.4, time: 20) == .capture, "Wheel captures its first finger")
        try await tick()
        try require(model.down(CGPoint(x: center.x + 45, y: center.y + 45), id: 201, device: .touch, clicks: 1, doubleTime: 0.4, time: 20.1) == .capture, "Wheel accepts the second finger")
        try await tick()
        try require(store.state["colors"]["foreground"].stableKey == beforeColor, "Pinching restores the wheel color exactly: \(beforeColor) -> \(store.state["colors"]["foreground"].stableKey)")
        _ = model.move(CGPoint(x: center.x + 95, y: center.y + 95), id: 201)
        _ = model.up(wheel, id: 200, cancelled: false); _ = model.up(center, id: 201, cancelled: false)
        try await tick(); try await settled("Color pinch")
        try require(color.view["diameter"].number > 216, "The native pair commits shared ring sizing")
        let diameter = color.view["diameter"].number, tinyCenter = color.placement.center
        let firstFinger = CGPoint(x: tinyCenter.x - 40, y: tinyCenter.y), secondFinger = CGPoint(x: tinyCenter.x + 40, y: tinyCenter.y)
        _ = model.down(firstFinger, id: 220, device: .touch, clicks: 1, doubleTime: 0.4, time: 20.2)
        _ = model.down(secondFinger, id: 221, device: .touch, clicks: 1, doubleTime: 0.4, time: 20.3)
        _ = model.move(CGPoint(x: secondFinger.x - 0.4, y: secondFinger.y), id: 221)
        try require(color.placement.pinch != 1, "A small pinch has a transient native scale")
        _ = model.up(firstFinger, id: 220, cancelled: false); _ = model.up(secondFinger, id: 221, cancelled: false)
        try require(color.placement.pinch == 1, "A sub-threshold pinch clears its transient scale on release")
        try await tick(); try await settled("Small pinch release")
        try require(color.view["diameter"].number == diameter, "A sub-threshold pinch preserves authored size")
        bubbles.showMenu(color, at: color.placement.center)
        _ = model.down(CGPoint(x: 1050, y: 700), id: 300, device: .mouse, clicks: 1, doubleTime: 0.4, time: 21)
        _ = model.up(CGPoint(x: 1050, y: 700), id: 300, cancelled: false)
        try await tick()
        try require(bubbles.menu.isNull, "An outside contact retires an in-flight menu reply")
        bubbles.showMenu(color, at: color.placement.center)
        try await wait("Bubble menu") { !bubbles.menu.isNull }
        let menu = bubbles.contextMenu()
        try require(menu.sections.flatMap { $0 }.contains { $0.isHeading }, "Bubble menu heading survives projection")
        guard let option = menu.actions.first(where: { $0.keepOpen && $0.enabled }) else { throw HostFailure(message: "Missing persistent bubble option") }
        option.action?(); try await tick()
        try require(!bubbles.menu.isNull, "An option keeps the bubble menu open")
        let returnedMenu = bubbles.contextMenu()
        guard let putBack = returnedMenu.actions.first(where: { $0.label == "Put back" }) else { throw HostFailure(message: "No Put back action") }
        bubbles.closeMenu(); putBack.action?(); try await tick()
        try require(!bubbles.items.contains { $0.id == color.id }, "Menu dismissal must not discard Put back")
        note("PASS ring and wheel rollback, pinch, persistent options and menu retirement: platform=\(platform) theme=\(theme)")
        let card = try await take({ $0["resolved_control"]["kind"].string == "panel" && $0["resolved_control"]["panel"].string == "layers" }, at: CGPoint(x: 800, y: 420))
        try require(!card.view["card"].isNull, "Layers comes out open")
        try await wait("Native card controls", step: tick) { model.nativeRegions.values.contains { $0.owner == card.id } }
        try await settled("Open Layers card")
        try await checkOwnerGlass(card)
        func cardBounds() -> CGRect {
            let size = card.view["card"]["size"].point
            return CGRect(x: card.placement.center.x - size.x / 2, y: card.placement.center.y - size.y / 2, width: size.x, height: size.y)
        }
        try await wait("Restored native card rows", failure: { store.failure }, step: tick) {
            model.nativeRegions.values.contains { $0.owner == card.id && $0.drags && cardBounds().contains($0.bounds) }
        }
        guard let rows = model.nativeRegions.values.first(where: { $0.owner == card.id && $0.drags && cardBounds().contains($0.bounds) }) else {
            throw HostFailure(message: "The card must retain its native rows")
        }
        let rowPoint = CGPoint(x: rows.bounds.midX, y: rows.bounds.minY + 20)
        try require(!model.context(at: rowPoint), "Native card rows retain secondary context input")
        try event(.rightMouseDown, at: rowPoint, marker: input, number: 402)
        try event(.rightMouseUp, at: rowPoint, marker: input, number: 402)
        try await wait("Native layer row context", step: tick) { store.workspace.hasPopover(excluding: bubbles.popup) }
        try require(bubbles.menu.isNull, "The bubble menu must not replace a native row menu")
        try await drain(0.15)
        try key("\u{1b}", code: 53, window: NSApp.keyWindow ?? window)
        try await wait("Native row context closes", step: tick) { !store.workspace.hasPopover(excluding: bubbles.popup) }
        let cardSize = card.view["card"]["size"].point
        let addPoint = CGPoint(x: card.placement.center.x - cardSize.x / 2 + 20,
            y: card.placement.center.y + cardSize.y / 2 - 24)
        try require(model.nativeRegions.values.contains { $0.owner == card.id && !$0.drags && $0.bounds.contains(addPoint) },
            "The Layers footer button must retain native input")
        let layerCount = store.state["layers"].array.count
        try event(.leftMouseDown, at: addPoint, marker: input, number: 400)
        try await drain(0.02)
        try event(.leftMouseUp, at: addPoint, marker: input, number: 400)
        try await wait("Native Layers footer adds a layer", failure: { store.failure }, step: tick) { store.state["layers"].array.count == layerCount + 1 }
        try await settled("Added layer settles")
        let nextSize = card.view["card"]["size"].point
        let footer = CGPoint(x: card.placement.center.x - nextSize.x / 2 + 20,
            y: card.placement.center.y + nextSize.y / 2 - 24)
        let drop = CGPoint(x: footer.x + 120, y: footer.y)
        try event(.leftMouseDown, at: footer, marker: input, number: 401); try await drain(0.02)
        try event(.leftMouseDragged, at: drop, marker: input, number: 401); try await drain(0.15)
        try event(.leftMouseUp, at: drop, marker: input, number: 401)
        try await settled("Native footer drag settles")
        try require(store.state["layers"].array.count == layerCount + 1, "Moving the card from its footer must not activate New layer")
        try await action(["type": "properties", "id": card.id]); try await drain(0.3)
        try require(card.view["card"]["properties"].bool, "The card retains the drawer's Properties page")
        try await action(["type": "properties", "id": card.id]); try await drain(0.3)
        try await action(["type": "option", "id": card.id, "option": ["kind": "full", "full": true]])
        try require(card.view["card"]["full"].bool && !store.panel("layers").isNull && !store.panel("properties").isNull,
            "Full card retains both native panel projections")
        try await capture("layers-full")
        try await action(["type": "close", "id": card.id]); try await drain(0.4)
        try require(card.view["card"].isNull, "Close folds the card")
        try await settled("Folded Layers face")
        try await checkOwnerGlass(card)
        try await store.apply(["type": "invoke", "command": "zen_mode"]); try await tick()
        try require(bubbles.items.count == 2, "Zen keeps the retained desk")
        let faded = BubblePlacement()
        faded.fade(down: true, faded: true, now: 1)
        try require(faded.tick(1, reduced: false), "Fade starts on its first drawn frame")
        _ = faded.tick(1.07, reduced: false)
        try require(abs(faded.opacity - 0.6) < 0.0001, "Fade uses a single smoothstep ramp")
        _ = faded.tick(1.14, reduced: false)
        faded.fade(down: false, faded: true, now: 2)
        _ = faded.tick(2.17, reduced: false)
        try require(abs(faded.opacity - 0.2) < 0.0001, "Stroke release retains its 180ms delay")
        _ = faded.tick(2.18, reduced: false); _ = faded.tick(2.25, reduced: false)
        try require(abs(faded.opacity - 0.6) < 0.0001, "Fade back also uses one smoothstep ramp")
        try await action(["type": "put_back_all"])
        try require(bubbles.items.isEmpty && !model.active, "Putting back retires native owners")
        try await settled("Empty desk idle")
        try await store.apply(["type": "invoke", "command": "zen_mode"])
        try await store.workspaces!.perform(["type": "switch", "id": "builtin:workspace:illustrator"])
        let workspace = store.workspace
        try await wait("Held toolbar sources", step: tick) { workspace.sources.values.filter { $0.surface == .tile }.count > 2 }
        let tiles = workspace.sources.values.filter { $0.surface == .tile && $0.canDrag }
            .compactMap { source -> (JSON, CGRect)? in
                guard let item = try? JSON.decode(source.item), item["kind"].string == "tile" else { return nil }
                return (item, source.bounds)
            }.sorted { $0.1.minY < $1.1.minY }
        guard let first = tiles.first(where: { item, _ in
            store.panel(item["panel"].string)["tiles"].array.first { $0["id"].uint == item["tile"].uint }?["tears_bubble"].bool == true
        }), let neighbor = tiles.last(where: { $0.0["panel"].string == first.0["panel"].string && $0.0.stableKey != first.0.stableKey }) else {
            throw HostFailure(message: "Two native toolbar tiles must support the held-drop check")
        }
        let originPoint = CGPoint(x: first.1.midX, y: first.1.midY)
        let dropPoint = CGPoint(x: neighbor.1.midX, y: neighbor.1.maxY - 2)
        let layout = store.state["workspace"]["layout"].stableKey
        workspace.start(first.0, point: originPoint); workspace.move(first.0, point: dropPoint)
        try await wait("Changing held toolbar hint", step: tick) { !workspace.dropHint["action"].isNull }
        workspace.move(first.0, point: dropPoint, released: true)
        await withCheckedContinuation { done in
            store.query(["type": "catalog"]) { _ in
                workspace.start(neighbor.0, point: dropPoint); done.resume()
            }
        }
        for _ in 0..<4 { try await tick() }
        try require(store.state["workspace"]["layout"].stableKey == layout && bubbles.items.isEmpty,
            "A newer native gesture must retire both bubble pickup and ordinary fallback from the old drop")
        workspace.cancel(neighbor.0)
        workspace.start(first.0, point: originPoint); workspace.move(first.0, point: dropPoint, released: true)
        try await wait("Current held toolbar drop", failure: { store.failure }, step: tick) { store.state["workspace"]["layout"].stableKey != layout }
        try require(bubbles.items.isEmpty, "An ordinary toolbar drop stays in its shared target")
        note("PASS tool bubbles platform=\(platform) theme=\(theme): retained motion, ring/wheel pair rollback, pinch, menus, card, Zen, fades and idle")
        let closed = await withCheckedContinuation { done in store.prepareClose { done.resume(returning: $0) } }
        try require(closed, "Close private bubble storage")
        withExtendedLifetime(input) {}
    }
    @MainActor static func checkTransitions() throws {
        var ramp = BubbleRamp(value: 0)
        ramp.set(1, duration: 0.2)
        _ = ramp.tick(0, reduced: false)
        try require(abs(ramp.tick(0.1, reduced: false) - 0.875) < 1e-10, "Bubble growth follows GTK cubic-out")
        func model(_ name: String?, rings: Bool = false) -> JSON {
            JSON(["view": ["id": 1, "diameter": 56, "chooser": name.map { [["kind": "icon", "label": $0]] } ?? [],
                "rings": rings ? [["label": "ring"]] : []], "rings": rings ? [["thumb": [0, 0]]] : []])
        }
        let item = BubblePresentation(model("first", rings: true))
        _ = item.tick(1, reduced: false); _ = item.tick(2, reduced: false)
        item.receive(model("second"))
        _ = item.tick(3, reduced: false); _ = item.tick(3.06, reduced: false)
        try require(item.choices.count == 2 && item.choices.contains { $0.leaving && $0.alpha > 0 },
            "Same-count chooser replacements retain outgoing items")
        try require(!item.rings.array.isEmpty && item.haloProgress > 0, "Closing halos retain their drawn rings")
        _ = item.tick(4, reduced: false)
        try require(item.choices.count == 1 && item.choices[0].value["label"].string == "second" && item.rings.array.isEmpty,
            "Settled transitions retire outgoing content")
        item.receive(model("third", rings: true)); _ = item.tick(5, reduced: true)
        try require(item.choices.count == 1 && item.choices[0].alpha == 1 && item.haloProgress == 1,
            "Reduced motion settles retained transitions immediately")
        let unopened = BubblePresentation(model("brief"))
        unopened.receive(model(nil)); _ = unopened.tick(0, reduced: false)
        try require(unopened.choices.isEmpty, "Closing a chooser before its first frame retires invisible items")
        func bead(_ id: String, _ x: Double, _ size: Double) -> JSON {
            JSON(["placement": ["bead": ["kind": id], "center": [x, 0], "size": size, "alpha": 1]])
        }
        let tween = BubblePresentation.tweenBeads([bead("foreground", 0, 20), bead("old", 10, 20)],
            [bead("foreground", 100, 40), bead("new", 30, 10)], 0.5)
        try require(tween.count == 3 && tween[0]["placement"]["center"][0].number == 50
            && tween[0]["placement"]["size"].number == 30 && tween[1]["placement"]["alpha"].number == 0.5
            && tween[2]["placement"]["alpha"].number == 0.5, "Bead identity retains and interpolates positions, sizes and alpha")
        let placement = BubblePlacement()
        placement.fade(down: true, faded: true, now: 0)
        _ = placement.tick(0, reduced: false); _ = placement.tick(0.07, reduced: false)
        placement.fade(down: true, faded: false, now: 0.08)
        _ = placement.tick(0.08, reduced: false); _ = placement.tick(0.3, reduced: false)
        try require(placement.opacity == 1, "Leaving the faded set during a stroke restores the bubble")
        placement.fade(down: true, faded: true, now: 1); _ = placement.tick(1, reduced: true)
        placement.fade(down: false, faded: false, now: 2)
        try require(placement.ignoresInput, "Stroke release keeps input disabled through its delay")
        _ = placement.tick(2.1, reduced: true)
        try require(placement.ignoresInput && placement.opacity == 0.2, "Delayed stroke reveal stays faded")
        _ = placement.tick(2.2, reduced: true)
        try require(!placement.ignoresInput && placement.opacity == 1, "Delayed reveal restores input and opacity")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do { try checkTransitions(); for platform: UInt32 in [0, 1] { for theme in ["light", "dark"] { try await run(platform, theme: theme) } }; exit(0) }
            catch { note("FAIL: " + error.localizedDescription); exit(1) }
        }
        NSApp.run()
    }
}
