import AppKit
import SwiftUI

@MainActor private final class PanelGeometry {
    var navigators: [UUID: NavigatorPlacement] = [:]
}

@main struct PanelMeasurementChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        for platform: UInt32 in [0, 1] { try await check(platform) }
    }
    @MainActor static func check(_ platform: UInt32) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        let geometry = PanelGeometry()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 900),
                              styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func wait(_ label: String, _ predicate: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(15)
            while !predicate() {
                window.contentView?.layoutSubtreeIfNeeded()
                if let error = store.failure { throw NSError(domain: error, code: 1) }
                guard Date() < deadline else {
                    FileHandle.standardError.write(Data("Measurements: \(store.snapshot["panel_measurements"].stableKey)\nGroups: \(store.snapshot["layout"]["groups"].stableKey)\nNavigators: \(geometry.navigators.count)\n".utf8))
                    throw NSError(domain: label, code: 1)
                }
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        func action(_ value: [String: Any]) async throws {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                store.edit(value) { error in
                    if let error { continuation.resume(throwing: NSError(domain: error, code: 1)) }
                    else { continuation.resume() }
                }
            }
        }
        func group(_ panel: String) -> JSON {
            store.snapshot["layout"]["groups"].array.first { $0["panels"].array.contains { $0.string == panel } } ?? JSON()
        }
        func measured(_ panel: String) -> JSON {
            store.snapshot["panel_measurements"].array.first { $0["panel"].string == panel } ?? JSON()
        }
        func fitted(_ panel: String) -> Bool {
            let g = group(panel), height = measured(panel)["content_height"].number
            return g["floating"].bool && height > 0 && abs(g["bounds"].rect.height - min(height + 36, 675)) < 0.02
        }
        func float(_ panel: String, x: Double) async throws {
            try await action(["type": "move_panel", "panel": panel, "target": ["kind": "float", "position": [x, 180]], "viewport": [1200, 900]])
            try await wait("\(panel) fits measured content") { fitted(panel) }
        }
        func settle() async throws {
            var previous = store.snapshot["panel_measurements"].stableKey
            var stableSince = Date()
            try await wait("Native measurements settle") {
                let next = store.snapshot["panel_measurements"].stableKey
                if next != previous { previous = next; stableSince = Date() }
                return Date().timeIntervalSince(stableSince) >= 0.3
            }
        }
        try await wait("Initial editor") { !store.state.isNull && !store.catalog.isNull }
        store.native?.resize(width: 1200, height: 900, scale: 1)
        for panel in ["navigator", "sizes"] where group(panel)["active"].string != panel {
            try await action(["type": "select_panel_tab", "group": group(panel)["id"].raw, "panel": panel])
        }
        window.contentView = NSHostingView(rootView: WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 900).coordinateSpace(name: "editor-workspace")
            .font(.system(size: 44 / 3))
            .onPreferenceChange(NavigatorPlacements.self) { geometry.navigators = $0 })
        try await wait("Native labels and bodies") {
            store.snapshot["panel_measurements"].array.count == store.snapshot["panels"].array.count
                && measured("sizes")["content_height"].number > 20 && geometry.navigators.count == 1
        }
        precondition(store.snapshot["panel_measurements"].array.allSatisfy { $0["tab_width"].number >= 36 })
        try await float("sizes", x: 320)
        let sizes = measured("sizes")["content_height"].number
        try await action(["type": "customize", "action": ["type": "set_control_visible", "panel": "sizes", "control": "size_presets", "visible": false]])
        try await wait("Hidden presets shrink the actual floating body") { fitted("sizes") && measured("sizes")["content_height"].number < sizes - 50 }
        try await action(["type": "invoke", "command": "undo_workspace"])
        try await wait("Workspace Undo restores natural sizing") { fitted("sizes") && abs(measured("sizes")["content_height"].number - sizes) < 0.02 }

        let originalBounds = group("sizes")["bounds"].rect
        for phase in ["down", "move", "up"] {
            try await action(["type": "resize_floating", "group": group("sizes")["id"].raw, "edge": "right", "phase": phase,
                "position": [phase == "down" ? originalBounds.maxX : originalBounds.minX + 140, originalBounds.midY], "viewport": [1200, 900]])
        }
        try await wait("Narrow body reflows while preserving the manually sized height") {
            abs(group("sizes")["bounds"].rect.width - 140) < 0.02
                && abs(group("sizes")["bounds"].rect.height - originalBounds.height) < 0.02
                && measured("sizes")["content_height"].number > sizes + 50
        }
        try await action(["type": "invoke", "command": "undo_workspace"])
        try await wait("Undo resize restores the natural body and height") { fitted("sizes") && abs(measured("sizes")["content_height"].number - sizes) < 0.02 }

        try await float("layers", x: 640)
        let layers = measured("layers")["content_height"].number
        try await action(["type": "invoke", "command": "add_layer"])
        try await wait("New layer grows intrinsic content") { fitted("layers") && measured("layers")["content_height"].number > layers + 20 }
        try await float("navigator", x: 880)
        precondition(abs(measured("navigator")["content_height"].number - 214) < 0.02)
        precondition(geometry.navigators.count == 1, "Measuring tabs must not mount extra GPU Navigator views")

        try await action(["type": "move_panel", "panel": "stats", "target": ["kind": "tab", "group": group("sizes")["id"].raw], "viewport": [1200, 900]])
        try await wait("Tabbed floating group fits both native labels") {
            let width = measured("sizes")["tab_width"].number + measured("stats")["tab_width"].number + 22
            return abs(group("sizes")["bounds"].rect.width - width) < 0.02 && fitted("stats")
        }

        // Let newly mounted bodies finish layout before saving expected facts.
        try await settle()
        let before = store.state["workspace"].stableKey
        let measurements = store.snapshot["panel_measurements"].stableKey
        try await action(["type": "measure_panels", "measurements": []])
        try await wait("Transient measurements are republished without a widget resize") {
            store.snapshot["panel_measurements"].stableKey == measurements
        }
        precondition(store.state["workspace"].stableKey == before, "Measurements must not change saved workspace/history")
        let revision = store.state["revision"].uint
        for _ in 0..<40 {
            window.contentView?.layoutSubtreeIfNeeded()
            try await Task.sleep(for: .milliseconds(5))
        }
        precondition(store.state["revision"].uint == revision, "Settled measurements must not create a publication loop")
        print("PASS: platform \(platform), intrinsic label/body measurement, floating/tab fit, width reflow, control visibility/history, growing layer content, one Navigator, transient revalidation and settled publication")
    }
}
