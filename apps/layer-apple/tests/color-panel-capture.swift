import SwiftUI
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

@MainActor private final class ColorGeometry { var panel: [String: CGRect] = [:] }

/// Production shared controls, both presets, all shapes/readouts/slots and the
/// minimum supported width. Component pixels do not substitute for native input.
enum ColorPanelCaptures {
    @MainActor static func capture(directory: URL) async throws {
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
                for shape in ["circle", "square", "triangle"] {
                    try await edit(["type": "color", "action": ["op": "shape", "shape": shape]])
                    for readout in ["shape", "rgb"] {
                        if store.snapshot["color_panel"]["readout"].string != readout {
                            try await edit(["type": "color", "action": ["op": "toggle_readout"]])
                        }
                        for slot in ["foreground", "background", "transparent"] {
                            try await edit(["type": "color", "action": ["op": "select", "slot": slot]])
                            for width: CGFloat in [128, 160, 226] {
                                let geometry = ColorGeometry()
                                let palette = EditorPalette(source: store.state["palette"])
                                let content = ColorPanel(store: store).frame(width: width, height: width)
                                    .coordinateSpace(name: "color-capture")
                                    .environment(\.measureColorPanel, true)
                                    .onPreferenceChange(ColorPanelFrames.self) { geometry.panel = $0 }
                                    .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                                    .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
                                    .foregroundStyle(palette["text"]).tint(palette.accent).background(palette["panel"])
                                #if canImport(AppKit)
                                let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: width),
                                    styleMask: [.borderless], backing: .buffered, defer: false)
                                window.isReleasedWhenClosed = false
                                defer { window.contentView = nil; window.close() }
                                window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
                                let host = NSHostingView(rootView: content.environment(\.controlActiveState, .active))
                                window.contentView = host
                                // Mount the actual host surface for the pixel matrix.
                                window.orderFront(nil)
                                func image() throws -> (Data, CGFloat) {
                                    host.layoutSubtreeIfNeeded(); host.displayIfNeeded()
                                    guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
                                        throw HostFailure(message: "No color panel bitmap")
                                    }
                                    window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                                    return (bitmap.representation(using: .png, properties: [:])!, CGFloat(bitmap.pixelsWide) / width)
                                }
                                #else
                                let host = UIHostingController(rootView: content)
                                host.safeAreaRegions = []; host.overrideUserInterfaceStyle = theme == "dark" ? .dark : .light
                                let window = UIWindow(frame: CGRect(x: 0, y: 0, width: width, height: width))
                                window.rootViewController = host; window.makeKeyAndVisible(); host.view.frame = window.bounds
                                defer { window.isHidden = true; window.rootViewController = nil }
                                func image() throws -> (Data, CGFloat) {
                                    host.view.setNeedsLayout(); host.view.layoutIfNeeded()
                                    let format = UIGraphicsImageRendererFormat()
                                    format.opaque = true; format.preferredRange = .standard; format.scale = window.screen.scale
                                    var painted = false
                                    let image = UIGraphicsImageRenderer(size: host.view.bounds.size, format: format).image { _ in
                                        painted = host.view.drawHierarchy(in: host.view.bounds, afterScreenUpdates: true)
                                    }
                                    guard painted, let png = image.pngData() else { throw HostFailure(message: "UIKit did not paint the panel") }
                                    return (png, format.scale)
                                }
                                #endif
                                // Layout publication can precede Canvas's backing pixels. Require
                                // consecutive stable captures instead of accepting a partial frame.
                                var previous = Data(), png = Data(), scale: CGFloat = 1, stable = 0
                                for _ in 0..<40 {
                                    try await Task.sleep(for: .milliseconds(25))
                                    (png, scale) = try image()
                                    stable = png == previous ? stable + 1 : 0; previous = png
                                    if stable >= 2 && geometry.panel.count == 12 { break }
                                }
                                precondition(stable >= 2, "Color pixels did not settle")
                                let name = "\(platform)-\(theme)-\(shape)-\(readout)-\(slot)-\(Int(width))"
                                try png.write(to: directory.appendingPathComponent("native-\(name).png"))
                                let model = store.snapshot["color_panel"]
                                precondition(geometry.panel.count == 12, "Incomplete color geometry: \(geometry.panel.keys.sorted())")
                                precondition(store.failure == nil, store.failure ?? "")
                                let wheel = geometry.panel["wheel"]!
                                let shapeID = ColorWheelShape(shape)
                                let resources = ColorUI.resolve(["type": "picker_layout", "size": width, "hdr": false])["layout"]
                                for key in ["wheel", "foreground", "background", "transparent", "swap", "readout"] {
                                    let expected = resources[key], actual = geometry.panel[key]!
                                    for (a, b) in zip([actual.minX, actual.minY, actual.width, actual.height], expected.array.map(\.number)) {
                                        precondition(abs(a - b) <= 0.5, "\(key) placement differs from shared layout")
                                    }
                                }
                                var fieldFile: String? = nil
                                var fieldSide = 0
                                if shape != "square" {
                                    fieldSide = Int(ceil(wheel.width * (shape == "circle" ? 1 : scale)))
                                    var bytes = Data(count: fieldSide * fieldSide * 4)
                                    let result = bytes.withUnsafeMutableBytes {
                                        capy_apple_color_field(UInt32(fieldSide), Float(model["wheel_components"][0].number),
                                            shapeID.rawValue, "Srgb", false, $0.bindMemory(to: UInt8.self).baseAddress, $0.count)
                                    }
                                    precondition(result == 1)
                                    fieldFile = "field-\(name).rgba"
                                    try bytes.write(to: directory.appendingPathComponent(fieldFile!))
                                }
                                let paint = store.state["colors"]["paint_slot"].string
                                try JSON(["space": model["space"].raw, "shape": shape,
                                    "rgba": store.state["colors"][paint].raw, "hue": model["wheel_components"][0].raw,
                                    "viewport": [width, width], "marker_radius": min(10, max(6, wheel.width * 0.04)),
                                    "field_corner_radius": shape == "square" ? min(6, wheel.width * 0.02) : 0,
                                    "wheel": [wheel.minX, wheel.minY, wheel.width, wheel.height]])
                                    .encoded().write(to: directory.appendingPathComponent("oracle-\(name).json"), atomically: true, encoding: .utf8)
                                fixtures.append(JSON(["name": name, "platform": platform, "width": width, "height": width,
                                    "scale": scale, "theme": theme, "shape": shape, "readout": readout, "slot": slot,
                                    "palette": store.state["palette"].raw, "model": model.raw, "resources": resources.raw,
                                    "preview_space": "DisplayP3", "field_file": fieldFile as Any? ?? NSNull(), "field_side": fieldSide,
                                    "frames": geometry.panel.mapValues {
                                        ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }]))
                            }
                        }
                    }
                }
            }
            print("Captured all compact color panels for Apple preset \(platform)")
        }
        precondition(fixtures.count == 216)
        try JSON(["schema": 2, "fixtures": fixtures.map(\.raw), "scope": "Shared SwiftUI color components; not a native-input or complete-editor check"])
            .encoded().write(to: directory.appendingPathComponent("fixtures.json"), atomically: true, encoding: .utf8)
    }
}

#if canImport(AppKit)
@main struct AppKitColorCaptures {
    @MainActor static func main() {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.accessory)
        precondition(NSImage(named: "icon-color-circle") != nil, "Set CAPY_TEST_ASSETS_APP to a current build")
        Task { @MainActor in
            do {
                try await ColorPanelCaptures.capture(directory: URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_COLOR_CAPTURES"]!))
                exit(0)
            } catch { print("FAIL: AppKit Color panels: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
#else
@MainActor private final class ColorCaptureDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication,
        didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        Task { @MainActor in
            do {
                precondition(UIImage(named: "icon-color-circle") != nil)
                let directory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("ColorCaptures")
                try await ColorPanelCaptures.capture(directory: directory)
                print("PASS: UIKit compact Color panels"); exit(0)
            } catch { print("FAIL: UIKit Color panels: \(error)"); exit(1) }
        }
        return true
    }
}
@main struct UIKitColorCaptures {
    static func main() { UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(ColorCaptureDelegate.self)) }
}
#endif
