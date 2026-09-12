// Actual shared editor/header views; temporary storage, invisible AppKit
// windows and no system menu automation. These captures do not include Metal.
import AppKit
import SwiftUI

@main struct WorkspaceHeaderChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["CAPY_HEADER_CAPTURES"]
            ?? "artifacts/apple-workspace-header", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        for platform: UInt32 in [0, 1] {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("capy-header-\(UUID())")
            defer { try? FileManager.default.removeItem(at: root) }
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            let library = store.workspaceLibrary!
            let deadline = Date().addingTimeInterval(20)
            while !library.ready {
                if let error = library.error { throw HostFailure(message: error) }
                guard Date() < deadline else { throw HostFailure(message: "Workspace startup timed out") }
                try await Task.sleep(for: .milliseconds(5))
            }
            for workspace in library.status["default_workspaces"].array {
                try await store.workspaceManager.run(JSON(["type": "switch", "value": workspace["id"].raw]))
                for width: CGFloat in [744, 1200] {
                    for dark in [false, true] {
                        let size = CGSize(width: width, height: 800)
                        store.native?.resize(width: UInt32(width), height: 800, scale: 1)
                        store.dispatch(["type": "system_theme_changed", "theme": dark ? "dark" : "light"])
                        let window = NSWindow(contentRect: CGRect(origin: .zero, size: size), styleMask: [.borderless], backing: .buffered, defer: false)
                        window.isReleasedWhenClosed = false
                        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
                        defer { window.contentView = nil; window.close() }
                        // Simulate the measured Mac window-control reservation.
                        store.headerLeadingInset = platform == 1 ? 76 : 0
                        let host = NSHostingView(rootView: EditorView(store: store, showsApplicationMenus: platform == 0) { Color.clear }
                            .environment(\.colorScheme, dark ? .dark : .light).frame(width: size.width, height: size.height))
                        window.contentView = host
                        for _ in 0..<60 {
                            host.layoutSubtreeIfNeeded()
                            try await Task.sleep(for: .milliseconds(5))
                        }
                        if let error = store.failure ?? library.error { throw HostFailure(message: error) }
                        guard let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { throw HostFailure(message: "No editor bitmap") }
                        window.appearance?.performAsCurrentDrawingAppearance { host.cacheDisplay(in: host.bounds, to: bitmap) }
                        let name = "workspace-\(platform)-\(workspace["name"].string.lowercased())-\(Int(width))-\(dark ? "dark" : "light").png"
                        try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent(name))
                    }
                }
            }
            try await library.close()
            print("PASS: platform \(platform), three task workspace editor/header captures at narrow/wide widths and both themes")
        }
    }
}
