// Invoke actual NSMenu items against the shared editor. No system menu bar,
// visible windows, artist storage or coordinate automation is involved.
import AppKit

@main struct NativeContextMenuChecks {
    @MainActor static func wait(_ label: String, until ready: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(15)
        while !ready() {
            guard Date() < deadline else { throw HostFailure(message: "Timed out: \(label)") }
            try await Task.sleep(for: .milliseconds(5))
        }
    }
    @MainActor static func query(_ store: EditorStore, _ request: [String: Any]) async -> JSON {
        await withCheckedContinuation { continuation in store.query(request) { continuation.resume(returning: $0) } }
    }
    @MainActor static func invoke(_ title: String, in menu: NSMenu) -> Bool {
        for (index, item) in menu.items.enumerated() {
            if item.title == title && item.submenu == nil {
                menu.performActionForItem(at: index); return true
            }
            if let submenu = item.submenu, invoke(title, in: submenu) { return true }
        }
        return false
    }
    @MainActor static func attributesAndLifetime() {
        var actions: [String] = []
        func leaf(_ title: String, enabled: Bool = true) -> [String: Any] {
            ["label": title, "enabled": enabled, "action": ["id": title], "selected": true,
             "hint": "Shortcut", "bindings": [["key": "z", "command": true, "shift": true]]]
        }
        // The projection can be released before the native menu is tracked.
        let menu = AppleContextMenu(JSON(["title": "Context", "sections": [
            [], [leaf("Enabled"), leaf("Disabled", enabled: false)],
            [["label": "Unavailable group", "enabled": false, "sections": [[leaf("Blocked child")]]]]
        ]])) { actions.append($0["id"].string) }.nativeMenu()
        precondition(menu.title == "Context" && menu.items.count == 4)
        precondition(!menu.autoenablesItems && menu.items[2].isSeparatorItem)
        let first = menu.items[0]
        precondition(first.state == .on && first.keyEquivalent == "z")
        precondition(first.keyEquivalentModifierMask == [.command, .shift] && first.toolTip == "Shortcut")
        precondition(invoke("Enabled", in: menu) && actions == ["Enabled"])
        precondition(invoke("Disabled", in: menu) && actions == ["Enabled"])
        precondition(invoke("Blocked child", in: menu) && actions == ["Enabled"],
            "A disabled ancestor must prevent direct descendant invocation too")
        print("PASS: native sections, state, shortcuts, disabled ancestors and retained callback lifetime")
    }
    @MainActor static func main() async throws {
        _ = NSApplication.shared; NSApp.setActivationPolicy(.prohibited)
        attributesAndLifetime()
        for platform: UInt32 in [0, 1] {
            let store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            try await wait("editor startup") { !store.state["layers"].array.isEmpty }
            let original = store.state["layers"].array.map { $0["id"].uint }
            let layer = original[0]
            let model = await query(store, ["type": "layer_menu", "id": layer, "mask": false])
            let menu = AppleContextMenu(model) { store.dispatch($0) }.nativeMenu()
            precondition(invoke("New layer", in: menu), "Shared layer menu must retain New layer")
            try await wait("native New layer action") { store.state["layers"].array.count == original.count + 1 }
            store.dispatch(["type": "invoke", "command": "undo"])
            try await wait("native menu action Undo") { store.state["layers"].array.map { $0["id"].uint } == original }
            store.dispatch(["type": "invoke", "command": "redo"])
            try await wait("native menu action Redo") { store.state["layers"].array.count == original.count + 1 }
            precondition(store.failure == nil, store.failure ?? "")
            print("PASS: platform \(platform), actual NSMenu New layer dispatch and shared Undo/Redo")
        }
    }
}
