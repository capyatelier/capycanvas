import AppKit
import SwiftUI

@MainActor private final class ChoiceGeometry { var frames: [String: CGRect] = [:] }

/// Shared choices in invisible hosts and production properties in owned windows.
@main final class ChoiceCaptures: NativeWorkspaceInputFixture {
    @MainActor private static func addFilter(platform: UInt32, theme: String,
        width: CGFloat, directory: URL) async throws -> JSON {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil), managedWorkspaces: false)
        try await wait("Production layer properties", seconds: 15, failure: { store.failure }) {
            !store.catalog.isNull && !store.state["layer_properties"]["add_filter"].isNull
        }
        try await store.apply(["type": "set_theme", "theme": theme])
        try await wait("Properties theme", seconds: 15, failure: { store.failure }) { store.state["theme"].string == theme }
        let palette = EditorPalette(source: store.state["palette"])
        let textSize = store.catalog["text_size_pt"].number * 4 / 3
        let window = NSWindow(contentRect: CGRect(x: 100, y: 100, width: width, height: 240),
            styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
        let content = LayerPropertiesPanel(store: store)
            .padding(6).frame(width: width, height: 240, alignment: .topLeading)
            .font(.system(size: textSize)).foregroundStyle(palette["text"]).background(palette["panel"])
            .environment(\.colorScheme, theme == "dark" ? .dark : .light)
            .environment(\.editorPalette, palette).environment(\.editorPopupStore, store)
            .environment(\.capyNativeCopy, store.catalog["native_copy"])
            .environment(\.capyCommonCopy, store.bootstrap["common"])
            .environment(\.capyInterfaceLanguage, store.interfaceLanguage)
            .modifier(EditorPopoverHost())
        let host = NSHostingView(rootView: content); window.contentView = host
        window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
        try await drain(0.3)
        try await wait("Visible production Add Filter", seconds: 5, failure: { store.failure }) {
            window.isVisible && window.isKeyWindow && nativeAccessibilityElement(host, identifier: "properties-add-filter") != nil
        }
        guard let button = nativeAccessibilityElement(host, identifier: "properties-add-filter") else {
            throw HostFailure(message: "Mounted production Add Filter button must expose its bounds")
        }
        let frame = nativeAccessibilityBounds(button), caption = store.state["layer_properties"]["add_filter"]["title"].string
        try require(!caption.isEmpty && frame.width > 0 && frame.height > 0, "Production Add Filter must expose caption and nonzero bounds")
        let advance = EditorTextMetrics.width(caption, size: textSize, weight: .bold)
        let natural = advance + 2 * 16 + 2 * store.panelMetrics.spacing + 2 * 6
        let pixel = 1 / window.backingScaleFactor
        guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
            throw HostFailure(message: "No properties bitmap")
        }
        window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
        let name = "properties-\(platform)-\(theme)-\(Int(width))"
        try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("native-\(name).png"))
        let measurement = JSON(["name": name, "platform": platform, "theme": theme, "width": width, "height": 240,
            "caption": caption, "text_size": textSize, "caption_advance": advance, "natural_button_width": natural,
            "button_bounds": ["x": frame.minX, "y": frame.minY, "width": frame.width, "height": frame.height],
            "scale": Double(bitmap.pixelsWide) / width])
        try measurement.encoded().write(to: directory.appendingPathComponent(name + ".json"), atomically: true, encoding: .utf8)
        try require(frame.width >= natural - 0.001 && frame.width <= natural + pixel + 0.001,
            "Add Filter must allocate both sixteen-point icons, shared gaps, caption and padding: actual=\(frame.width), natural=\(natural)")
        try require(abs(frame.height - store.panelMetrics.height) < 0.5, "Add Filter must keep the panel control height")
        return measurement
    }

    @MainActor static func run() async throws {
        precondition(NSImage(named: "icon-chevron-down") != nil, "Set CAPY_TEST_ASSETS_APP")
        let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_CHOICE_CAPTURES"]!)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var fixtures: [JSON] = []
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            let deadline = Date().addingTimeInterval(15)
            while store.catalog.isNull {
                guard Date() < deadline else { fatalError("Choice catalog startup timed out") }
                try await Task.sleep(for: .milliseconds(5))
            }
            let options = store.catalog["layer_blends"].array.map(\.string)
            precondition(!options.isEmpty)
            precondition(SquircleShape.controlRadius == store.panelMetrics.radius, "Controls use the shared control radius")
            let longest = options.indices.max { options[$0].count < options[$1].count }!
            for theme in ["light", "dark"] {
                try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
                    store.edit(["type": "set_theme", "theme": theme]) { error in
                        if let error { c.resume(throwing: HostFailure(message: error)) } else { c.resume() }
                    }
                }
                for width: CGFloat in [160, 226, 320] {
                    for selected in [0, longest] {
                        for enabled in [true, false] {
                            let geometry = ChoiceGeometry(), height: CGFloat = 140, control = store.panelMetrics.height
                            let palette = EditorPalette(source: store.state["palette"])
                            let textSize = store.catalog["text_size_pt"].number * 4 / 3
                            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height),
                                styleMask: [.borderless], backing: .buffered, defer: false)
                            window.isReleasedWhenClosed = false
                            defer { window.contentView = nil; window.close() }
                            window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
                            let content = ZStack(alignment: .topLeading) {
                                PropertyChoiceRow {
                                    Text("Blend mode").lineLimit(1)
                                    EditorChoice(label: "Blend mode", options: options, selected: selected,
                                        identifier: "property", background: palette["input"], height: control) { _ in }
                                }.frame(width: width - 12, height: control).disabled(!enabled)
                                    .opacity(enabled ? 1 : 0.4).offset(x: 6, y: 6)
                                EditorChoice(label: "Configuration layer", options: options, selected: selected,
                                    identifier: "wide", background: palette["input"], height: control) { _ in }
                                    .frame(width: width - 12, height: control)
                                    .offset(x: 6, y: 56)
                                EditorChoice(label: "Layer blend mode", options: options, selected: selected,
                                    identifier: "compact", background: palette["input"], presentation: .layerMenu, height: control) { _ in }
                                    .frame(width: (width - 12 - store.panelMetrics.spacing) / 2, height: control).disabled(!enabled).offset(x: 6, y: 106)
                            }.frame(width: width, height: height, alignment: .topLeading)
                                .coordinateSpace(name: "choice-capture").environment(\.measureChoices, true)
                                .onPreferenceChange(ChoiceFrames.self) { geometry.frames = $0 }
                                .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                                .font(.system(size: textSize)).foregroundStyle(palette["text"])
                                .background(palette["panel"])
                            let host = NSHostingView(rootView: content); window.contentView = host
                            for _ in 0..<10 { host.layoutSubtreeIfNeeded(); try await Task.sleep(for: .milliseconds(5)) }
                            guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { fatalError("No choice bitmap") }
                            window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                            let name = "\(platform)-\(theme)-\(Int(width))-\(selected)-\(enabled ? "enabled" : "disabled")"
                            try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("native-\(name).png"))
                            precondition(geometry.frames.count == 9)
                            for identifier in ["property", "wide", "compact"] {
                                let bounds = geometry.frames[identifier]!, text = geometry.frames[identifier + ":text"]!
                                precondition(abs(bounds.height - control) < 0.5
                                    && abs(text.minX - bounds.minX - (identifier == "compact" ? 6 : 10)) < 0.5
                                    && abs(text.width - bounds.width + 28) < 0.5,
                                    "Choices must retain their height and role-specific text insets")
                            }
                            fixtures.append(JSON(["name": name, "platform": platform, "width": width, "height": height,
                                "scale": Double(bitmap.pixelsWide) / width, "theme": theme, "text_size": textSize,
                                "palette": store.state["palette"].raw, "panel_metrics": store.catalog["panel_metrics"].raw,
                                "options": options, "selected": selected,
                                "enabled": enabled, "frames": geometry.frames.mapValues {
                                    ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }]))
                        }
                    }
                }
            }
        }
        try JSON(["schema": 1, "fixtures": fixtures.map(\.raw)])
            .encoded().write(to: directory.appendingPathComponent("fixtures.json"), atomically: true, encoding: .utf8)
        print("Captured \(fixtures.count) property, full-width and compact choice combinations")
        try require(fixtures.count == 48, "All original choice captures must complete before visible properties")
        NSApp.setActivationPolicy(.accessory)
        var properties: [JSON] = []
        for platform: UInt32 in [0, 1] {
            for theme in ["light", "dark"] {
                for width: CGFloat in [160, 226, 320] {
                    properties.append(try await addFilter(platform: platform, theme: theme, width: width, directory: directory))
                }
            }
        }
        try JSON(["schema": 1, "properties": properties.map(\.raw)])
            .encoded().write(to: directory.appendingPathComponent("properties.json"), atomically: true, encoding: .utf8)
        note("PASS: \(properties.count) production Add Filter allocations")
    }
    @MainActor static func main() {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        Task { @MainActor in
            do { try await run(); exit(0) }
            catch { note("FAIL: \(error)"); exit(1) }
        }
        NSApp.run()
    }
}
