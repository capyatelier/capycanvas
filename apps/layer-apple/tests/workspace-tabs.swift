// Real shared SwiftUI measurements and Rust drag actions on both Apple presets.
// No visible window, native menu automation, artist persistence or GPU is used.
import AppKit
import SwiftUI

@main struct WorkspaceTabChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        for platform: UInt32 in [0, 1] {
            for theme in ["light", "dark"] {
                for collapsed in [false, true] { try await check(platform, collapsed: collapsed, theme: theme) }
            }
        }
    }
    @MainActor private static func check(_ platform: UInt32, collapsed: Bool, theme: String) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 870),
                              styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func wait(_ message: String, _ condition: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(10)
            while !condition() {
                window.contentView?.layoutSubtreeIfNeeded()
                guard Date() < deadline else {
                    print("Preview", store.workspace.tabSlide.preview.stableKey)
                    print("Frames", store.workspace.tabFrames)
                    print("Drawers", store.state["customization"]["column_drawers"].stableKey)
                    fflush(stdout)
                    throw NSError(domain: message, code: 1)
                }
                try await Task.sleep(for: .milliseconds(5))
            }
            precondition(store.failure == nil, store.failure ?? "")
        }
        func action(_ value: [String: Any]) async throws {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                store.edit(value) { error in
                    if let error { continuation.resume(throwing: NSError(domain: error, code: 1)) }
                    else { continuation.resume() }
                }
            }
        }
        try await wait("Initial state") { !store.state.isNull }
        try await action(["type": "set_theme", "theme": theme])
        store.native?.resize(width: 1200, height: 870, scale: 1)
        try await action(["type": "invoke", "command": "fit_canvas"])
        guard let groupModel = store.snapshot["layout"]["groups"].array.first(where: {
            $0["panels"].array.contains { $0.string == "brushes" }
        }) else {
            throw NSError(domain: "Missing fixture's brush group", code: 1)
        }
        let group = groupModel["id"].uint
        for panel in ["toolbar", "navigator"] {
            try await action(["type": "move_panel", "panel": panel,
                              "target": ["kind": "tab", "group": group], "viewport": [1200,870]])
        }
        try await action(["type": "customize", "action": ["type": "set_tab_style", "group": group, "style": "icon_name"]])
        // A user-sized column deliberately clips the final tab in both hosts.
        let divider = store.snapshot["layout"]["dividers"].array.first { $0["band"].bool && !$0["reversed"].bool && $0["axis"].string == "horizontal" }!
        try await action(["type": "nudge_divider", "id": divider["id"].raw, "forward": false, "viewport": [1200, 870]])
        if collapsed {
            try await action(["type": "customize", "action": ["type": "set_column_collapsed", "group": group, "collapsed": true]])
        }
        let root = WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 870).coordinateSpace(name: "editor-workspace")
            .font(.system(size: 44 / 3)).foregroundStyle(EditorPalette(source: store.state["palette"])["text"])
            .background(EditorPalette(source: store.state["palette"])["bg"])
            .onPreferenceChange(WorkspaceTabs.self) { bounds in
                store.workspace.tabs = bounds.compactMap { key, rect in
                    let parts = key.split(separator: ":").compactMap { UInt64($0) }
                    guard parts.count == 2 else { return nil }
                    return JSON(["group": parts[0], "index": parts[1], "bounds": JSON(rect).raw])
                }
            }
            .onPreferenceChange(WorkspaceTabFrames.self) { store.workspace.tabFrames = $0 }
        window.contentView = NSHostingView(rootView: root)
        if collapsed {
            try await action(["type": "customize", "action": ["type": "toggle_column_drawer", "group": group, "panel": "toolbar"]])
        }
        func frames() -> [WorkspaceTabFrame] { (0..<3).compactMap { store.workspace.tabFrames["\(group):\($0)"] } }
        var previous: [WorkspaceTabFrame] = [], stableSince = Date()
        try await wait("Complete settled tab frames") {
            let current = frames()
            if current != previous { previous = current; stableSince = Date() }
            return current.count == 3 && Date().timeIntervalSince(stableSince) > 0.2
        }
        let frozen = frames(), before = store.state["workspace"].stableKey
        precondition(frozen.last!.bounds.maxX > frozen[0].clip.maxX, "The fixture must include a clipped natural-width tab")
        let item = JSON(["kind": "panel", "panel": "toolbar"])
        let visible = frozen[1].bounds.intersection(frozen[1].clip)
        let press = CGPoint(x: visible.minX + 3, y: visible.midY)
        precondition(store.workspace.source(at: press)?.stableKey == item.stableKey)
        // The neighbor's halfway point is a distance from the grab, independent
        // of where inside the source tab the user pressed.
        let halfway = frozen[2].bounds.width * 0.5
        let beyond = CGPoint(x: press.x + halfway + 1, y: press.y)
        let captureDirectory = ProcessInfo.processInfo.environment["CAPY_TAB_CAPTURE_DIRECTORY"].map { URL(fileURLWithPath: $0, isDirectory: true) }
        let prefix = "\(platform)-\(collapsed ? "drawer" : "docked")-\(theme)"
        func capture(_ phase: String) throws {
            guard let directory = captureDirectory, let host = window.contentView else { return }
            precondition(NSImage(named: "icon-pen") != nil, "Captures require CAPY_TEST_ASSETS_APP with compiled vector assets")
            precondition(host.isFlipped)
            let strip = frozen[0].clip
            guard let bitmap = host.bitmapImageRepForCachingDisplay(in: strip) else { fatalError("No tab bitmap") }
            host.cacheDisplay(in: strip, to: bitmap)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("native-\(prefix)-\(phase).png"))
            let metadata: [String: Any] = ["schema": 1, "platform": platform, "collapsed": collapsed, "group": group,
                "viewport": [1200, 870], "workspace": try JSON.decode(before).raw, "theme": store.state["theme"].raw,
                "clip": JSON(strip).raw, "frames": frozen.map { JSON($0.bounds).raw },
                "press": [press.x, press.y], "position": [beyond.x, beyond.y],
                "scale": Double(bitmap.pixelsWide) / strip.width, "preview": store.workspace.tabSlide.preview.raw]
            try JSON(metadata).encoded().write(to: directory.appendingPathComponent("native-\(prefix)-\(phase).json"), atomically: true, encoding: .utf8)
        }
        try capture("before")
        store.workspace.start(item, point: press)
        store.workspace.move(item, point: beyond)
        try await wait("Attached source and neighbor preview") {
            store.workspace.tabSlide.preview["insertion"].uint == 3
                && store.workspace.tabSlide.offset(group: group, index: 2) == -frozen[1].bounds.width
        }
        precondition(store.workspace.tabSlide.grab?.frames == frozen)
        for _ in 0..<35 {
            window.contentView?.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(5))
        }
        precondition(frames() == frozen, "Moving tab visuals must not move native hit or switch-point measurements")
        precondition(store.state["workspace"].stableKey == before, "A preview must not create a layout history entry")
        try capture("drag")
        let back = CGPoint(x: press.x + halfway - 1, y: press.y)
        store.workspace.move(item, point: back)
        try await wait("Reversed drag restores the original slot") { store.workspace.tabSlide.preview["insertion"].uint == 1 }
        // Release beyond the switch without delivering a move at that point.
        store.workspace.move(item, point: beyond, released: true)
        try await wait("Release commits and retires its visual") {
            store.workspace.tabSlide.grab == nil && store.state["workspace"].stableKey != before
        }
        let after = store.state["workspace"].stableKey
        try await action(["type": "invoke", "command": "undo_workspace"])
        precondition(store.state["workspace"].stableKey == before)
        try await action(["type": "invoke", "command": "redo_workspace"])
        precondition(store.state["workspace"].stableKey == after)
        // Cancellation and a new drag can overlap owner acknowledgements. An
        // old completion must not erase the newer gesture's frozen geometry.
        try await action(["type": "invoke", "command": "undo_workspace"])
        if collapsed {
            // Workspace history retires transient drawers; reopen for the next gesture.
            try await action(["type": "customize", "action": ["type": "toggle_column_drawer", "group": group, "panel": "toolbar"]])
        }
        previous = []; stableSince = Date()
        try await wait("Restored tab measurements") {
            let current = frames()
            if current != previous { previous = current; stableSince = Date() }
            return current.count == 3 && Date().timeIntervalSince(stableSince) > 0.2
        }
        // Drawer placement can change with its selected content after Undo.
        // A new gesture captures that current placement, not the previous one.
        let restarted = frames(), restartVisible = restarted[1].bounds.intersection(restarted[1].clip)
        let restartPress = CGPoint(x: restartVisible.minX + 3, y: restartVisible.midY)
        let restartBeyond = CGPoint(x: restartPress.x + restarted[2].bounds.width * 0.5 + 1, y: restartPress.y)
        store.workspace.start(item, point: restartPress)
        store.workspace.move(item, point: restartBeyond)
        store.workspace.cancel(item)
        store.workspace.start(item, point: restartPress)
        store.workspace.move(item, point: restartBeyond)
        try await wait("New preview survives the previous cancellation") { store.workspace.tabSlide.preview["insertion"].uint == 3 }
        precondition(store.workspace.tabSlide.grab?.frames == restarted)
        store.workspace.cancel(item)
        try await wait("Cancellation retires the preview") { store.workspace.tabSlide.grab == nil }
        precondition(store.state["workspace"].stableKey == before)
        try await action(["type": "customize", "action": ["type": "set_tab_style", "group": group, "style": "icon"]])
        try await wait("Shared icon-only tab widths") { frames().count == 3 && frames().allSatisfy { $0.bounds.width == 36 } }
        print("PASS: platform \(platform), \(theme) \(collapsed ? "drawer" : "docked") tabs: clipped natural widths, frozen hits, reversal, release, undo/redo, cancellation/restart, 36-point icon tabs")
    }
}
