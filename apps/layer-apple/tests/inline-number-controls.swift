import AppKit
import SwiftUI

@MainActor private final class InlineGeometry { var frames: [String: CGRect] = [:] }

/// The production layer-opacity control with real shared catalog and edits.
@main struct InlineNumberCaptures {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_INLINE_CAPTURES"]!)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var fixtures: [JSON] = []
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            let deadline = Date().addingTimeInterval(15)
            while store.catalog.isNull || store.state.isNull {
                precondition(Date() < deadline, "Inline control startup timed out")
                try await Task.sleep(for: .milliseconds(5))
            }
            func edit(_ action: [String: Any]) async throws {
                try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                    store.edit(action) { error in
                        if let error { c.resume(throwing: HostFailure(message: error)) } else { c.resume() }
                    }
                }
            }
            let control = store.catalog["layer_opacity"]
            for theme in ["light", "dark"] {
                try await edit(["type": "set_theme", "theme": theme])
                for panelWidth: CGFloat in [160, 226, 320] {
                    let width = (panelWidth - 18) / 2, height: CGFloat = 36
                    for value in [0.0, 0.5, 1.0] {
                        try await edit(["type": "set_layer_opacity", "opacity": value])
                        for enabled in [true, false] {
                            let geometry = InlineGeometry(), palette = EditorPalette(source: store.state["palette"])
                            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width + 12, height: height),
                                styleMask: [.borderless], backing: .buffered, defer: false)
                            window.isReleasedWhenClosed = false
                            defer { window.contentView = nil; window.close() }
                            window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
                            let textSize = store.catalog["text_size_pt"].number * 4 / 3
                            let content = LayerOpacityField(store: store).disabled(!enabled)
                                .frame(width: width).padding(6).frame(width: width + 12, height: height, alignment: .topLeading)
                                .coordinateSpace(name: "number-capture").environment(\.measureNumberControls, true)
                                .onPreferenceChange(NumberControlFrames.self) { geometry.frames = $0 }
                                .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                                .environment(\.controlActiveState, .active)
                                .font(.system(size: textSize)).foregroundStyle(palette["text"])
                                .background(palette["panel"])
                            let host = NSHostingView(rootView: content); window.contentView = host
                            for _ in 0..<20 { host.layoutSubtreeIfNeeded(); try await Task.sleep(for: .milliseconds(5)) }
                            guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { fatalError("No inline control bitmap") }
                            window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                            let name = "\(platform)-\(theme)-\(Int(panelWidth))-\(Int(value * 100))-\(enabled ? "enabled" : "disabled")"
                            try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("native-\(name).png"))
                            fixtures.append(JSON(["name": name, "platform": platform, "width": width + 12, "height": height,
                                "scale": Double(bitmap.pixelsWide) / (width + 12), "theme": theme, "text_size": textSize,
                                "palette": store.state["palette"].raw, "control": control.raw, "value": value, "enabled": enabled,
                                "formatted": try store.resolveNumber(control, value: value, operation: ["type": "format"]).raw,
                                "minimum": try store.resolveNumber(control, value: control["min"].number, operation: ["type": "format"]).raw,
                                "maximum": try store.resolveNumber(control, value: control["max"].number, operation: ["type": "format"]).raw,
                                "frames": geometry.frames.mapValues {
                                    ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }]))
                        }
                    }
                }
            }
        }
        try JSON(["schema": 1, "fixtures": fixtures.map(\.raw)])
            .encoded().write(to: directory.appendingPathComponent("fixtures.json"), atomically: true, encoding: .utf8)
        print("Captured \(fixtures.count) inline layer-opacity controls")
    }
}
