// Exercise real SwiftUI geometry preferences and the serial Rust owner without
// XCTest, a visible editor, GPU rendering or system menu automation.
import AppKit
import SwiftUI

@main struct WorkspaceDrawerChecks {
    @MainActor static func main() async throws {
        _ = NSApplication.shared
        NSApp.setActivationPolicy(.prohibited)
        let binding = JSON(["key": "z", "control": true, "shift": true, "alt": false])
        precondition(menuBindingMatches(binding, key: "я", latin: "z", command: true, shift: true, alt: false))
        precondition(!menuBindingMatches(binding, key: "z", latin: nil, command: true, shift: false, alt: false))
        precondition(!menuBindingMatches(binding, key: "z", latin: nil, command: true, shift: true, alt: true))
        let store = EditorStore(platform: 1, persistence: EditorPersistence(root: nil))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 870),
            styleMask: [.borderless], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        defer { window.contentView = nil; window.close() }
        func wait(_ message: String, _ condition: () -> Bool) async throws {
            let deadline = Date().addingTimeInterval(10)
            while !condition() {
                window.contentView?.layoutSubtreeIfNeeded()
                guard Date() < deadline else {
                    print("Drawer state:", store.state["customization"]["column_drawers"].stableKey)
                    print("Tabs:", JSON(store.workspace.tabs.map(\.raw)).stableKey)
                    print("Drawer bounds:", store.contentDrawers.items["4"]?.geometry["placement"]["bounds"].stableKey ?? "missing")
                    print("Header bounds:", store.workspace.sources[JSON(["kind": "group", "group": 6]).stableKey]?.bounds as Any)
                    fflush(stdout)
                    throw NSError(domain: message, code: 1)
                }
                try await Task.sleep(for: .milliseconds(5))
            }
            precondition(store.failure == nil, store.failure ?? "")
        }
        try await wait("Initial owner state") { !store.state.isNull }
        store.native?.resize(width: 1200, height: 870, scale: 1)
        store.dispatch(["type": "move_panel", "panel": "toolbar", "target": ["kind": "tab", "group": 6], "viewport": [1200,870]])
        store.customize(["type": "set_column_collapsed", "group": 6, "collapsed": true])
        store.customize(["type": "set_column_drawers", "column": 4, "drawers": true])
        let root = WorkspacePanels(store: store, workspace: store.workspace)
            .frame(width: 1200, height: 870).coordinateSpace(name: "editor-workspace")
            .font(.system(size: 44 / 3))
            .onPreferenceChange(WorkspaceTabs.self) { bounds in
                store.workspace.tabs = bounds.compactMap { key, rect in
                    let parts = key.split(separator: ":").compactMap { UInt64($0) }
                    guard parts.count == 2 else { return nil }
                    return JSON(["group": parts[0], "index": parts[1], "bounds": JSON(rect).raw])
                }
            }
        window.contentView = NSHostingView(rootView: root)
        store.customize(["type": "toggle_column_drawer", "group": 6, "panel": "toolbar"])
        let item = JSON(["kind": "panel", "panel": "toolbar"])
        try await wait("Drawer sources and tab measurements") {
            store.workspace.sources[item.stableKey] != nil && store.workspace.tabs.filter { $0["group"].uint == 6 }.count == 2
        }
        let bounds = store.workspace.sources[item.stableKey]!.bounds
        let start = CGPoint(x: bounds.midX, y: bounds.midY)
        precondition(store.workspace.source(at: start)?.stableKey == item.stableKey)
        let groupItem = JSON(["kind": "group", "group": 6])
        try await wait("Drawer header animation and native measurements agree") {
            guard let measured = store.workspace.sources[groupItem.stableKey]?.bounds,
                let drawer = store.contentDrawers.items["4"] else { return false }
            let target = drawer.geometry["placement"]["bounds"].rect
            return target.width > 0 && abs(measured.minY - target.minY) < 0.5 && abs(measured.width - target.width) < 0.5
        }
        let drawerBounds = store.contentDrawers.items["4"]!.geometry["placement"]["bounds"].rect
        let emptyHeader = CGPoint(x: drawerBounds.maxX - 30, y: drawerBounds.minY + 18)
        precondition(store.workspace.source(at: emptyHeader)?.stableKey == groupItem.stableKey,
            "Unused drawer header space must drag the whole group")
        let first = store.workspace.tabs.first { $0["group"].uint == 6 && $0["index"].uint == 0 }!["bounds"].rect
        let end = CGPoint(x: first.minX + 12, y: first.midY)
        store.workspace.start(item, point: start)
        store.workspace.move(item, point: end)
        store.workspace.move(item, point: end, released: true)
        try await wait("Native drawer tab reorder") {
            store.state["customization"]["column_drawers"][0]["tabs"]["panels"][0].string == "toolbar"
        }
        try await wait("Reordered SwiftUI geometry") {
            guard let tools = store.workspace.sources[item.stableKey]?.bounds,
                let brushes = store.workspace.sources[JSON(["kind": "panel", "panel": "brushes"]).stableKey]?.bounds else { return false }
            return tools.midX < brushes.midX
        }
        store.dispatch(["type": "measure_column_drawers", "measurements": []])
        let reordered = store.workspace.sources[item.stableKey]!.bounds
        let restart = CGPoint(x: reordered.midX, y: reordered.midY)
        store.workspace.start(item, point: restart)
        let last = store.workspace.tabs.first { $0["group"].uint == 6 && $0["index"].uint == 1 }!["bounds"].rect
        let append = CGPoint(x: last.maxX - 4, y: last.midY)
        let hint: JSON = await withCheckedContinuation { c in
            store.query(["type": "drop", "item": item.raw, "position": [append.x, append.y],
                "tabs": store.workspace.tabs.map(\.raw)]) { c.resume(returning: $0) }
        }
        precondition(hint["target"]["kind"].string == "tab" && hint["target"]["group"].uint == 6,
            "A drag must restore the drawer's measured drop target after transient invalidation")
        store.workspace.cancel(item)
        let _: JSON = await withCheckedContinuation { c in store.query(["type": "catalog"]) { c.resume(returning: $0) } }
        precondition(store.failure == nil, store.failure ?? "")
        let inspector = ContentDrawerPresentation(id: "4", store: store)
        inspector.reduceMotion = true
        let inspectorModel = store.state["customization"]["column_drawers"][0].replacing("layer_properties", with: JSON(false))
        let sizes = JSON(["heights": [400, 500], "footer_height": 49])
        inspector.refresh(inspectorModel)
        inspector.measureLayerPages(sizes)
        try await wait("Measured inspector page") { inspector.geometry["placement"]["bounds"].rect.height == 400 }
        let nextAnchor = inspectorModel["anchor"].replacing("origin", with: JSON("brushes"))
        inspector.refresh(inspectorModel.replacing("anchor", with: nextAnchor).replacing("layer_properties", with: JSON(true)))
        try await wait("Inspector page after anchor context changes") { inspector.pagePosition == 1 }
        inspector.measureLayerPages(sizes)
        precondition(inspector.geometry["placement"]["bounds"].rect.height == 500,
            "A new sizing context must retain measured page heights until replacement measurements arrive")
        inspector.stop()
        store.customize(["type": "toggle_column_drawer", "group": 6, "panel": "toolbar"])
        try await wait("Closing drawer stops intercepting input") {
            store.contentDrawers.items["4"]?.interactive != true
                && !store.workspace.sourceInstances.values.contains { $0.item == item.stableKey && $0.layer >= 200 }
        }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("phone-drawers-\(UUID())")
        defer { try? FileManager.default.removeItem(at: directory) }
        let phone = EditorStore(platform: 0, persistence: EditorPersistence(root: directory))
        try await wait("Phone drawer owner") { !phone.state.isNull }
        try await phone.workspaces!.started()
        try await phone.workspaces!.perform(["type": "switch", "id": "builtin:workspace:painter"])
        phone.native?.resize(width: 375, height: 667, scale: 1)
        try await wait("Shared phone layout") {
            !phone.snapshot["layout"]["phone"].isNull && phone.snapshot["layout"]["viewport"][0].number == 375
                && phone.snapshot["layout"]["viewport"][1].number == 667
        }
        var phonePlane = CGSize.zero
        window.setContentSize(NSSize(width: 375, height: 667))
        window.contentView = NSHostingView(rootView: PhoneFrame(store: phone, input: phone.workspace.input.phone)
            .onGeometryChange(for: CGSize.self) { $0.size } action: { phonePlane = $0 }
            .frame(width: 375, height: 667, alignment: .topLeading).coordinateSpace(name: "editor-workspace"))
        try await wait("Phone frame native plane") { phonePlane.width > 0 && phonePlane.height > 0 }
        precondition(phonePlane == CGSize(width: 375, height: 667),
            "The phone frame plane must fill its viewport before placing controls: \(phonePlane)")
        phone.dispatch(["type": "phone", "action": ["kind": "panel", "panel": "color"]])
        try await wait("Phone Color measurement plan") { phone.contentDrawers.items["tool"].map { !$0.measurement["measure"].isNull } ?? false }
        let color = phone.contentDrawers.items["tool"]!
        color.reduceMotion = true
        precondition(color.measurement["scrolls_together"].bool && color.measurement["measure"]["placement"]["columns"].array.count == 2)
        color.measure(400, column: 0); color.measure(280, column: 1); color.measureColor(300)
        try await wait("Shared Color fit and placement") { !color.colorFit.isNull && !color.geometry["placement"].isNull }
        precondition(color.colorFit["size"].number >= 128 && color.colorFit["height"].number <= color.colorFit["natural_height"].number)
        phone.dispatch(["type": "phone", "action": ["kind": "panel", "panel": "proof"]])
        try await wait("Content-sized phone Proof") { color.model["columns"][0][0].string == "proof" && color.measurement["content_sized"][0][0].bool }
        color.measure(80, column: 0)
        try await wait("Proof uses its measured body height") { abs(color.geometry["placement"]["bounds"]["height"].number - 80) < 0.5 }
        precondition(!color.measurement["scrolls_together"].bool)
        phone.dispatch(["type": "phone", "action": ["kind": "panel", "panel": "layers"]])
        try await wait("Phone Layers measurement plan") { color.model["columns"][0][0].string == "layers" && !color.measurement["measure"].isNull }
        let layerSizing: JSON = await withCheckedContinuation { continuation in
            phone.query(["type": "layer_drawer_sizing", "measurement": ["fixed_height": 20, "unit_height": 40], "properties_height": 120]) {
                continuation.resume(returning: $0)
            }
        }
        color.measureLayerPages(layerSizing)
        let snapped = ContentDrawerPresentation.frame(["to": layerSizing["placements"][0].raw, "progress": 1, "clip_contents": true],
            pixelScale: phone.contentDrawers.pixelScale)["placement"]
        try await wait("Phone Layers uses shared page placement") { color.geometry["placement"].stableKey == snapped.stableKey }
        color.stop()
        phone.customize(["type": "close_expanded"])
        phone.dispatch(["type": "phone", "action": ["kind": "capy"]])
        let presenter = PhoneMenuPresentation(store: phone)
        window.contentView = NSHostingView(rootView: PhoneMenu(store: phone, input: presenter)
            .frame(width: 375, height: 667).coordinateSpace(name: "editor-workspace"))
        try await wait("Phone menu native width and row measurements publish the constrained height probe") {
            phone.state["phone"]["menu"].bool && phone.contentDrawers.sources["menu"] != nil
        }
        precondition(phone.contentDrawers.sources["menu"]!.anchor["anchor"]["kind"].string == "capy")
        try await wait("Previous tool drawer retires before menu contact") { phone.contentDrawers.items["tool"] == nil }
        try await wait("The common drawer owner completes the menu opening") {
            presenter.frame["placement"].stableKey == ContentDrawerPresentation.frame(["to": presenter.target.raw, "progress": 1, "clip_contents": true], pixelScale: phone.contentDrawers.pixelScale)["placement"].stableKey
        }
        precondition(presenter.drawer === phone.contentDrawers.menu && presenter.focused == nil,
            "Touch opens the common menu without a keyboard focus highlight")
        let openedHeight = presenter.frame["placement"]["bounds"]["height"].number
        phone.dispatch(["type": "phone", "action": ["kind": "capy"]])
        try await wait("The common menu owner starts closing") {
            !presenter.drawer.interactive && !presenter.frame["placement"].isNull
                && presenter.frame["placement"]["bounds"]["height"].number < openedHeight
        }
        presenter.refresh(layout: phone.snapshot["layout"]["phone"], phone: phone.state["phone"])
        phone.dispatch(["type": "phone", "action": ["kind": "capy"]])
        try await wait("Reopening interrupts closing at the unchanged menu placement") {
            phone.state["phone"]["menu"].bool && presenter.drawer.interactive
                && presenter.frame["placement"].stableKey == ContentDrawerPresentation.frame(["to": presenter.target.raw, "progress": 1, "clip_contents": true], pixelScale: phone.contentDrawers.pixelScale)["placement"].stableKey
        }
        let menuBounds = presenter.frame["placement"]["bounds"].rect
        phone.workspace.chrome(["kind": "contact", "position": [menuBounds.midX, menuBounds.midY], "canvas": false])
        let contacted: JSON = await withCheckedContinuation { continuation in
            phone.query(["type": "document_tabs", "op": "phone_menu"]) { continuation.resume(returning: $0) }
        }
        precondition(!contacted.isNull, "A contact inside the native menu frame must preserve the shared menu")
        var fileHit = JSON(), fileRoute = JSON()
        for (section, rows) in presenter.page["sections"].array.enumerated() {
            for (index, row) in rows.array.enumerated() where row["target"]["page"]["kind"].string == "application"
                && row["target"]["page"]["menu"].string == "file" {
                fileHit = PhoneMenuPresentation.hit("row", [section, index]); fileRoute = row["target"]
            }
        }
        precondition(!fileHit.isNull)
        let rootList = presenter.geometry["list"].rect.offsetBy(dx: presenter.target["bounds"]["x"].number,
            dy: presenter.target["bounds"]["y"].number)
        let clippedRow = CGRect(x: rootList.minX, y: rootList.maxY - 4, width: rootList.width, height: presenter.rowHeight)
        presenter.allocated(fileHit, clippedRow)
        let inside = CGPoint(x: rootList.midX, y: rootList.maxY - 2)
        presenter.contact.prepare(presenter.source(at: inside)!, device: .touch, origin: inside)
        precondition(presenter.pressed == fileHit.stableKey && presenter.focused == nil,
            "Pressing a partly clipped row must not scroll keyboard focus to it")
        presenter.contact.cancel(); presenter.cancelPress()
        precondition(presenter.source(at: CGPoint(x: rootList.midX, y: rootList.maxY + 2))?.id != fileHit.stableKey,
            "The hidden part of a row cannot receive a press under the separator or strip")
        presenter.key("arrowdown", latin: nil, command: false, shift: false, alt: false)
        precondition(presenter.focused == presenter.order.first)
        let rootPhone = phone.state["phone"]
        let rootRoute = rootPhone["menu_route"].stableKey
        presenter.activate(fileHit, semantic: true)
        try await wait("The native File row selects the shared route after chrome admission") {
            phone.state["phone"]["menu"].bool && phone.state["phone"]["menu_route"].stableKey == fileRoute.stableKey
                && presenter.route.stableKey == fileRoute.stableKey
        }
        let filePhone = phone.state["phone"]
        let placement = presenter.frame["placement"]
        let list = PhoneUI.resolve(["kind": "menu_geometry", "layout": phone.snapshot["layout"]["phone"].raw,
            "placement": placement.raw, "row_height": presenter.rowHeight])["list"].rect
            .offsetBy(dx: placement["bounds"]["x"].number, dy: placement["bounds"]["y"].number)
        let origin = CGPoint(x: list.minX + list.width * 0.25, y: list.midY)
        presenter.contact.prepare(presenter.source(at: origin)!, device: .touch, origin: origin)
        let released = CGPoint(x: origin.x + list.width * 0.6, y: origin.y)
        precondition(presenter.beginSwipe(at: released), "The shared swipe claims the measured menu list")
        presenter.finishSwipe(cancelled: false)
        presenter.contact.release(at: released)
        try await wait("The retained native menu input lets the shared Back settle complete") {
            if presenter.route.stableKey == fileRoute.stableKey {
                precondition(presenter.swiping && presenter.progress == 1 && !presenter.enabled,
                    "A completed swipe retains the parent preview until the shared route changes")
            }
            return phone.state["phone"]["menu"].bool && phone.state["phone"]["menu_route"].stableKey == rootRoute
                && presenter.route.stableKey == rootRoute && presenter.enabled && !presenter.swiping
        }
        for next in [filePhone, rootPhone] {
            let layout = phone.snapshot["layout"]["phone"]
            let observation = presenter.objectWillChange.sink { _ in
                precondition(presenter.route.stableKey == next["menu_route"].stableKey
                    && presenter.open == next["menu"].bool && presenter.layout.stableKey == layout.stableKey,
                    "Native menu refresh must stage its complete route before publishing")
            }
            presenter.refresh(layout: layout, phone: next)
            observation.cancel()
        }
        window.contentView = nil
        try await wait("The phone menu relinquishes its common chrome frame") {
            phone.contentDrawers.menu.geometry.isNull && phone.contentDrawers.sources["menu"] == nil
        }
        print("PASS: SwiftUI drawer geometry, tab/header drag sources, reorder, restored drop targets, closing retirement and phone menu measurement/contact publication")
    }
}
