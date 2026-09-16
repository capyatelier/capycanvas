import AppKit
import SwiftUI

@MainActor private final class Geometry { var frames: [String: CGRect] = [:] }
@main final class ColorPanelInteractions: NativeWorkspaceInputFixture {
    @MainActor static func run() async throws {
        let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_COLOR_CAPTURES"]!)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let store = EditorStore(platform: 0, persistence: EditorPersistence(root: nil))
        let deadline = Date().addingTimeInterval(15)
        while store.catalog.isNull || store.snapshot["color_panel"].isNull {
            try require(Date() < deadline, "Color owner did not start")
            try await drain(0.02)
        }
        func edit(_ action: [String: Any]) async throws {
            try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                store.edit(action) { error in
                    if let error { c.resume(throwing: HostFailure(message: error)) } else { c.resume() }
                }
            }
        }
        try await edit(["type": "color", "action": ["op": "select", "slot": "background"]])
        try await edit(["type": "set_color", "rgba": [0.1, 0.7, 0.3, 0.75]])
        try await edit(["type": "color", "action": ["op": "select", "slot": "foreground"]])
        try await edit(["type": "set_color", "rgba": [0.8, 0.2, 0.4, 0.5]])
        var fixtures: [JSON] = []
        for theme in ["light", "dark"] {
            try await edit(["type": "set_theme", "theme": theme])
            if store.snapshot["color_panel"]["readout"].string != "shape" {
                try await edit(["type": "color", "action": ["op": "toggle_readout"]])
            }
            let geometry = Geometry()
            let width: CGFloat = 226
            let palette = EditorPalette(source: store.state["palette"])
            let content = ColorPanel(store: store).frame(width: width, height: width)
                .coordinateSpace(name: "color-capture").environment(\.measureColorPanel, true)
                .onPreferenceChange(ColorPanelFrames.self) { geometry.frames = $0 }
                .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                .environment(\.controlActiveState, .active).foregroundStyle(palette["text"]).tint(palette.accent)
                .background(palette["panel"])
            let window = NSWindow(
                contentRect: NSRect(x: 100, y: 100, width: width, height: width), styleMask: [.titled, .closable],
                backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.title = "Color control check"
            window.acceptsMouseMovedEvents = true
            let host = NSHostingView(rootView: content)
            window.contentView = host
            defer {
                window.contentView = nil
                window.close()
            }
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            try await drain(0.3)
            try require(geometry.frames.count == 12, "Incomplete control geometry")
            var eventNumber = 0
            func mouse(_ type: NSEvent.EventType, _ point: CGPoint) async throws {
                eventNumber += 1
                try require(host.bounds.contains(point), "Pointer must stay inside the fixture content")
                let screen = window.convertPoint(toScreen: host.convert(point, to: nil))
                let cgPoint = CGPoint(x: screen.x, y: NSScreen.screens.first!.frame.maxY - screen.y)
                try require(CGWarpMouseCursorPosition(cgPoint) == .success, "Native pointer movement failed")
                let event = NSEvent.mouseEvent(
                    with: type, location: host.convert(point, to: nil), modifierFlags: [],
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                    eventNumber: eventNumber, clickCount: 1, pressure: type == .leftMouseDown ? 0.5 : 0)!
                NSApp.postEvent(event, atStart: false)
                try await drain(0.12)
            }
            let outside = CGPoint(x: width - 2, y: 2)
            func point(_ target: String) -> CGPoint {
                let f = geometry.frames[target]!
                return CGPoint(x: target == "background" ? f.maxX - 3 : f.midX, y: f.midY)
            }
            func capture(_ phase: String, target: String = "", pressed: Bool = false) throws {
                host.layoutSubtreeIfNeeded()
                host.displayIfNeeded()
                let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds)!
                host.cacheDisplay(in: host.bounds, to: bitmap)
                let name = "0-\(theme)-\(phase)-226"
                let model = store.snapshot["color_panel"]
                let wheel = geometry.frames["wheel"]!
                let scale = CGFloat(bitmap.pixelsWide) / width
                try bitmap.representation(using: .png, properties: [:])!.write(
                    to: directory.appendingPathComponent("native-\(name).png"))
                let resourcePointer = capy_apple_color_resources(Float(width), 0)!
                let resources = try JSON.decode(String(cString: resourcePointer))
                capy_apple_string_free(resourcePointer)
                let side = UInt32(ceil(wheel.width))
                var bytes = Data(count: Int(side * side * 4))
                try require(
                    bytes.withUnsafeMutableBytes {
                        capy_apple_color_field(
                            side, Float(model["wheel_components"][0].number), 0, "Srgb", false,
                            $0.bindMemory(to: UInt8.self).baseAddress, $0.count)
                    } == 1, "Field bytes failed")
                let field = "field-\(name).rgba"
                try bytes.write(to: directory.appendingPathComponent(field))
                fixtures.append(
                    JSON([
                        "name": name, "platform": 0, "width": width, "height": width, "scale": scale, "theme": theme,
                        "shape": "circle", "readout": model["readout"].string, "slot": "foreground",
                        "palette": store.state["palette"].raw, "model": model.raw, "resources": resources.raw,
                        "field_file": field, "field_side": side,
                        "frames": geometry.frames.mapValues {
                            ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height]
                        }, "interaction": ["target": target, "pressed": pressed],
                    ]))
            }
            try await mouse(.mouseMoved, outside)
            try capture("rest")
            let before = store.state["colors"].stableKey
            for target in ["foreground", "background", "transparent", "shape-0", "swap"] {
                try await mouse(.mouseMoved, point(target))
                try capture("hover-" + target, target: target)
                try await mouse(.mouseMoved, outside)
            }
            for target in ["background", "shape-0", "swap"] {
                try await mouse(.mouseMoved, point(target))
                try await mouse(.leftMouseDown, point(target))
                try capture("pressed-" + target, target: target, pressed: true)
                try require(store.state["colors"].stableKey == before, "Press must wait for activation")
                try await mouse(.leftMouseDragged, outside)
                try await mouse(.leftMouseUp, outside)
                try require(store.state["colors"].stableKey == before, "A cancelled button press must not edit paint")
            }
            try require(store.failure == nil, store.failure ?? "")
            note("Completed native color states in \(theme)")
        }
        try require(fixtures.count == 18, "Incomplete interaction matrix")
        try JSON([
            "schema": 2, "interaction_states": true, "fixtures": fixtures.map(\.raw),
            "scope": "Native AppKit hover and press cancellation; not keyboard focus or physical iPad input",
        ]).encoded().write(to: directory.appendingPathComponent("fixtures.json"), atomically: true, encoding: .utf8)
        print("PASS: 18 native Color interaction captures and cancelled presses")
    }
    @MainActor static func main() {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do {
                try await run()
                exit(0)
            } catch {
                note("FAIL: \(error)")
                exit(1)
            }
        }
        NSApp.run()
    }
}
