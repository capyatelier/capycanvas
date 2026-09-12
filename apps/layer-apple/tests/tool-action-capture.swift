import AppKit
import SwiftUI

private struct ActionFrames: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue()) { _, new in new }
    }
}
@MainActor private final class ActionGeometry { var frames: [String: CGRect] = [:] }

/// Captures the production control on an invisible AppKit surface. Four columns
/// retain all enabled/selected combinations, including unavailable selections.
@main struct ToolActionCaptures {
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        let fixture = try JSON.decode(String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8))
        let palette = EditorPalette(source: fixture["palette"])
        let width = fixture["width"].number, height = fixture["height"].number
        let geometry = ActionGeometry()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height),
            styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        window.appearance = NSAppearance(named: fixture["theme"].string == "dark" ? .darkAqua : .aqua)
        let content = HStack(alignment: .top, spacing: 8) {
            ForEach(0..<4) { column in
                VStack(spacing: 8) {
                    ForEach(fixture["actions"].array.indices, id: \.self) { index in
                        let item = fixture["actions"][index]
                        let command = item["command"].replacing("selected", with: JSON(column % 2 == 1))
                            .replacing("enabled", with: JSON(column < 2))
                        ToolActionControl(command: command, checkable: item["checkable"].bool, textSize: fixture["text_size"].number) {}
                            .background(GeometryReader { proxy in
                                Color.clear.preference(key: ActionFrames.self,
                                    value: ["\(column)-\(index)": proxy.frame(in: .named("capture"))])
                            })
                    }
                }.frame(width: fixture["column_width"].number)
            }
        }.padding(6).frame(width: width, height: height, alignment: .topLeading)
            .coordinateSpace(name: "capture").onPreferenceChange(ActionFrames.self) { geometry.frames = $0 }
            .environment(\.colorScheme, fixture["theme"].string == "dark" ? .dark : .light)
            .environment(\.controlActiveState, .active)
            .font(.system(size: fixture["text_size"].number)).foregroundStyle(palette["text"])
            .tint(palette.accent).background(palette["panel"])
        let host = NSHostingView(rootView: content); window.contentView = host
        for _ in 0..<30 { host.layoutSubtreeIfNeeded(); try await Task.sleep(for: .milliseconds(5)) }
        guard geometry.frames.count == fixture["actions"].array.count * 4,
            let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else {
            throw HostFailure(message: "Missing tool-action geometry or capture")
        }
        window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
        let output = URL(fileURLWithPath: CommandLine.arguments[2])
        try bitmap.representation(using: .png, properties: [:])!.write(to: output)
        try JSON(["scale": Double(bitmap.pixelsWide) / width,
            "frames": geometry.frames.mapValues { ["x": $0.minX, "y": $0.minY, "width": $0.width, "height": $0.height] }])
            .encoded().write(to: output.deletingPathExtension().appendingPathExtension("json"), atomically: true, encoding: .utf8)
        print("Captured \(geometry.frames.count) real tool action controls")
    }
}
