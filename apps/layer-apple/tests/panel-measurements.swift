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
        func group(_ panel: String) -> JSON {
            store.snapshot["layout"]["groups"].array.first { $0["panels"].array.contains { $0.string == panel } } ?? JSON()
        }
        func measured(_ panel: String) -> JSON {
            store.snapshot["panel_measurements"].array.first { $0["panel"].string == panel } ?? JSON()
        }
        func fitted(_ panel: String) -> Bool {
            let g = group(panel), measurement = measured(panel), height = measurement["content_height"].number
            let scroll = measurement["scroll"], unit = scroll["unit_height"].number > 0 ? scroll["unit_height"].number : 36
            let body = scroll.isNull ? height : min(height, scroll["fixed_height"].number + 4 * unit)
            return g["floating"].bool && height > 0 && abs(g["bounds"].rect.height - min(body + 36, 675)) < 0.02
        }
        func float(_ panel: String, x: Double) async throws {
            try await store.apply(["type": "move_panel", "panel": panel, "target": ["kind": "float", "position": [x, 180]], "viewport": [1200, 900]])
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
        do {
            let bar = store.barMetrics, textSize: CGFloat = 16
            func size(_ labels: [String], selected: Int = 0) -> CGSize {
                let option = JSON(["Choice": ["items": labels.enumerated().map {
                    ["label": $0.element, "selected": $0.offset == selected] as [String: Any]
                }]])
                return toolOptionSize(option, vertical: false, width: 600,
                    tile: CGSize(width: bar.tile, height: bar.tile), preferences: JSON(), textSize: textSize, bar: bar)
            }
            let labels = ["Short", "A much wider localized dropdown choice"]
            let measured = size(labels)
            let expected = ceil(toolbarTextWidth(labels[1], size: textSize)) + 16 + bar.icon + 12 + 12 + 2 * bar.clear
            precondition(measured.width == expected, "Dropdown fits its widest item, icon, arrow, gaps and padding")
            precondition(size(labels, selected: 1) == measured, "Changing selection retains room for every item")
            precondition(size(["Short"]).width < measured.width, "Changed choice items refit the control")
            precondition(size(labels + ["An even wider localized dropdown choice that needs more room"]).width > measured.width,
                "A replacement choice grows to fit its new items")
        }
        do {
            let unitStore = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
            try await wait("Unit measurement editor") {
                precondition(unitStore.failure == nil, String(describing: unitStore.failure))
                return !unitStore.state.isNull && !unitStore.catalog.isNull
            }
            let retainedUnits = PanelMeasurements(store: unitStore)
            func measuredUnitPanel() -> JSON {
                precondition(unitStore.failure == nil, String(describing: unitStore.failure))
                return unitStore.snapshot["panel_measurements"].array.first { $0["panel"].string == "layers" } ?? JSON()
            }
            let rowUnits: [(CGFloat?, CGFloat)] = [(40, 40), (nil, 40), (32, 32), (0, 32)]
            for (index, row) in rowUnits.enumerated() {
                let (unit, expected) = row
                let tabWidth = CGFloat(86 + index)
                var facts: [PanelSizeKey: CGFloat] = [
                    PanelSizeKey(panel: "layers", part: "tab", kind: .tab): tabWidth,
                    PanelSizeKey(panel: "layers", part: "header"): 92,
                    PanelSizeKey(panel: "layers", part: "rows", kind: .scroll): 120,
                ]
                if let unit { facts[PanelSizeKey(panel: "layers", part: "row-unit", kind: .unit)] = unit }
                retainedUnits.receive(facts)
                try await wait("Measured row units survive lazy absence and adopt actual smaller rows") {
                    measuredUnitPanel()["tab_width"].number == Double(tabWidth)
                        && measuredUnitPanel()["scroll"]["unit_height"].number == expected
                        && measuredUnitPanel()["content_height"].number == 212
                }
            }
            retainedUnits.receive([
                PanelSizeKey(panel: "layers", part: "tab", kind: .tab): 86,
                PanelSizeKey(panel: "layers", part: "rows", kind: .scroll): 0,
            ])
            try await wait("Empty content must not invent retained rows") { measuredUnitPanel()["content_height"].number == 0 }
        }
        do {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent("CapyPanelCache-\(UUID())", isDirectory: true)
            let cacheStore = EditorStore(platform: platform, persistence: EditorPersistence(root: root))
            try await wait("Private cache workspace editor") {
                precondition(cacheStore.failure == nil, String(describing: cacheStore.failure))
                return cacheStore.workspaces?.ready == true && !cacheStore.state.isNull && !cacheStore.catalog.isNull
            }
            cacheStore.native?.resize(width: 1200, height: 900, scale: 1)
            try await wait("Private cache workspace has the fixture viewport") {
                precondition(cacheStore.failure == nil, String(describing: cacheStore.failure))
                return cacheStore.snapshot["layout"]["viewport"].array.map { $0.number } == [1200, 900]
            }
            try await cacheStore.workspaces!.started()
            try await cacheStore.workspaces!.perform(["type": "switch", "id": "builtin:workspace:illustrator"])
            let cache = PanelMeasurements(store: cacheStore)
            func cacheMeasured(_ panel: String) -> JSON {
                precondition(cacheStore.failure == nil, String(describing: cacheStore.failure))
                return cacheStore.snapshot["panel_measurements"].array.first { $0["panel"].string == panel } ?? JSON()
            }
            func registered(_ panel: String) -> Bool {
                cacheStore.state["workspace"]["layout"]["panels"].array.contains { $0["id"].string == panel }
            }
            func presented(_ panel: String) -> Bool {
                cacheStore.snapshot["panels"].array.contains { $0["id"].string == panel }
            }
            func rowFacts(_ unit: CGFloat?, tab: CGFloat) -> [PanelSizeKey: CGFloat] {
                var facts: [PanelSizeKey: CGFloat] = [
                    PanelSizeKey(panel: "layers", part: "tab", kind: .tab): tab,
                    PanelSizeKey(panel: "layers", part: "header"): 60,
                    PanelSizeKey(panel: "layers", part: "footer"): 32,
                    PanelSizeKey(panel: "layers", part: "rows", kind: .scroll): 120,
                ]
                if let unit { facts[PanelSizeKey(panel: "layers", part: "row-unit", kind: .unit)] = unit }
                return facts
            }
            cache.receive(rowFacts(40, tab: 190))
            try await wait("Known row unit is measured before the presentation change") {
                cacheMeasured("layers")["tab_width"].number == 190 && cacheMeasured("layers")["scroll"]["unit_height"].number == 40
            }
            try await cacheStore.workspaces!.perform(["type": "switch", "id": "builtin:workspace:painter"])
            try await wait("Painter keeps Layers registered without presenting it") { registered("layers") && !presented("layers") }
            var painterPanel = ""
            try await wait("Painter's configured compact toolbar is presented") {
                precondition(cacheStore.failure == nil, String(describing: cacheStore.failure))
                let bands = cacheStore.state["workspace"]["layout"]["bands"].array.filter {
                    $0["edge"].string == "left" && $0["alignment"].string == "center"
                }
                guard bands.count == 1, bands[0]["root"]["kind"].string == "tabs" else { return false }
                let panel = bands[0]["root"]["active"].string
                guard !panel.isEmpty && registered(panel) && presented(panel) else { return false }
                painterPanel = panel
                return true
            }
            cache.receive([PanelSizeKey(panel: painterPanel, part: "tab", kind: .tab): 191])
            try await wait("Hidden workspace cache publication completes") {
                cacheMeasured(painterPanel)["tab_width"].number == 191 && cacheMeasured("layers").isNull
            }
            let layerHeader = cacheStore.snapshot["header"]["items"].array.first {
                $0["resolved_control"]["kind"].string == "panel" && $0["resolved_control"]["panel"].string == "layers"
            }!
            try await cacheStore.apply(["type": "measure_header", "height": 60,
                "items": [["id": layerHeader["id"].raw,
                    "bounds": ["x": 800, "y": 0, "width": 40, "height": 60]]]])
            try await cacheStore.apply(["type": "customize", "action": ["type": "toggle_header_drawer", "id": layerHeader["id"].raw]])
            try await wait("Layers is presented again with the same registration") { registered("layers") && presented("layers") }
            cache.receive(rowFacts(nil, tab: 192))
            try await wait("Registered row unit survives Painter presentation absence") {
                cacheMeasured("layers")["tab_width"].number == 192
                    && cacheMeasured("layers")["scroll"]["unit_height"].number == 40
                    && cacheMeasured("layers")["content_height"].number == 212
            }
            cache.receive(rowFacts(32, tab: 193))
            try await wait("Actual smaller rows replace the retained unit after reopening") {
                cacheMeasured("layers")["tab_width"].number == 193 && cacheMeasured("layers")["scroll"]["unit_height"].number == 32
            }
            let before = Set(cacheStore.state["workspace"]["layout"]["panels"].array.map { $0["id"].string })
            try await cacheStore.apply(["type": "customize", "action": ["type": "duplicate_toolbar", "panel": "toolbar"]])
            try await cacheStore.apply(["type": "customize", "action": ["type": "toolbar_name", "name": "Measurement cache \(platform)"]])
            try await cacheStore.apply(["type": "customize", "action": ["type": "confirm_toolbar"]])
            let custom = cacheStore.state["workspace"]["layout"]["panels"].array.first { !before.contains($0["id"].string) }!["id"].string
            try await wait("The new toolbar is registered and presented") { registered(custom) && presented(custom) }
            cache.receive([PanelSizeKey(panel: custom, part: "tab", kind: .tab): 194,
                PanelSizeKey(panel: custom, part: "body"): 99])
            try await wait("The new toolbar has cached width and body facts") {
                cacheMeasured(custom)["tab_width"].number == 194 && cacheMeasured(custom)["content_height"].number == 99
            }
            try await cacheStore.apply(["type": "customize", "action": ["type": "delete_toolbar", "panel": custom]])
            try await cacheStore.apply(["type": "customize", "action": ["type": "confirm_toolbar"]])
            try await wait("Deleting the toolbar removes its registration") { !registered(custom) && !presented(custom) }
            let remaining = painterPanel
            cache.receive([PanelSizeKey(panel: remaining, part: "tab", kind: .tab): 195])
            try await wait("Removed-config cache pruning is published") {
                cacheMeasured(remaining)["tab_width"].number == 195 && cacheMeasured(custom).isNull
            }
            try await cacheStore.apply(["type": "invoke", "command": "undo_workspace"])
            try await wait("Undo restores the same toolbar registration") { registered(custom) && presented(custom) }
            cache.receive([PanelSizeKey(panel: remaining, part: "tab", kind: .tab): 196])
            try await wait("Restored toolbar does not revive its pruned width") {
                cacheMeasured(remaining)["tab_width"].number == 196 && cacheMeasured(custom).isNull
            }
            cache.receive([PanelSizeKey(panel: custom, part: "tab", kind: .tab): 197])
            try await wait("A fresh tab measurement does not revive the removed body") {
                cacheMeasured(custom)["tab_width"].number == 197 && cacheMeasured(custom)["content_height"].number == 0
            }
            await cacheStore.workspaces!.detached()
            print("PASS: platform \(platform), registered row cache survives presentation absence and adopts actual smaller rows; removed registration prunes width and body")
        }
        store.native?.resize(width: 1200, height: 900, scale: 1)
        for panel in ["navigator", "sizes"] where group(panel)["active"].string != panel {
            try await store.apply(["type": "select_panel_tab", "group": group(panel)["id"].raw, "panel": panel])
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
        try await wait("Native scrolling measurements") {
            measured("layers")["scroll"]["unit_height"].number >= 40
                && measured("layers")["scroll"]["fixed_height"].number > 24
                && !measured("brushes")["scroll"].isNull
        }
        precondition(measured("color")["scroll"].isNull, "The compact color picker must retain its full natural size")
        try await store.apply(["type": "customize", "action": ["type": "set_control_visible", "panel": "layers", "control": "layers", "visible": false]])
        try await wait("Hiding the rows removes their scrolling measurements") { measured("layers")["scroll"].isNull }
        try await store.apply(["type": "invoke", "command": "undo_workspace"])
        try await wait("Undo restores the measured rows") { measured("layers")["scroll"]["unit_height"].number >= 40 }
        try await float("sizes", x: 320)
        let sizes = measured("sizes")["content_height"].number
        try await store.apply(["type": "customize", "action": ["type": "set_control_visible", "panel": "sizes", "control": "size_presets", "visible": false]])
        try await wait("Hidden presets shrink the actual floating body") { fitted("sizes") && measured("sizes")["content_height"].number < sizes - 50 }
        try await store.apply(["type": "invoke", "command": "undo_workspace"])
        try await wait("Workspace Undo restores natural sizing") { fitted("sizes") && abs(measured("sizes")["content_height"].number - sizes) < 0.02 }

        let originalBounds = group("sizes")["bounds"].rect
        for phase in ["down", "move", "up"] {
            try await store.apply(["type": "resize_floating", "group": group("sizes")["id"].raw, "edge": "right", "phase": phase,
                "position": [phase == "down" ? originalBounds.maxX : originalBounds.minX + 140, originalBounds.midY], "viewport": [1200, 900]])
        }
        try await wait("Narrow body reflows while preserving the manually sized height") {
            abs(group("sizes")["bounds"].rect.width - 140) < 0.02
                && abs(group("sizes")["bounds"].rect.height - originalBounds.height) < 0.02
                && measured("sizes")["content_height"].number > sizes + 50
        }
        try await store.apply(["type": "invoke", "command": "undo_workspace"])
        try await wait("Undo resize restores the natural body and height") { fitted("sizes") && abs(measured("sizes")["content_height"].number - sizes) < 0.02 }

        try await float("layers", x: 640)
        let layers = measured("layers")["content_height"].number
        try await store.apply(["type": "invoke", "command": "add_layer"])
        try await wait("New layer grows intrinsic content") { fitted("layers") && measured("layers")["content_height"].number > layers + 20 }
        try await float("navigator", x: 880)
        let document = store.state["tabs"][0], navigatorWidth = group("navigator")["bounds"].rect.width
        precondition(abs(measured("navigator")["content_height"].number
            - ((navigatorWidth - 16) * document["height"].number / document["width"].number + store.panelMetrics.height + store.panelMetrics.spacing + 16)) < 0.02,
            "The Navigator measures its overview at the floating width plus its command row")
        precondition(geometry.navigators.count == 1, "Measuring tabs must not mount extra GPU Navigator views")

        try await store.apply(["type": "move_panel", "panel": "stats", "target": ["kind": "tab", "group": group("sizes")["id"].raw], "viewport": [1200, 900]])
        try await wait("Tabbed floating group fits both native labels") {
            let width = measured("sizes")["tab_width"].number + measured("stats")["tab_width"].number + 22
            return abs(group("sizes")["bounds"].rect.width - width) < 0.02 && fitted("stats")
        }

        // Let newly mounted bodies finish layout before saving expected facts.
        try await settle()
        let before = store.state["workspace"].stableKey
        let measurements = store.snapshot["panel_measurements"].stableKey
        try await store.apply(["type": "measure_panels", "measurements": []])
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

        // Native geometry supplies facts; the ordinary shared drag transaction
        // freezes the preview and fits a long scroller once, when released.
        let source = group("brushes")["bounds"].rect
        let beforeDrag = store.state["workspace"]["layout"].stableKey
        func drag(_ phase: String, _ point: CGPoint) async throws {
            try await store.apply(["type": "drag_workspace", "item": ["kind": "panel", "panel": "brushes"],
                "phase": phase, "position": [point.x, point.y], "viewport": [1200, 900], "tabs": []])
        }
        try await drag("down", CGPoint(x: source.minX + 24, y: source.minY + 12))
        try await drag("move", CGPoint(x: 600, y: 899))
        try await wait("Floating preview keeps the visible sidebar size") {
            let preview = store.workspaceMotion.position(group("brushes")["id"].uint).rect
            return abs(preview.width - source.width) < 0.02 && abs(preview.height - source.height) < 0.02
        }
        try await drag("up", CGPoint(x: 600, y: 899))
        try await wait("Long floating panel fits the shared scrolling budget") {
            let bounds = group("brushes")["bounds"].rect
            return group("brushes")["floating"].bool && bounds.height <= 400
                && bounds.height >= 180 && bounds.maxY <= 900
        }
        let afterDrag = store.state["workspace"]["layout"].stableKey
        try await store.apply(["type": "invoke", "command": "undo_workspace"])
        try await wait("One Undo restores the docked panel") { store.state["workspace"]["layout"].stableKey == beforeDrag }
        try await store.apply(["type": "invoke", "command": "redo_workspace"])
        try await wait("One Redo restores the fitted float") { store.state["workspace"]["layout"].stableKey == afterDrag }
        print("PASS: platform \(platform), native tab/body/scroll measurements, floating/tab fit, width reflow, visibility/history, one Navigator, stable publication, frozen drag preview and fitted release with one-step history")
    }
}
