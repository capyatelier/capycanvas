// Real Apple header views on deterministic backgrounds, without a Metal view,
// artist storage, visible windows or system menu automation. Each platform has
// its own temporary workspace library with the three shared default identities.
import AppKit
import SwiftUI

@MainActor private final class HeaderBattery: BatterySource {
    func start(_ changed: @escaping (DeviceBattery?) -> Void) { changed(DeviceBattery(level: 0.85, charging: false)) }
    func stop() {}
}
@MainActor private final class HeaderGeometry { var frames: [String: CGRect] = [:] }

@main struct HeaderControlCaptures {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        precondition(NSImage(named: "icon-settings") != nil, "Set CAPY_TEST_ASSETS_APP to a built app")
        let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_HEADER_CAPTURES"]
            ?? "artifacts/apple-header-controls", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var fixtures: [JSON] = []
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-header-controls-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            let library = store.workspaceLibrary!
            let status = SystemStatus(source: HeaderBattery(), now: { Date(timeIntervalSince1970: 1_700_000_000) })
            // Invisible AppKit windows do not activate scene subscriptions.
            // Feed the real status view through its ordinary observable model.
            let subscription = status.acquire()
            defer { status.release(subscription) }
            let deadline = Date().addingTimeInterval(20)
            while store.state.isNull || store.catalog.isNull || !library.ready {
                if let error = library.error { throw HostFailure(message: error) }
                guard Date() < deadline else { throw HostFailure(message: "Header owner startup timed out") }
                try await Task.sleep(for: .milliseconds(5))
            }
            func action(_ value: [String: Any]) async throws {
                try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                    store.edit(value) { error in
                        if let error { continuation.resume(throwing: HostFailure(message: error)) }
                        else { continuation.resume() }
                    }
                }
            }
            for workspace in library.status["default_workspaces"].array {
                try await store.workspaceManager.run(JSON(["type": "switch", "value": workspace["id"].raw]))
                for width: CGFloat in [744, 1200] {
                    store.native?.resize(width: UInt32(width), height: 870, scale: 1)
                    for theme in ["light", "dark"] {
                        try await action(["type": "set_theme", "theme": theme])
                        for clock in [false, true] {
                            try await action(["type": "preferences", "action": ["type": "edit", "id": "show_clock", "value": clock ? 1 : 2]])
                            for paper in [false, true] {
                                let palette = EditorPalette(source: store.state["palette"])
                                let geometry = HeaderGeometry()
                                store.headerLeadingInset = platform == 1 ? 76 : 0
                                let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: 48),
                                    styleMask: [.borderless], backing: .buffered, defer: false)
                                window.isReleasedWhenClosed = false
                                defer { window.contentView = nil; window.close() }
                                window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
                                let content = ZStack(alignment: .topLeading) {
                                    paper ? Color.white : palette["bg"]
                                    EditorHeader(store: store, showsApplicationMenus: platform == 0, status: status)
                                    EditorZenButton(store: store)
                                }.frame(width: width, height: 48).coordinateSpace(name: "editor-workspace")
                                    .environment(\.colorScheme, theme == "dark" ? .dark : .light)
                                    .environment(\.measureHeaderControls, true)
                                    .onPreferenceChange(HeaderControlFrames.self) { geometry.frames = $0 }
                                    .font(.system(size: store.catalog["text_size_pt"].number * 4 / 3))
                                    .foregroundStyle(palette["text"]).tint(palette.accent)
                                let host = NSHostingView(rootView: content)
                                window.contentView = host
                                for _ in 0..<40 {
                                    host.layoutSubtreeIfNeeded()
                                    try await Task.sleep(for: .milliseconds(5))
                                }
                                precondition(store.failure == nil, store.failure ?? "")
                                precondition(!clock || status.battery?.percent == 85)
                                guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { throw HostFailure(message: "No header bitmap") }
                                window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                                let name = "\(platform)-\(workspace["name"].string.lowercased())-\(Int(width))-\(theme)-\(clock ? "clock" : "no-clock")-\(paper ? "paper" : "surround")"
                                try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("native-\(name).png"))
                                let elements = geometry.frames.mapValues { JSON($0).raw }
                                precondition(elements["settings-button"] != nil && elements["zen-button"] != nil)
                                precondition((elements["system-clock"] != nil) == clock && (elements["system-battery"] != nil) == clock)
                                precondition((elements["document-title"] != nil) == (platform == 1 || width > 850))
                                let menuCount = elements.keys.filter { $0.hasPrefix("menu-") }.count
                                precondition(platform == 0 ? (menuCount == store.snapshot["application_menus"].array.count ||
                                    menuCount == 0 && elements["application-menus"] != nil) : menuCount == 0)
                                precondition(elements["workspace-switcher"] != nil && elements.keys.filter { $0.hasPrefix("workspace-switch-") }.count == 3)
                                fixtures.append(JSON(["name": name, "platform": platform, "viewport": [width, 870],
                                    "scale": Double(bitmap.pixelsWide) / width, "clip": ["x": 0, "y": 0, "width": width, "height": 48],
                                    "theme": theme, "clock_visible": clock, "clock": status.time, "battery_percent": 85,
                                    "active_workspace": workspace["id"].raw,
                                    "surface": paper ? "#ffffff" : store.state["palette"]["bg"].string,
                                    "header_leading_inset": store.headerLeadingInset, "workspace": store.state["workspace"].raw,
                                    "title": store.state["tabs"][0].raw, "elements": elements]))
                            }
                        }
                    }
                }
            }
            try await library.close()
            print("PASS: platform \(platform), 48 real header captures with three task workspaces, fixed clock/battery, narrow/wide geometry and two backgrounds/themes")
        }
        try JSON(["schema": 1, "scope": "Complete shared header components; no Metal or UIKit pixels", "fixtures": fixtures.map(\.raw)])
            .encoded().write(to: directory.appendingPathComponent("fixtures.json"), atomically: true, encoding: .utf8)
    }
}
