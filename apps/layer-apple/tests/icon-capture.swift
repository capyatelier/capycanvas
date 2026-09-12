import AppKit
import SwiftUI

/// Shared glyphs only, with explicit foreground/background and disabled opacity.
/// Invisible AppKit surfaces exercise the compiled vectors, not SVG test doubles.
@main struct IconCaptures {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        precondition(NSDataAsset(name: "shared-icon-paints") != nil, "Rebuild CAPY_TEST_ASSETS_APP with current assets")
        let environment = ProcessInfo.processInfo.environment
        let directory = URL(fileURLWithPath: environment["CAPY_ICON_CAPTURES"]!)
        let source = URL(fileURLWithPath: environment["CAPY_ICON_SOURCES"]!)
        let names = try FileManager.default.contentsOfDirectory(at: source, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "svg" }.map { $0.deletingPathExtension().lastPathComponent }
            .sorted()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let width = 576, height = ((names.count + 11) / 12) * 48
        var fixtures: [JSON] = []
        for theme in ["light", "dark"] {
            let background = theme == "light" ? "#eeeeee" : "#303030"
            for size in [16, 24, 32] {
                for mode in ["normal", "accent", "disabled"] {
                    let foreground = mode == "accent" ? "#3584e4" : theme == "light" ? "#333333" : "#fafafa"
                    let opacity = mode == "disabled" ? 0.36 : 1.0
                    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height),
                        styleMask: [.borderless], backing: .buffered, defer: false)
                    window.isReleasedWhenClosed = false
                    defer { window.contentView = nil; window.close() }
                    let content = ZStack(alignment: .topLeading) {
                        ForEach(names.indices, id: \.self) { index in
                            SharedIcon(name: names[index], size: CGFloat(size))
                                .foregroundStyle(Color(hex: foreground)).opacity(opacity)
                                .position(x: CGFloat(index % 12 * 48 + 24), y: CGFloat(index / 12 * 48 + 24))
                        }
                    }.frame(width: CGFloat(width), height: CGFloat(height))
                        .background(Color(hex: background))
                        .environment(\.colorScheme, theme == "light" ? .light : .dark)
                    let host = NSHostingView(rootView: content); window.contentView = host
                    for _ in 0..<10 { host.layoutSubtreeIfNeeded(); try await Task.sleep(for: .milliseconds(5)) }
                    guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { fatalError("No icon bitmap") }
                    host.cacheDisplay(in: host.bounds, to: bitmap)
                    let name = "\(theme)-\(size)-\(mode)"
                    try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("native-\(name).png"))
                    fixtures.append(JSON(["name": name, "width": width, "height": height,
                        "scale": Double(bitmap.pixelsWide) / Double(width), "size": size, "theme": theme,
                        "foreground": foreground, "background": background, "opacity": opacity, "icons": names]))
                }
            }
        }
        try JSON(["schema": 1, "fixtures": fixtures.map(\.raw)])
            .encoded().write(to: directory.appendingPathComponent("fixtures.json"), atomically: true, encoding: .utf8)
        print("Captured \(names.count) shared SVG icons in \(fixtures.count) size/tint/opacity combinations")
    }
}
