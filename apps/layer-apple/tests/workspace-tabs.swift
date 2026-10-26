// Real shared SwiftUI measurements and Rust drag actions on both Apple presets.
// No visible window, native menu automation, artist persistence or GPU is used.
import AppKit
import SwiftUI

@MainActor private final class RetirementSources { var values: [String: WorkspaceSource] = [:] }

@main struct WorkspaceTabChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        for theme in ["light", "dark"] {
            for (stripAlpha, panelAlpha) in [(1.0, 1.0), (0.55, 0.55), (0.3, 0.55)] {
                for offset in [0.0, 0.25, 0.5, 0.75] {
                    try await contour(theme: theme, stripAlpha: stripAlpha, panelAlpha: panelAlpha, offset: offset)
                }
            }
        }
        for platform: UInt32 in [0, 1] {
            for theme in ["light", "dark"] {
                for collapsed in [false, true] { try await check(platform, collapsed: collapsed, theme: theme) }
            }
        }
    }
    @MainActor private static func contour(theme: String, stripAlpha: Double, panelAlpha: Double, offset: Double) async throws {
        let stripShade = theme == "light" ? 0.64 : 0.15, panelShade = theme == "light" ? 0.82 : 0.31
        let strip = Color(.sRGB, red: stripShade, green: stripShade, blue: stripShade, opacity: stripAlpha)
        let panel = Color(.sRGB, red: panelShade, green: panelShade, blue: panelShade, opacity: panelAlpha)
        let bounds = CGRect(x: 54 + offset, y: 0, width: 80.5, height: 36)
        let tab = WorkspaceTabFrame(bounds: bounds, clip: CGRect(x: 0, y: 0, width: 200, height: 36))
        let host = NSHostingView(rootView: VStack(spacing: 0) {
            WorkspaceTabStrip(shape: .surface, tab: tab, strip: strip, panel: panel,
                slide: WorkspaceTabSlide(motion: WorkspaceMotion()), group: 0, active: "toolbar").frame(width: 200, height: 36)
            HStack(spacing: 0) { strip; panel }.frame(width: 200, height: 12)
        }.coordinateSpace(name: "editor-workspace")
            .background(Color(.sRGB, red: 0.95, green: 0.05, blue: 0.75, opacity: 1)))
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 200, height: 48),
                              styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        window.contentView = host
        for _ in 0..<40 {
            host.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(5))
        }
        let bitmap = host.bitmapImageRepForCachingDisplay(in: host.bounds)!
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let sx = Double(bitmap.pixelsWide) / 200, sy = Double(bitmap.pixelsHigh) / 48
        func rgb(_ x: Int, _ y: Int) -> [Double] {
            let color = bitmap.colorAt(x: x, y: y)!.usingColorSpace(.sRGB)!
            return [Double(color.redComponent), Double(color.greenComponent), Double(color.blueComponent)]
        }
        let a = rgb(Int(50 * sx), Int(42 * sy)), b = rgb(Int(150 * sx), Int(42 * sy))
        let delta = zip(a, b).map { $1 - $0 }, length = delta.reduce(0) { $0 + $1 * $1 }
        precondition(length > 0.01, "The contour fixture needs distinct panel and strip colors")
        for (point, expected) in [(CGPoint(x: bounds.midX, y: 18), b),
                                  (CGPoint(x: bounds.minX + 1, y: 1), a),
                                  (CGPoint(x: bounds.minX, y: 35.5), b)] {
            let actual = rgb(Int(point.x * sx), Int(point.y * sy))
            precondition(zip(actual, expected).allSatisfy { abs($0 - $1) <= 2.5 / 255 },
                "The selected body, excluded upper corner and concave foot must retain their surface roles: \(point), \(actual), expected \(expected)")
        }
        var upper = 0, feet = 0
        for y in 0..<Int(36 * sy) {
            let py = (Double(y) + 0.5) / sy
            let left: Double
            if py < 18 {
                left = bounds.minX + 18 * (1 - pow(1 - pow((18 - py) / 18, 4), 0.25))
            } else if py > 30 {
                left = bounds.minX - 6 + 6 * pow(1 - pow((py - 30) / 6, 4), 0.25)
            } else { left = bounds.minX }
            let right = bounds.maxX + bounds.minX - left
            for x in 0..<bitmap.pixelsWide {
                let px = (Double(x) + 0.5) / sx
                guard min(abs(px - left), abs(px - right)) <= 1.5 / sx else { continue }
                let actual = rgb(x, y), relative = zip(actual, a).map { $0 - $1 }
                let projection = min(1, max(0, zip(relative, delta).reduce(0) { $0 + $1.0 * $1.1 } / length))
                let expected = zip(a, delta).map { $0 + projection * $1 }
                precondition(zip(actual, expected).allSatisfy { abs($0 - $1) <= 2.5 / 255 },
                    "A tab contour must contain exactly one surface: \(theme), alpha \(stripAlpha)/\(panelAlpha), offset \(offset), pixel \(x),\(y): \(actual), role segment \(a)...\(b)")
                if py < 18 { upper += 1 }
                if py > 30 { feet += 1 }
            }
        }
        precondition(upper > 50 && feet > 15, "Both upper squircles and concave lower feet must be sampled")
        print("PASS: \(theme), alpha \(stripAlpha)/\(panelAlpha), offset \(offset): absolute tab contour colors, \(upper) upper and \(feet) foot pixels")
    }
    @MainActor private static func check(_ platform: UInt32, collapsed: Bool, theme: String) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 870),
                              styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func wait(_ message: String, seconds: TimeInterval = 10, _ condition: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(seconds)
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
        try await wait("Initial state") { !store.state.isNull }
        try await store.apply(["type": "set_theme", "theme": theme])
        store.native?.resize(width: 1200, height: 870, scale: 1)
        try await store.apply(["type": "invoke", "command": "fit_canvas"])
        guard let groupModel = store.snapshot["layout"]["groups"].array.first(where: {
            $0["panels"].array.contains { $0.string == "brushes" }
        }) else {
            throw NSError(domain: "Missing fixture's brush group", code: 1)
        }
        let group = groupModel["id"].uint
        for panel in ["toolbar", "navigator"] {
            try await store.apply(["type": "move_panel", "panel": panel,
                              "target": ["kind": "tab", "group": group], "viewport": [1200,870]])
        }
        try await store.apply(["type": "customize", "action": ["type": "set_tab_style", "group": group, "style": "icon_name"]])
        // A user-sized column deliberately clips the final tab in both hosts.
        let divider = store.snapshot["layout"]["dividers"].array.first { $0["band"].bool && !$0["reversed"].bool && $0["axis"].string == "horizontal" }!
        try await store.apply(["type": "nudge_divider", "id": divider["id"].raw, "forward": false, "viewport": [1200, 870]])
        if collapsed {
            try await store.apply(["type": "customize", "action": ["type": "set_column_collapsed", "group": group, "collapsed": true]])
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
            try await store.apply(["type": "customize", "action": ["type": "toggle_column_drawer", "group": group, "panel": "toolbar"]])
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
        try await store.apply(["type": "invoke", "command": "undo_workspace"])
        precondition(store.state["workspace"].stableKey == before)
        try await store.apply(["type": "invoke", "command": "redo_workspace"])
        precondition(store.state["workspace"].stableKey == after)
        // Cancellation and a new drag can overlap owner acknowledgements. An
        // old completion must not erase the newer gesture's frozen geometry.
        try await store.apply(["type": "invoke", "command": "undo_workspace"])
        if collapsed && !store.snapshot["layout"]["groups"].array.contains(where: { $0["id"].uint == group }) {
            try await store.apply(["type": "customize", "action": ["type": "toggle_column_drawer", "group": group, "panel": "toolbar"]])
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
        let liveGroups = store.snapshot["layout"]["groups"].array
            + store.state["customization"]["column_drawers"].array.map { $0["tabs"].replacing("id", with: $0["tabs"]["group"]) }
        let previousActive = liveGroups.first { $0["id"].uint == group }!["active"].string
        let nextActive = previousActive == "toolbar" ? "brushes" : "toolbar"
        let comparisonActive = nextActive == "toolbar" ? "brushes" : "toolbar"
        if previousActive != comparisonActive {
            try await store.apply(["type": "select_panel_tab", "group": group, "panel": comparisonActive])
        }
        try await wait("Late-selection comparison panel") {
            let groups = store.snapshot["layout"]["groups"].array
                + store.state["customization"]["column_drawers"].array.map { $0["tabs"].replacing("id", with: $0["tabs"]["group"]) }
            return groups.first { $0["id"].uint == group }?["active"].string == comparisonActive
        }
        store.workspace.start(item, point: restartPress)
        store.workspace.move(item, point: restartBeyond)
        try await wait("Preview before late selection") { !store.workspace.tabSlide.preview.isNull }
        func previewShade() async throws -> [CGFloat] {
            for _ in 0..<35 {
                window.contentView?.layoutSubtreeIfNeeded()
                try await Task.sleep(for: .milliseconds(5))
            }
            let grab = store.workspace.tabSlide.grab!, frame = grab.frames[grab.source].bounds
            let x = store.workspace.tabSlide.preview["bounds"]["x"].number + frame.width / 2
            let area = CGRect(x: x, y: frame.maxY - 2, width: 1, height: 1)
            let host = window.contentView!, bitmap = host.bitmapImageRepForCachingDisplay(in: area)!
            host.cacheDisplay(in: area, to: bitmap)
            let color = bitmap.colorAt(x: bitmap.pixelsWide / 2, y: bitmap.pixelsHigh / 2)!.usingColorSpace(.deviceRGB)!
            return [color.redComponent, color.greenComponent, color.blueComponent]
        }
        let previousShade = try await previewShade(), previewID = store.workspace.tabSlide.grab!.id
        try await store.apply(["type": "select_panel_tab", "group": group, "panel": nextActive])
        let selectedShade = try await previewShade()
        precondition(zip(previousShade, selectedShade).contains { abs($0 - $1) > 1 / 255 },
            "A late selection publication must update the retained preview's selected surface")
        precondition(store.workspace.tabSlide.grab!.id == previewID && store.workspace.tabSlide.grab!.frames == restarted)
        store.workspace.cancel(item)
        try await wait("Late-selection preview retires") { store.workspace.tabSlide.grab == nil }
        try await store.apply(["type": "select_panel_tab", "group": group, "panel": previousActive])
        precondition(store.state["workspace"].stableKey == before)
        try await store.apply(["type": "customize", "action": ["type": "set_tab_style", "group": group, "style": "icon"]])
        try await wait("Shared icon-only tab widths") { frames().count == 3 && frames().allSatisfy { $0.bounds.width == 36 } }
        print("PASS: platform \(platform), \(theme) \(collapsed ? "drawer" : "docked") tabs: clipped natural widths, frozen hits, reversal, release, undo/redo, cancellation/restart, live preview selection, 36-point icon tabs")
        let arrangement = store.state["workspace"]
        for panel in store.snapshot["panels"].array {
            try await store.apply(["type": "customize", "action": ["type": "set_panel_visible", "panel": panel["id"].raw, "visible": false]])
        }
        let replacement = store.state["workspace"]
        try require(store.snapshot["layout"]["groups"].array.isEmpty && store.snapshot["layout"]["collapsed"].array.isEmpty,
            "The shared visibility actions must produce an empty replacement layout")
        func retire<Root: View>(_ root: Root, kind: String) async throws {
            let sources = RetirementSources()
            func containers() -> [WorkspaceSource] {
                sources.values.values.filter { source in
                    let kind = (try? JSON.decode(source.item))?["kind"].string ?? ""
                    return ["panel", "group", "column"].contains(kind)
                }
            }
            let retained = NSHostingView(rootView: root.frame(width: 1200, height: 870)
                .coordinateSpace(name: "editor-workspace")
                .onPreferenceChange(WorkspaceSources.self) { sources.values = $0 })
            window.contentView = retained
            try await wait("Retained \(kind) content") {
                containers().contains { (try? JSON.decode($0.item))?["kind"].string == kind }
            }
            try await store.apply(["type": "restore_workspace", "workspace": replacement.raw])
            try await wait("Replacement panel index") {
                store.snapshot["layout"]["groups"].array.isEmpty && store.snapshot["layout"]["collapsed"].array.isEmpty
                    && store.snapshot["panels"].array.isEmpty
            }
            try await wait("Retired \(kind) sources", seconds: 5) { containers().isEmpty }
            try require(containers().isEmpty, "A retained lazy body must withdraw its retired container and panel sources")
            print("PASS: platform \(platform), \(theme): retained production \(kind) content retires after workspace replacement")
        }
        try await store.apply(["type": "restore_workspace", "workspace": arrangement.raw])
        if collapsed {
            try require(store.snapshot["layout"]["collapsed"].array.contains { column in
                column["groups"].array.contains { $0["group"].uint == group }
            }, "The column retirement case must mount the fixture's collapsed group")
            try await retire(WorkspaceCollapsedColumns(store: store).body, kind: "column")
        } else {
            try require(store.snapshot["layout"]["groups"].array.contains {
                $0["id"].uint == group && $0["tabs_visible"].bool && $0["panels"].array.count > 1
            }, "The group retirement case must mount the fixture's real tabs")
            try await retire(WorkspacePanels(store: store, workspace: store.workspace).body, kind: "group")
        }
    }
}
