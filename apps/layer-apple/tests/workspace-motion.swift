// Real native workspace placement, hit preferences and retained Navigator
// identity on both Apple presets. The AppKit host stays invisible and isolated.
import AppKit
import SwiftUI

@MainActor private final class MotionGeometry {
    var navigators: [UUID: NavigatorPlacement] = [:]
}

@main struct WorkspaceMotionChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        for platform: UInt32 in [0, 1] { try await check(platform) }
    }
    @MainActor private static func check(_ platform: UInt32) async throws {
        let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        let geometry = MotionGeometry()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 870),
                              styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func settle() async throws {
            for _ in 0..<20 {
                window.contentView?.layoutSubtreeIfNeeded()
                try await Task.sleep(for: .milliseconds(5))
            }
        }
        func wait(_ message: String, _ condition: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(10)
            while !condition() {
                window.contentView?.layoutSubtreeIfNeeded()
                guard Date() < deadline else { throw NSError(domain: message, code: 1) }
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
        func group() -> JSON {
            store.snapshot["layout"]["groups"].array.first { $0["panels"].array.contains { $0.string == "navigator" } } ?? JSON()
        }
        try await wait("Initial state") { !store.state.isNull }
        store.native?.resize(width: 1200, height: 870, scale: 1)
        try await action(["type": "invoke", "command": "fit_canvas"])
        if group()["active"].string != "navigator" {
            try await action(["type": "select_panel_tab", "group": group()["id"].raw, "panel": "navigator"])
        }
        let root = WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 870).coordinateSpace(name: "editor-workspace")
            .font(.system(size: 44 / 3))
            .onPreferenceChange(NavigatorPlacements.self) { geometry.navigators = $0 }
            .onPreferenceChange(WorkspaceTabFrames.self) { store.workspace.tabFrames = $0 }
            .onPreferenceChange(WorkspaceTabs.self) { bounds in
                store.workspace.tabs = bounds.compactMap { key, rect in
                    let parts = key.split(separator: ":").compactMap { UInt64($0) }
                    guard parts.count == 2 else { return nil }
                    return JSON(["group": parts[0], "index": parts[1], "bounds": JSON(rect).raw])
                }
            }
        window.contentView = NSHostingView(rootView: root)
        try await wait("Initial Navigator allocation") { !geometry.navigators.isEmpty }
        try await settle()
        func close(_ a: CGRect, _ b: CGRect) -> Bool {
            abs(a.minX - b.minX) < 0.02 && abs(a.minY - b.minY) < 0.02
                && abs(a.width - b.width) < 0.02 && abs(a.height - b.height) < 0.02
        }
        func checkResizes(floating: Bool) async throws {
            for cancel in [true, false] {
                try await settle()
                let saved = store.state["workspace"].stableKey
                let bounds = group()["bounds"].rect, id = group()["id"].uint
                let divider = store.snapshot["layout"]["dividers"].array.first {
                    let d = $0["bounds"].rect
                    return d.width <= 6 && d.height > 100 && d.minY <= bounds.midY && d.maxY >= bounds.midY
                        && min(abs(d.midX - bounds.minX), abs(d.midX - bounds.maxX)) <= 6
                } ?? JSON()
                precondition(floating || !divider.isNull, "Navigator must have an adjacent vertical divider")
                let item = floating ? JSON(["type": "resize_floating", "group": id, "edge": "right"])
                    : JSON(["type": "drag_divider", "id": divider["id"].raw])
                let start = CGPoint(x: floating ? bounds.maxX : divider["bounds"].rect.midX, y: bounds.midY)
                let direction: CGFloat = floating || start.x > bounds.midX ? 1 : -1
                guard let identity = geometry.navigators.keys.first else { fatalError("Navigator missing") }
                store.workspace.start(item, point: start)
                try await settle()
                let content = store.workspaceMotion.contentRevision
                let revision = store.state["revision"].uint
                let commands = store.state["commands"].stableKey
                let panels = store.snapshot["panels"].stableKey
                let menus = store.snapshot["application_menus"].stableKey
                precondition(!store.state["commands"].array.isEmpty && !store.snapshot["panels"].array.isEmpty
                    && !store.snapshot["application_menus"].array.isEmpty)
                for index in 1...16 {
                    let delta = CGFloat(index * 2)
                    let previousModel = store.workspaceMotion.modelRevision
                    store.workspace.move(item, point: CGPoint(x: start.x + delta * direction, y: start.y))
                    try await wait("Resize geometry \(platform)/\(floating)/\(cancel)/\(index)") {
                        let g = group(), b = g["bounds"].rect
                        guard abs(b.width - bounds.width - delta) < 0.02,
                            geometry.navigators.count == 1, let n = geometry.navigators[identity],
                            abs(n.bounds.width - (b.width - 16)) < 0.02,
                            abs(n.bounds.minX - b.minX - 8) < 0.02,
                            close(n.clip, n.bounds.intersection(b)),
                            store.workspaceMotion.modelRevision != previousModel else { return false }
                        let expectedHandle = floating ? g["resize_handles"].array.first { $0["edge"].string == "right" }?["bounds"] ?? JSON()
                            : store.snapshot["layout"]["dividers"].array.first { $0["id"].stableKey == divider["id"].stableKey }?["bounds"] ?? JSON()
                        guard let handle = store.workspace.sources[item.stableKey], close(handle.bounds, expectedHandle.rect) else { return false }
                        let tabs = store.workspace.tabFrames.filter { $0.key.hasPrefix("\(id):") }
                        let clip = CGRect(x: b.minX, y: b.minY, width: b.width - 20, height: 36)
                        return !tabs.isEmpty && tabs.values.allSatisfy { close($0.clip, clip) }
                    }
                    precondition(store.workspaceMotion.contentRevision == content && store.state["revision"].uint == revision,
                        "Live resizing must retain the content revision")
                    precondition(store.state["commands"].stableKey == commands && store.snapshot["panels"].stableKey == panels
                        && store.snapshot["application_menus"].stableKey == menus, "Live resizing must retain control models")
                }
                if cancel {
                    store.workspace.cancel(item)
                    try await wait("Resize cancellation restores workspace") { store.state["workspace"].stableKey == saved }
                } else {
                    store.workspace.move(item, point: CGPoint(x: start.x + 36 * direction, y: start.y), released: true)
                    try await wait("Resize release publishes final geometry and content") {
                        abs(group()["bounds"].rect.width - bounds.width - 36) < 0.02
                            && store.workspaceMotion.contentRevision != content
                    }
                    let committed = store.state["workspace"].stableKey
                    try await action(["type": "invoke", "command": "undo_workspace"])
                    precondition(store.state["workspace"].stableKey == saved, "One Undo restores the complete resize")
                    try await action(["type": "invoke", "command": "redo_workspace"])
                    precondition(store.state["workspace"].stableKey == committed)
                    try await action(["type": "invoke", "command": "undo_workspace"])
                }
                try await settle()
                precondition(geometry.navigators.count == 1 && geometry.navigators[identity] != nil,
                    "Resize, cancel, release and history must retain the Navigator allocation")
            }
        }
        try await checkResizes(floating: false)
        let docked = store.state["workspace"].stableKey
        let panel = JSON(["kind": "panel", "panel": "navigator"])
        let press = CGPoint(x: group()["bounds"].rect.minX + 15, y: group()["bounds"].rect.minY + 18)
        store.workspace.start(panel, point: press)
        store.workspace.move(panel, point: CGPoint(x: 620, y: 360))
        try await wait("Navigator tear-off publishes new models") { group()["floating"].bool }
        try await settle()

        func checkMoves(_ item: JSON, from start: CGPoint) async throws {
            let id = group()["id"].uint
            let retained = store.snapshot["layout"].stableKey
            let modelRevision = store.workspaceMotion.modelRevision
            let camera = store.camera.value.stableKey
            let baseline = store.workspaceMotion.position(id).rect
            let sources = store.workspace.sources.filter { key, _ in
                guard let source = try? JSON.decode(key) else { return false }
                return source["group"].uint == id && !source["group"].isNull || source["panel"].string == "navigator"
            }
            let tabs = store.workspace.tabFrames.filter { $0.key.hasPrefix("\(id):") }
            precondition(!sources.isEmpty && !tabs.isEmpty)
            precondition(sources.keys.contains { (try? JSON.decode($0)["type"].string) == "resize_floating" })
            guard let (identity, navigator) = geometry.navigators.first else { fatalError("Missing floating Navigator") }
            func translated(_ actual: CGRect, _ original: CGRect, _ dx: CGFloat, _ dy: CGFloat) -> Bool {
                let expected = original.offsetBy(dx: dx, dy: dy)
                return abs(actual.minX - expected.minX) < 0.01 && abs(actual.minY - expected.minY) < 0.01
                    && abs(actual.width - expected.width) < 0.01 && abs(actual.height - expected.height) < 0.01
            }
            for index in 1...12 {
                let dx = CGFloat(index * 2), dy = CGFloat(index)
                store.workspace.move(item, point: CGPoint(x: start.x + dx, y: start.y + dy))
                try await wait("Native hit, clip and Navigator geometry must follow move \(index)") {
                    guard translated(store.workspaceMotion.position(id).rect, baseline, dx, dy),
                          let next = geometry.navigators[identity], translated(next.bounds, navigator.bounds, dx, dy),
                          translated(next.clip, navigator.clip, dx, dy), translated(next.image, navigator.image, dx, dy) else { return false }
                    return sources.allSatisfy { key, source in
                        guard let moved = store.workspace.sources[key] else { return false }
                        return moved.layer == source.layer && translated(moved.bounds, source.bounds, dx, dy)
                    } && tabs.allSatisfy { key, frame in
                        guard let moved = store.workspace.tabFrames[key] else { return false }
                        return translated(moved.bounds, frame.bounds, dx, dy) && translated(moved.clip, frame.clip, dx, dy)
                    }
                }
                precondition(store.snapshot["layout"].stableKey == retained && store.workspaceMotion.modelRevision == modelRevision,
                             "Ordinary placement must retain the panel models")
                precondition(store.camera.value.stableKey == camera, "Camera-less motion must preserve the readout")
            }
        }
        try await checkMoves(panel, from: CGPoint(x: 620, y: 360))
        store.workspace.cancel(panel)
        try await wait("Cancellation restores the dock") { store.state["workspace"].stableKey == docked && store.workspaceMotion.position(group()["id"].uint).isNull }
        try await settle()
        store.workspace.start(panel, point: press)
        store.workspace.move(panel, point: CGPoint(x: 620, y: 360), released: true)
        try await wait("Committed floating panel") { group()["floating"].bool && store.workspaceMotion.tab.isNull }
        try await settle()
        try await checkResizes(floating: true)
        let floated = store.state["workspace"].stableKey
        let item = JSON(["kind": "group", "group": group()["id"].raw])
        let bounds = group()["bounds"].rect
        let start = CGPoint(x: bounds.maxX - 10, y: bounds.minY + 18)
        store.workspace.start(item, point: start)
        try await wait("Floating group starts its own presentation") { !store.workspaceMotion.position(group()["id"].uint).isNull }
        try await settle()
        try await checkMoves(item, from: start)
        // A newer release position still commits through the shared action.
        store.workspace.move(item, point: CGPoint(x: start.x + 30, y: start.y + 15), released: true)
        try await wait("Group release publishes committed geometry") { store.state["workspace"].stableKey != floated && store.workspaceMotion.position(group()["id"].uint).isNull }
        let after = store.state["workspace"].stableKey
        try await action(["type": "invoke", "command": "undo_workspace"])
        precondition(store.state["workspace"].stableKey == floated)
        try await action(["type": "invoke", "command": "redo_workspace"])
        precondition(store.state["workspace"].stableKey == after)
        let zoom = store.camera.value["zoom"].number
        try await action(["type": "invoke", "command": "zoom_in"])
        precondition(store.camera.value["zoom"].number > zoom)
        print("PASS: platform \(platform), 24 floating moves and 64 divider/floating resize moves retain content and Navigator identity; hits, tab clips, resize handles and overview follow; cancel/release/history/camera action pass")
    }
}
