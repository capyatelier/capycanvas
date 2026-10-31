import AppKit
import SwiftUI
import Darwin

@MainActor private final class PanelGeometry {
    var navigators: [UUID: NavigatorPlacement] = [:]
}

@main struct PanelMeasurementChecks {
    @MainActor static func main() {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.accessory)
        Task { @MainActor in
            do {
                FileHandle.standardError.write(Data("PANEL_MEASUREMENTS_BEGIN\n".utf8))
                for platform: UInt32 in [0, 1] { try await check(platform) }
                FileHandle.standardError.write(Data("PANEL_MEASUREMENTS_COMPLETE\n".utf8))
                exit(0)
            } catch {
                FileHandle.standardError.write(Data("FAIL: \(error)\n".utf8)); exit(1)
            }
        }
        NSApp.run()
    }
    @MainActor static func check(_ platform: UInt32) async throws {
        var store = EditorStore(platform: platform, persistence: EditorPersistence(root: nil))
        let geometry = PanelGeometry()
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 900),
                              styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        var advanceFrames: (() async -> Void)?
        func wait(_ label: String, _ predicate: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(15)
            while !predicate() {
                window.contentView?.layoutSubtreeIfNeeded()
                if let error = store.failure { throw NSError(domain: error, code: 1) }
                guard Date() < deadline else {
                    FileHandle.standardError.write(Data("Measurements: \(store.snapshot["panel_measurements"].stableKey)\nGroups: \(store.snapshot["layout"]["groups"].stableKey)\nNavigators: \(geometry.navigators.count)\n".utf8))
                    throw NSError(domain: label, code: 1)
                }
                if let advanceFrames { await advanceFrames() }
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
        func receive(_ cache: PanelMeasurements, from source: EditorStore, facts: [PanelSizeKey: CGFloat]) {
            precondition(!source.snapshot["layout"]["measurement_requests"].array.isEmpty,
                "Shared layout must publish measurement requests before native facts")
            let origins = cache.origins(scale: 1, style: "panel-measurements-test")
            var tagged: [PanelSizeKey: CGFloat] = [:]
            var bodies = Set<String>()
            for (key, value) in facts {
                guard let origin = origins[key.panel] else { preconditionFailure("Missing measurement request for \(key.panel)") }
                tagged[PanelSizeKey(panel: key.panel, part: key.part, kind: key.kind,
                    origin: origin)] = value
                if key.kind == .tab {
                    tagged[PanelSizeKey(panel: key.panel, part: "icon", kind: .tab, origin: origin)] = 36
                } else { bodies.insert(key.panel) }
            }
            for panel in bodies {
                guard let origin = origins[panel] else { continue }
                tagged[PanelSizeKey(panel: panel, part: "width", kind: .width, origin: origin)] =
                    CGFloat(origin.request["width"].number)
            }
            cache.receive(tagged)
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
            unitStore.native?.resize(width: 1200, height: 900, scale: 1)
            try await wait("Unit measurements have the fixture viewport") {
                unitStore.snapshot["layout"]["viewport"].array.map { $0.number } == [1200, 900]
            }
            let retainedUnits = PanelMeasurements(store: unitStore)
            func measuredUnitPanel() -> JSON {
                precondition(unitStore.failure == nil, String(describing: unitStore.failure))
                return unitStore.snapshot["panel_measurements"].array.first { $0["panel"].string == "layers" } ?? JSON()
            }
            let requested = retainedUnits.origins(scale: 1, style: "panel-measurements-test")["layers"]!
            let requestedWidth = CGFloat(requested.request["width"].number)
            let otherWidth = requestedWidth + 64
            let other = PanelMeasurementOrigin(request: requested.request.replacing("width", with: JSON(Double(otherWidth))),
                context: requested.context)
            retainedUnits.receive([
                PanelSizeKey(panel: "layers", part: "tab", kind: .tab, origin: requested): 86,
                PanelSizeKey(panel: "layers", part: "icon", kind: .tab, origin: requested): 36,
                PanelSizeKey(panel: "layers", part: "width", kind: .width, origin: requested): requestedWidth,
                PanelSizeKey(panel: "layers", part: "header", origin: requested): 70,
                PanelSizeKey(panel: "layers", part: "width", kind: .width, origin: other): otherWidth,
                PanelSizeKey(panel: "layers", part: "header", origin: other): 130,
            ])
            try await wait("Two origins for one panel do not sum body fragments or retag width") {
                measuredUnitPanel()["tab_widths"][0].number == 86
                    && measuredUnitPanel()["content_height"].number == 70
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
                receive(retainedUnits, from: unitStore, facts: facts)
                try await wait("Measured row units survive lazy absence and adopt actual smaller rows") {
                    measuredUnitPanel()["tab_widths"][0].number == Double(tabWidth)
                        && measuredUnitPanel()["scroll"]["unit_height"].number == expected
                        && measuredUnitPanel()["content_height"].number == 212
                }
            }
            receive(retainedUnits, from: unitStore, facts: [
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
            receive(cache, from: cacheStore, facts: rowFacts(40, tab: 190))
            try await wait("Known row unit is measured before the presentation change") {
                cacheMeasured("layers")["tab_widths"][0].number == 190 && cacheMeasured("layers")["scroll"]["unit_height"].number == 40
            }
            let illustratorLayersTab = cache.origins(scale: 1, style: "panel-measurements-test")["layers"]!.request["tab"].string
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
            receive(cache, from: cacheStore, facts: [PanelSizeKey(panel: painterPanel, part: "tab", kind: .tab): 191])
            let painterLayersTab = cache.origins(scale: 1, style: "panel-measurements-test")["layers"]!.request["tab"].string
            let retainedLayerTab = illustratorLayersTab == painterLayersTab ? 190.0 : 0.0
            try await wait("Hidden workspace keeps the registered Layers tab fact") {
                cacheMeasured(painterPanel)["tab_widths"][0].number == 191
                    && cacheMeasured("layers")["tab_widths"][0].number == retainedLayerTab
            }
            let layerHeader = cacheStore.snapshot["header"]["items"].array.first {
                $0["resolved_control"]["kind"].string == "panel" && $0["resolved_control"]["panel"].string == "layers"
            }!
            try await cacheStore.apply(["type": "measure_header", "height": 60,
                "items": [["id": layerHeader["id"].raw,
                    "bounds": ["x": 800, "y": 0, "width": 40, "height": 60]]]])
            try await cacheStore.apply(["type": "customize", "action": ["type": "toggle_header_drawer", "id": layerHeader["id"].raw]])
            try await wait("Layers is presented again with the same registration") { registered("layers") && presented("layers") }
            receive(cache, from: cacheStore, facts: rowFacts(40, tab: 192))
            try await wait("Reopened Layers publishes a fresh body fact") {
                cacheMeasured("layers")["tab_widths"][0].number == 192
                    && cacheMeasured("layers")["scroll"]["unit_height"].number == 40
                    && cacheMeasured("layers")["content_height"].number == 212
            }
            receive(cache, from: cacheStore, facts: rowFacts(32, tab: 193))
            try await wait("Actual smaller rows replace the retained unit after reopening") {
                cacheMeasured("layers")["tab_widths"][0].number == 193 && cacheMeasured("layers")["scroll"]["unit_height"].number == 32
            }
            let before = Set(cacheStore.state["workspace"]["layout"]["panels"].array.map { $0["id"].string })
            try await cacheStore.apply(["type": "customize", "action": ["type": "duplicate_toolbar", "panel": "toolbar"]])
            try await cacheStore.apply(["type": "customize", "action": ["type": "toolbar_name", "name": "Measurement cache \(platform)"]])
            try await cacheStore.apply(["type": "customize", "action": ["type": "confirm_toolbar"]])
            let custom = cacheStore.state["workspace"]["layout"]["panels"].array.first { !before.contains($0["id"].string) }!["id"].string
            try await wait("The new toolbar is registered and presented") { registered(custom) && presented(custom) }
            receive(cache, from: cacheStore, facts: [PanelSizeKey(panel: custom, part: "tab", kind: .tab): 194,
                PanelSizeKey(panel: custom, part: "body"): 99])
            try await wait("The new toolbar has cached width and body facts") {
                cacheMeasured(custom)["tab_widths"][0].number == 194 && cacheMeasured(custom)["content_height"].number == 99
            }
            try await cacheStore.apply(["type": "customize", "action": ["type": "delete_toolbar", "panel": custom]])
            try await cacheStore.apply(["type": "customize", "action": ["type": "confirm_toolbar"]])
            try await wait("Deleting the toolbar removes its registration") { !registered(custom) && !presented(custom) }
            let remaining = painterPanel
            receive(cache, from: cacheStore, facts: [PanelSizeKey(panel: remaining, part: "tab", kind: .tab): 195])
            try await wait("Removed-config cache pruning is published") {
                cacheMeasured(remaining)["tab_widths"][0].number == 195 && cacheMeasured(custom).isNull
            }
            try await cacheStore.apply(["type": "invoke", "command": "undo_workspace"])
            try await wait("Undo restores the same toolbar registration") { registered(custom) && presented(custom) }
            receive(cache, from: cacheStore, facts: [PanelSizeKey(panel: remaining, part: "tab", kind: .tab): 196])
            try await wait("Restored toolbar reuses its matching shared identity") {
                cacheMeasured(remaining)["tab_widths"][0].number == 196
                    && cacheMeasured(custom)["tab_widths"][0].number == 194
                    && cacheMeasured(custom)["content_height"].number == 99
            }
            receive(cache, from: cacheStore, facts: [PanelSizeKey(panel: custom, part: "tab", kind: .tab): 197])
            try await wait("A fresh tab measurement keeps the matching body width") {
                cacheMeasured(custom)["tab_widths"][0].number == 197 && cacheMeasured(custom)["content_height"].number == 99
            }
            await cacheStore.workspaces!.detached()
            print("PASS: platform \(platform), registered row facts are remeasured at a new origin, smaller rows replace older units, and undo reuses matching shared facts")
        }
        store.native?.resize(width: 1200, height: 900, scale: 1)
        for panel in ["navigator", "sizes"] where group(panel)["active"].string != panel {
            try await store.apply(["type": "select_panel_tab", "group": group(panel)["id"].raw, "panel": panel])
        }
        let host = NSHostingView(rootView: WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 900).coordinateSpace(name: "editor-workspace")
            .font(.system(size: 44 / 3))
            .onPreferenceChange(NavigatorPlacements.self) { geometry.navigators = $0 })
        window.contentView = host
        try await wait("Native labels and bodies") {
            store.snapshot["panel_measurements"].array.count == store.snapshot["panels"].array.count
                && measured("sizes")["content_height"].number > 20 && geometry.navigators.count == 1
        }
        precondition(store.snapshot["panel_measurements"].array.allSatisfy { $0["tab_widths"][0].number >= 36 })
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

        try await store.apply(["type": "customize", "action": ["type": "set_column_collapsed", "group": 16, "collapsed": true]])
        try await store.apply(["type": "customize", "action": ["type": "toggle_column_drawer", "group": 16, "panel": "layers"]])
        func glass() -> JSON {
            let sent = Mirror(reflecting: store.glass).descendant("sent") as? String ?? "null"
            return (try? JSON.decode(sent)) ?? JSON()
        }
        try await wait("Connected column glass settles") { !glass()["panels"].array.isEmpty }
        let retainedGlass = glass()
        guard let allocations = Mirror(reflecting: store.glass).descendant("panelAllocations") as? [String: PanelBridgeAllocation] else {
            throw HostFailure(message: "Mounted panel allocations must remain available")
        }
        store.glass.panelMeasurements(allocations, layout: store.snapshot["layout"].replacing("glass_query_probe", with: JSON(1)))
        let pendingGlass = glass()
        precondition(!pendingGlass["panels"].array.isEmpty && pendingGlass["panel_revision"].uint == retainedGlass["panel_revision"].uint,
            "A pending panel bridge query must retain the published glass")
        try await store.apply(["type": "customize", "action": ["type": "toggle_column_drawer", "group": 16, "panel": "layers"]])
        try await store.apply(["type": "customize", "action": ["type": "set_column_collapsed", "group": 16, "collapsed": false]])

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
            let width = measured("sizes")["tab_widths"][0].number + measured("stats")["tab_widths"][0].number + 22
            return abs(group("sizes")["bounds"].rect.width - width) < 0.02 && fitted("stats")
        }
        try await store.apply(["type": "select_panel_tab", "group": group("sizes")["id"].raw, "panel": "sizes"])
        try await wait("Sizes is measured at the tabbed width") { fitted("sizes") }
        if group("sizes")["active"].string != "stats" {
            try await store.apply(["type": "select_panel_tab", "group": group("sizes")["id"].raw, "panel": "stats"])
        }
        try await wait("Stats is the mounted tab") { group("sizes")["active"].string == "stats" }

        // Let newly mounted bodies finish layout before saving expected facts.
        try await settle()
        let before = store.state["workspace"].stableKey
        let statsHeight = measured("stats")["content_height"].number
        precondition(statsHeight > 0 && measured("sizes")["content_height"].number > 0,
            "Both tabbed bodies must have prior measurements")
        let reloadContext = store.panelMeasurements.origins(scale: 1, style: "fixture-font-reload")["stats"]!.context
        try await store.apply(["type": "measure_panels", "measurements": ["context": reloadContext.raw, "reports": []]])
        try await wait("Mounted Stats is freshly measured after context invalidation") {
            abs(measured("stats")["content_height"].number - statsHeight) < 0.02
                && measured("sizes")["content_height"].number == 0
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
        guard CommandLine.arguments.contains("--gpu") else { return }
        guard let path = ProcessInfo.processInfo.environment["CAPY_STORAGE_DIR"], path.hasPrefix("/") else {
            throw NSError(domain: "The Photo scope phase requires an absolute owned CAPY_STORAGE_DIR", code: 1)
        }
        let photoRoot = URL(fileURLWithPath: path, isDirectory: true)
            .appendingPathComponent("photo-scopes-\(platform)-\(UUID())", isDirectory: true)
        try FileManager.default.createDirectory(at: photoRoot, withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700])
        FileHandle.standardError.write(Data("SCOPE_PRIVATE_ROOT platform=\(platform) path=\(photoRoot.path) preserved=true\n".utf8))
        store = EditorStore(platform: platform, persistence: EditorPersistence(root: photoRoot))
        host.rootView = WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 900).coordinateSpace(name: "editor-workspace")
            .font(.system(size: 44 / 3))
            .onPreferenceChange(NavigatorPlacements.self) { geometry.navigators = $0 }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        try await wait("Photo private workspace editor is ready") {
            window.isVisible && store.workspaces?.ready == true && !store.state.isNull && !store.catalog.isNull
        }
        store.native?.resize(width: 1200, height: 900, scale: 1)
        try await wait("Photo private workspace has the owned fixture viewport") {
            store.snapshot["layout"]["viewport"].array.map(\.number) == [1200, 900]
                && window.contentView === host
        }
        let native = store.native!
        let scopeSurface = attachSurface(store, CGSize(width: 1200, height: 900))
        defer { native.detach(); withExtendedLifetime(scopeSurface) {} }
        advanceFrames = { await frame(native) }
        try await prepare(native, "Photo scope analysis")
        try await store.workspaces!.started()
        try await store.workspaces!.perform(["type": "switch", "id": "builtin:workspace:photographer"])
        try await wait("Photo scope group is mounted") { !group("histogram").isNull && !group("waveform").isNull }
        func nativeElement(_ root: NSObject, _ identifier: String, depth: Int = 0) -> NSObject? {
            guard depth < 32 else { return nil }
            let id = NSSelectorFromString("accessibilityIdentifier")
            if root.responds(to: id), root.perform(id)?.takeUnretainedValue() as? String == identifier { return root }
            let children = NSSelectorFromString("accessibilityChildren")
            let values = root.responds(to: children) ? root.perform(children)?.takeUnretainedValue() as? [NSObject] ?? [] : []
            for child in values { if let found = nativeElement(child, identifier, depth: depth + 1) { return found } }
            return nil
        }
        func nativeFrame(_ identifier: String) -> CGRect? {
            guard let element = nativeElement(host, identifier) else { return nil }
            let selector = NSSelectorFromString("accessibilityFrame")
            guard element.responds(to: selector) else { return nil }
            let frame = unsafeBitCast(element.method(for: selector),
                to: (@convention(c) (AnyObject, Selector) -> CGRect).self)(element, selector)
            guard frame.minX.isFinite && frame.minY.isFinite && frame.width.isFinite && frame.height.isFinite
                && frame.width > 0 && frame.height > 0 else { return nil }
            return CGRect(x: frame.minX - window.frame.minX, y: window.frame.maxY - frame.maxY,
                width: frame.width, height: frame.height)
        }
        func scopeFrames(_ kind: String) -> [String: CGRect]? {
            var frames: [String: CGRect] = [:]
            for part in ["source", "channel", "chart", "log", "status", "shadows", "highlights"] {
                guard let frame = nativeFrame("scope-\(kind)-\(part)") else { return nil }
                frames[part] = frame
            }
            return frames
        }
        func documentWitness() -> String {
            let layers = store.state["layers"], stamps = store.snapshot["document_tabs"]["session_stamps"]
            let history = store.state["commands"].array.filter { ["undo", "redo"].contains($0["id"].string) }
            precondition(!layers.array.isEmpty && !stamps.array.isEmpty && history.count == 2,
                "Scope resizing must observe actual artwork, checkpoint owners and Undo/Redo states")
            return JSON(["layers": layers.raw, "checkpoints": stamps.raw, "history": history.map(\.raw),
                "selection": store.state["selection"].raw]).stableKey
        }
        func allocation(_ kind: String, height: Double) async throws {
            let bounds = group(kind)["bounds"].rect
            for phase in ["down", "move", "up"] {
                try await store.apply(["type": "resize_floating", "group": group(kind)["id"].raw,
                    "edge": "bottom", "phase": phase,
                    "position": [bounds.midX, phase == "down" ? bounds.maxY : bounds.minY + height],
                    "viewport": [1200, 900]])
            }
            try await wait("The real floating scope reaches its manual allocation") {
                abs(group(kind)["bounds"]["height"].number - height) < 0.02 && window.contentView === host
            }
            try await wait("SCOPE_AX_PREREQUISITE: actual mounted \(kind) chart and native rows") { scopeFrames(kind) != nil }
            try await settle()
        }
        func ordered(_ frames: [String: CGRect]) -> Bool {
            frames["source"]!.maxY <= frames["chart"]!.minY
                && frames["channel"]!.maxY <= frames["chart"]!.minY
                && frames["chart"]!.maxY <= frames["log"]!.minY
                && frames["log"]!.maxY <= frames["status"]!.minY
                && frames["log"]!.maxY <= frames["shadows"]!.minY
        }
        for theme in ["light", "dark"] {
            try await store.apply(["type": "set_theme", "theme": theme])
            window.appearance = NSAppearance(named: theme == "dark" ? .darkAqua : .aqua)
            for (index, kind) in ["histogram", "waveform"].enumerated() {
                if !group(kind)["floating"].bool { try await float(kind, x: Double(320 + index * 320)) }
                try await store.apply(["type": "select_panel_tab", "group": group(kind)["id"].raw, "panel": kind])
                try await wait("The presented scope publishes real analysis") {
                    !store.state[kind]["description"].string.isEmpty
                }
                try await allocation(kind, height: 450)
                try await wait("Scope native chart and rows are exposed") {
                    scopeFrames(kind) != nil && measured(kind)["content_height"].number > 0
                }
                try await settle()
                let first = scopeFrames(kind)!, natural = measured(kind)["content_height"].number
                let firstBounds = group(kind)["bounds"].rect
                let artwork = documentWitness(), saved = store.state["workspace"]["layout"].stableKey
                precondition(ordered(first), "The first real floating allocation retains ordered controls")
                try await allocation(kind, height: 550)
                let grown = scopeFrames(kind)!, grownBounds = group(kind)["bounds"].rect
                FileHandle.standardError.write(Data("SCOPE_GEOMETRY platform=\(platform) theme=\(theme) kind=\(kind) first=\(first) grown=\(grown) first_group=\(firstBounds) grown_group=\(grownBounds) natural=\(natural) measured=\(measured(kind).stableKey)\n".utf8))
                precondition(grown["chart"]!.height > first["chart"]!.height,
                    "A taller real floating scope must grow its mounted native chart")
                precondition(abs((grown["chart"]!.height - first["chart"]!.height)
                    - (grownBounds.height - firstBounds.height)) < 0.02,
                    "The chart alone consumes the added actual body height")
                precondition(ordered(grown), "Growing the chart must retain Log and footer order")
                for part in ["source", "channel", "log", "status", "shadows", "highlights"] {
                    precondition(abs(grown[part]!.height - first[part]!.height) < 0.02,
                        "Scope resizing must preserve native \(part) height")
                }
                precondition(abs(grown["log"]!.width - first["log"]!.width) < 0.02,
                    "This height-only regression must retain the actual scope width")
                precondition(abs(measured(kind)["content_height"].number - natural) < 0.02,
                    "Allocated chart height must not replace the natural fitting request")
                try await allocation(kind, height: 450)
                try await wait("The same mounted chart returns to its first manual allocation") {
                    guard let restored = scopeFrames(kind) else { return false }
                    return abs(restored["chart"]!.height - first["chart"]!.height) < 0.02
                        && abs(measured(kind)["content_height"].number - natural) < 0.02
                }
                precondition(documentWitness() == artwork && store.state["workspace"]["layout"].stableKey == saved,
                    "Scope allocation and restoration preserve layers/checkpoints, artwork Undo/Redo and saved layout")
                let settled = store.state["revision"].uint
                for _ in 0..<40 {
                    host.layoutSubtreeIfNeeded()
                    try await Task.sleep(for: .milliseconds(5))
                }
                precondition(store.state["revision"].uint == settled,
                    "Restored natural measurements must not create a publication loop")
                FileHandle.standardError.write(Data("SCOPE_ALLOCATION_PASS platform=\(platform) theme=\(theme) kind=\(kind) heights=450,550,450\n".utf8))
            }
        }
        await store.workspaces!.detached()
        print("PASS: platform \(platform), native tab/body/scroll measurements, floating/tab fit, width reflow, visibility/history, one Navigator, stable publication, frozen drag preview and fitted release with one-step history")
    }
}
