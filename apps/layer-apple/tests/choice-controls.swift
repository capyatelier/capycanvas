import AppKit
import SwiftUI

@MainActor private final class ChoiceGeometry { var frames: [String: CGRect] = [:] }

/// Production shared choices with actual Rust blend names, in invisible hosts.
@main struct ChoiceCaptures {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
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
                            let geometry = ChoiceGeometry(), height: CGFloat = 140
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
                                        identifier: "property", background: palette["input"]) { _ in }
                                }.frame(width: width - 12, height: 34).disabled(!enabled)
                                    .opacity(enabled ? 1 : 0.4).offset(x: 6, y: 6)
                                EditorChoice(label: "Channel", options: options, selected: selected,
                                    identifier: "wide", background: palette["input"]) { _ in }
                                    .frame(width: width - 12, height: 34).disabled(!enabled)
                                    .opacity(enabled ? 1 : 0.4).offset(x: 6, y: 56)
                                EditorChoice(label: "Layer blend mode", options: options, selected: selected,
                                    identifier: "compact", background: palette["input"], compact: true) { _ in }
                                    .frame(width: (width - 18) / 2, height: 24).disabled(!enabled).offset(x: 6, y: 106)
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
                            fixtures.append(JSON(["name": name, "platform": platform, "width": width, "height": height,
                                "scale": Double(bitmap.pixelsWide) / width, "theme": theme, "text_size": textSize,
                                "palette": store.state["palette"].raw, "options": options, "selected": selected,
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
    }
}
