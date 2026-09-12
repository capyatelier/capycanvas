import AppKit
import SwiftUI

@MainActor private final class ColorGeometry {
    var panel: [String: CGRect] = [:]
    var numbers: [String: CGRect] = [:]
}

/// Complete production controls on invisible AppKit surfaces, with current Rust
/// color models. Both presets are covered; these are not UIKit/Metal captures.
@main struct ColorPanelCaptures {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        precondition(NSImage(named: "icon-plus") != nil, "Set CAPY_TEST_ASSETS_APP")
        let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_COLOR_CAPTURES"]!)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var fixtures: [JSON] = []
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            let deadline = Date().addingTimeInterval(15)
            while store.catalog.isNull || store.snapshot["color_panel"].isNull {
                guard Date() < deadline else { throw HostFailure(message: "Color owner startup timed out") }
                try await Task.sleep(for: .milliseconds(5))
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
            for theme in ["light", "dark"] {
                try await edit(["type": "set_theme", "theme": theme])
                for space in ["hsv", "hls"] {
                    if store.snapshot["color_panel"]["space"].string != space {
                        try await edit(["type": "color", "action": ["op": "toggle_space"]])
                    }
                    for slot in ["foreground", "background", "transparent"] {
                        try await edit(["type": "color", "action": ["op": "select", "slot": slot]])
                        for width: CGFloat in [160, 226] {
                            let geometry = ColorGeometry(), height: CGFloat = 600
                            let palette = EditorPalette(source: store.state["palette"])
                            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height),
                                styleMask: [.borderless], backing: .buffered, defer: false)
                            window.isReleasedWhenClosed = false
                            defer { window.contentView = nil; window.close() }
                            window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
                            let content = ColorPanel(store: store)
                                .frame(width: width).fixedSize(horizontal: false, vertical: true)
                                .frame(width: width, height: height, alignment: .topLeading)
                                .coordinateSpace(name: "color-capture").coordinateSpace(name: "number-capture")
                                .environment(\.measureColorPanel, true).environment(\.measureNumberControls, true)
                                .onPreferenceChange(ColorPanelFrames.self) { geometry.panel = $0 }
                                .onPreferenceChange(NumberControlFrames.self) { geometry.numbers = $0 }
                                .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                                .environment(\.controlActiveState, .active)
                                .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
                                .foregroundStyle(palette["text"]).tint(palette.accent).background(palette["panel"])
                            let host = NSHostingView(rootView: content); window.contentView = host
                            for _ in 0..<25 { host.layoutSubtreeIfNeeded(); try await Task.sleep(for: .milliseconds(5)) }
                            guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
                                throw HostFailure(message: "No color panel bitmap")
                            }
                            window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                            let name = "\(platform)-\(theme)-\(space)-\(slot)-\(Int(width))"
                            try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("native-\(name).png"))
                            let model = store.snapshot["color_panel"]
                            let formatted = try model["components"].array.map { item in
                                ["value": try store.resolveNumber(item["numeric"], value: item["value"].number, operation: ["type": "format"]).raw,
                                 "minimum": try store.resolveNumber(item["numeric"], value: item["numeric"]["min"].number, operation: ["type": "format"]).raw]
                            }
                            let frames = geometry.panel.merging(geometry.numbers) { _, new in new }
                            precondition(geometry.panel.count == 10 && geometry.numbers.count == 21)
                            precondition(store.failure == nil, store.failure ?? "")
                            let wheel = geometry.panel["wheel"]!
                            let paint = store.state["colors"]["paint_slot"].string
                            try JSON(["space": model["space"].raw,
                                "rgba": store.state["colors"][paint].raw,
                                "hue": model["components"][0]["value"].raw,
                                "viewport": [width, height],
                                "wheel": [wheel.minX, wheel.minY, wheel.width, wheel.height]])
                                .encoded().write(to: directory.appendingPathComponent("oracle-\(name).json"), atomically: true, encoding: .utf8)
                            fixtures.append(JSON(["name": name, "platform": platform, "width": width, "height": height,
                                "scale": Double(bitmap.pixelsWide) / width, "theme": theme, "space": space, "slot": slot,
                                "text_size": store.catalog["text_size_pt"].number * 4 / 3, "palette": store.state["palette"].raw,
                                "model": model.raw, "formatted": formatted, "frames": frames.mapValues {
                                    ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }]))
                        }
                    }
                }
            }
            print("Captured 24 full color panels for Apple preset \(platform)")
        }
        try JSON(["schema": 1, "fixtures": fixtures.map(\.raw), "scope": "Shared SwiftUI color controls; AppKit pixels, not physical UIKit/Metal"])
            .encoded().write(to: directory.appendingPathComponent("fixtures.json"), atomically: true, encoding: .utf8)
    }
}
