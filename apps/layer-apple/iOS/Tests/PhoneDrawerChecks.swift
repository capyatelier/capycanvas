import XCTest
import UIKit

extension EditorLaunchTests {
    @MainActor func testPhoneToolDrawersLight() throws { try checkPhoneToolDrawers(theme: "light") }
    @MainActor func testPhoneToolDrawersDark() throws { try checkPhoneToolDrawers(theme: "dark") }
    @MainActor func testPhoneCommandSettingsLight() throws { try checkPhoneCommandSettings(theme: "light") }
    @MainActor func testPhoneCommandSettingsDark() throws { try checkPhoneCommandSettings(theme: "dark") }
    @MainActor func testPhoneWindowDrawersLight() throws { try checkPhoneWindowDrawers(theme: "light") }
    @MainActor func testPhoneWindowDrawersDark() throws { try checkPhoneWindowDrawers(theme: "dark") }

    @MainActor private func checkPhoneToolDrawers(theme: String) throws {
        for orientation in phoneCaptureOrientations {
            requestedPhoneOrientation = orientation
            for preset in ["painter", "illustrator", "photographer"] {
                let actions: [[String: Any]] = [
                    ["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:" + preset]]]
                let app = try phoneCaptureApplication(theme: theme, actions: actions)
                for identifier in phoneTools(in: app).map(\.identifier) {
                    let tile = app.buttons[identifier]
                    let label = tile.label
                    guard let tileID = Int(identifier.replacingOccurrences(of: "phone-tool-", with: "")),
                        let control = phoneObservedTools(in: app)?.first(where: { ($0["id"] as? Int) == tileID })?["control"] as? [String: Any] else {
                        return XCTFail("The drawer opener needs its observed shared control")
                    }
                    let transforming = control["command"] as? String == "scale_rotate"
                    let layers = transforming ? try phoneDrawerOwnedImage(in: app) : nil
                    if preset == "painter" && (control["panel"] as? String == "layers" || control["kind"] as? String == "color") {
                        phoneDrawerPencil(in: app)
                    }
                    phoneDrawerOpen(tile, in: app)
                    let drawer = phoneDrawer(in: app)
                    XCTAssertTrue(drawer.waitForExistence(timeout: 10), label)
                    phoneDrawerCapture(in: app, id: phoneInventoryTileID(in: app, tile: tile, variant: "body"), theme: theme,
                        fixture: actions, extra: ["opener": identifier, "tool_label": label])
                    try visitPhoneDrawerSets(in: app) {
                        if let id = phoneInventoryTileID(in: app, tile: tile, variant: "bottom", required: false) {
                            phoneDrawerCapture(in: app, id: id, theme: theme, fixture: actions, extra: ["scroll_position": "bottom"])
                        }
                    }
                    if app.descendants(matching: .any)["color-panel-controls"].firstMatch.exists {
                        visitPhoneDrawerColor(in: app, tile: tile, theme: theme, fixture: actions)
                    }
                    if app.buttons["filter-search-toggle"].exists {
                        workspaceActivate(app.buttons["filter-search-toggle"])
                        let search = app.textFields["filter-search"]
                        XCTAssertTrue(search.waitForExistence(timeout: 10))
                        expectation(for: NSPredicate(format: "hasKeyboardFocus == YES"), evaluatedWith: search)
                        waitForExpectations(timeout: 10)
                        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
                        dismissEditorKeyboard(in: app)
                        workspaceActivate(app.buttons["filter-search-toggle"])
                    }
                    if app.buttons["layer-properties-disclosure"].exists {
                        visitPhoneDrawerProperties(in: app) {
                            phoneDrawerCapture(in: app, id: phoneInventoryTileID(in: app, tile: tile, variant: "properties"),
                                theme: theme, fixture: actions)
                        }
                    }
                    workspaceActivate(tile)
                    XCTAssertTrue(drawer.waitForNonExistence(timeout: 10))
                    workspaceActivate(app.buttons["phone-tool-settings"])
                    XCTAssertTrue(phoneDrawer(in: app).waitForExistence(timeout: 10))
                    XCTAssertTrue(app.descendants(matching: .any)["drawer-panel-tool_settings"].firstMatch.exists)
                    workspaceActivate(app.buttons["phone-tool-settings"])
                    XCTAssertTrue(phoneDrawer(in: app).waitForNonExistence(timeout: 10))
                    if let layers { phoneDrawerRemoveOwnedImage(layers, in: app) }
                }
                workspaceActivate(app.buttons["phone-tool-settings"])
                XCTAssertTrue(phoneDrawer(in: app).waitForExistence(timeout: 10))
                XCTAssertTrue(app.descendants(matching: .any)["drawer-panel-tool_settings"].firstMatch.exists)
                let workspace = try XCTUnwrap(phoneObservedValue("persistence-workspace", in: app))
                phoneDrawerCapture(in: app, id: phoneInventoryID(workspace: workspace,
                    control: ["kind": "panel", "panel": "tool_settings"], variant: "body"), theme: theme, fixture: actions)
                workspaceActivate(app.buttons["phone-tool-settings"])
                XCTAssertTrue(phoneDrawer(in: app).waitForNonExistence(timeout: 10))
                phoneDrawerPencil(in: app)
                _ = phoneMenuRoot(in: app)
                phoneChooseFingerMode("Draw", in: app)
                workspaceActivate(app.buttons["phone-capy"])
                XCTAssertTrue(app.descendants(matching: .any)["phone-menu"].firstMatch.waitForNonExistence(timeout: 10))
                phoneDrawerStroke(in: app)
                app.terminate()
            }
        }
    }

    @MainActor private func checkPhoneWindowDrawers(theme: String) throws {
        for orientation in phoneCaptureOrientations {
            requestedPhoneOrientation = orientation
            try visitPhoneKeyboardDrawer(theme: theme)
            try visitPhoneWindowDrawers(theme: theme)
            try visitPhoneLayerCounts(theme: theme)
            try visitPhoneDrawerAnchors(theme: theme)
        }
    }

    @MainActor private func visitPhoneKeyboardDrawer(theme: String) throws {
        let app = try phoneCaptureApplication(theme: theme, actions: [
            ["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:painter"]],
            ["type": "invoke", "command": "drawing_brush"]])
        defer { app.terminate() }
        workspaceActivate(app.buttons["phone-tool-settings"])
        let drawer = phoneDrawer(in: app)
        XCTAssertTrue(drawer.waitForExistence(timeout: 10))
        let value = drawer.buttons["number-value-tool-size"]
        XCTAssertTrue(value.waitForExistence(timeout: 10))
        phoneDrawerReveal(value, in: drawer)
        let bars = ["phone-bar-0", "phone-bar-1"].map { app.descendants(matching: .any)[$0].firstMatch.frame }
        workspaceActivate(value)
        let keyboard = app.keyboards.firstMatch
        XCTAssertTrue(keyboard.waitForExistence(timeout: 10))
        let entry = app.textFields["number-entry-tool-size"]
        XCTAssertTrue(entry.waitForExistence(timeout: 10))
        XCTAssertTrue(entry.isHittable)
        XCTAssertLessThanOrEqual(entry.frame.maxY, keyboard.frame.minY + 1)
        expectation(for: NSPredicate { _, _ in drawer.exists && drawer.frame.maxY <= keyboard.frame.minY + 1 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        for (index, frame) in bars.enumerated() {
            XCTAssertEqual(app.descendants(matching: .any)["phone-bar-" + String(index)].firstMatch.frame, frame)
        }
        entry.typeText("\n")
        XCTAssertTrue(keyboard.waitForNonExistence(timeout: 10))
        XCTAssertTrue(drawer.exists)
    }

    @MainActor private func phoneDrawer(in app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any)["tool-drawer"].firstMatch
    }

    @MainActor private func phoneDrawerOpen(_ tile: XCUIElement, in app: XCUIApplication) {
        workspaceActivate(tile)
        expectation(for: NSPredicate { _, _ in tile.isSelected || self.phoneDrawer(in: app).exists }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        if !phoneDrawer(in: app).exists { workspaceActivate(tile) }
    }

    @MainActor private func phoneDrawerCapture(in app: XCUIApplication, id: PhoneCaptureID?, theme: String,
        fixture: [[String: Any]], extra: [String: Any] = [:]) {
        let drawer = phoneDrawer(in: app)
        XCTAssertTrue(drawer.exists)
        let frame = drawer.frame, viewport = app.frame
        XCTAssertGreaterThan(frame.width, 0)
        XCTAssertGreaterThan(frame.height, 0)
        XCTAssertGreaterThanOrEqual(frame.minX, viewport.minX - 1)
        XCTAssertGreaterThanOrEqual(frame.minY, viewport.minY - 1)
        XCTAssertLessThanOrEqual(frame.maxX, viewport.maxX + 1)
        XCTAssertLessThanOrEqual(frame.maxY, viewport.maxY + 1)
        var metadata = extra
        metadata["drawer_bounds"] = [frame.minX, frame.minY, frame.width, frame.height]
        metadata["presented_panels"] = drawer.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "drawer-panel-")).allElementsBoundByIndex.map(\.identifier)
        phoneCapture(in: app, id: id, theme: theme, fixture: fixture, extra: metadata)
    }

    @MainActor private func visitPhoneDrawerSets(in app: XCUIApplication, captureBottom: () -> Void = {}) throws {
        let drawer = phoneDrawer(in: app)
        func revealLastChoice() -> Bool {
            let choices = drawer.buttons.matching(NSPredicate(format:
                "identifier BEGINSWITH %@ OR identifier BEGINSWITH %@", "brush-", "tool-subtool-")).allElementsBoundByIndex
                .filter { $0.isEnabled }
            guard let last = choices.last else { return false }
            phoneDrawerReveal(last, in: drawer)
            return true
        }
        let groups = drawer.buttons.matching(NSPredicate(format:
            "identifier BEGINSWITH %@ OR identifier BEGINSWITH %@ OR identifier BEGINSWITH %@ OR identifier BEGINSWITH %@",
            "tool-group-", "brush_sets-", "sculpt_sets-", "brushes-")).allElementsBoundByIndex
            .filter { $0.isEnabled }.map { ($0.identifier, $0.label) }
        let selectedGroup = groups.first { identifier, label in
            drawer.buttons.matching(NSPredicate(format: "identifier == %@ AND label == %@", identifier, label)).firstMatch.isSelected
        }
        let selectedChoice = drawer.buttons.matching(NSPredicate(format:
            "(identifier BEGINSWITH %@ OR identifier BEGINSWITH %@) AND selected == YES", "brush-", "tool-subtool-"))
            .allElementsBoundByIndex.first.map { ($0.identifier, $0.label) }
        for (identifier, label) in groups {
            let group = drawer.buttons.matching(NSPredicate(format: "identifier == %@ AND label == %@", identifier, label)).firstMatch
            phoneDrawerReveal(group, in: drawer)
            workspaceActivate(group)
            expectation(for: NSPredicate(format: "selected == true"), evaluatedWith: group)
            waitForExpectations(timeout: 10)
            _ = revealLastChoice()
        }
        for selected in [selectedGroup, selectedChoice].compactMap({ $0 }) {
            let control = drawer.buttons.matching(NSPredicate(format: "identifier == %@ AND label == %@", selected.0, selected.1)).firstMatch
            if !control.isSelected {
                phoneDrawerReveal(control, in: drawer)
                workspaceActivate(control)
            }
            expectation(for: NSPredicate(format: "selected == true"), evaluatedWith: control)
            waitForExpectations(timeout: 10)
        }
        if revealLastChoice() { captureBottom() }
    }

    @MainActor private func visitPhoneDrawerColor(in app: XCUIApplication, tile: XCUIElement?, theme: String,
        fixture: [[String: Any]]) {
        let drawer = phoneDrawer(in: app)
        let wheel = app.descendants(matching: .any)["color-panel-controls"].firstMatch
        XCTAssertGreaterThanOrEqual(wheel.frame.width, 127)
        XCTAssertGreaterThanOrEqual(wheel.frame.height, 127)
        XCTAssertGreaterThanOrEqual(wheel.frame.minX, drawer.frame.minX - 1)
        XCTAssertLessThanOrEqual(wheel.frame.maxX, drawer.frame.maxX + 1)
        let footer = app.descendants(matching: .any)["palette-footer"].firstMatch
        phoneDrawerReveal(footer, in: drawer)
        XCTAssertGreaterThanOrEqual(footer.frame.minY, drawer.frame.minY - 1)
        XCTAssertLessThanOrEqual(footer.frame.maxY, drawer.frame.maxY + 1)
        XCTAssertTrue(app.descendants(matching: .any)["drawer-panel-palettes"].firstMatch.exists)
        if let tile, let id = phoneInventoryTileID(in: app, tile: tile, variant: "bottom", required: false) {
            phoneDrawerCapture(in: app, id: id, theme: theme, fixture: fixture, extra: ["scroll_position": "bottom"])
        }
    }

    @MainActor private func visitPhoneDrawerProperties(in app: XCUIApplication, capture: () -> Void = {}) {
        let disclosure = app.buttons["layer-properties-disclosure"]
        let label = disclosure.label
        workspaceActivate(disclosure)
        expectation(for: NSPredicate(format: "label != %@", label), evaluatedWith: disclosure)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.descendants(matching: .any)["layer-properties-pages"].firstMatch.exists)
        capture()
        workspaceActivate(disclosure)
        expectation(for: NSPredicate(format: "label == %@", label), evaluatedWith: disclosure)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.descendants(matching: .any)["layer-rows"].firstMatch.exists)
    }

    @MainActor private func phoneWindowDrawerMenu(in app: XCUIApplication) -> XCUIElement {
        workspaceActivate(app.buttons["phone-capy"])
        let menu = app.descendants(matching: .any)["phone-menu"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        let window = phoneMenuRow("Window", in: app)
        revealEditorControl(window, in: app.descendants(matching: .any)["phone-menu-scroll"].firstMatch)
        guard let title = phoneMenuTitle(for: window) else { return menu }
        workspaceActivate(window)
        phoneWaitForMenuTitle(title, in: app)
        return menu
    }

    @MainActor private func phoneWindowDrawer(_ label: String, in app: XCUIApplication) {
        let menu = phoneWindowDrawerMenu(in: app)
        let row = phoneMenuRow(label, in: app)
        revealEditorControl(row, in: app.descendants(matching: .any)["phone-menu-scroll"].firstMatch)
        workspaceActivate(row)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        XCTAssertTrue(phoneDrawer(in: app).waitForExistence(timeout: 10))
    }

    @MainActor private func visitPhoneWindowDrawers(theme: String) throws {
        let actions: [[String: Any]] = [["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:photographer"]],
            ["type": "select_brush", "id": 2]]
        let app = try phoneCaptureApplication(theme: theme, actions: actions)
        let menu = phoneWindowDrawerMenu(in: app)
        let color = phoneMenuRow("Color", in: app)
        XCTAssertTrue(color.exists)
        let section = color.identifier.split(separator: "-").dropLast().joined(separator: "-") + "-"
        let panels = menu.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", section))
            .allElementsBoundByIndex.map { ($0.identifier, $0.label) }
        XCTAssertGreaterThan(panels.count, 10)
        workspaceActivate(app.buttons["phone-capy"])
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        for (identifier, label) in panels {
            phoneWindowDrawer(label, in: app)
            let panels = Set(phoneDrawer(in: app).descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "drawer-panel-")).allElementsBoundByIndex.map(\.identifier))
            for panel in panels.sorted() {
                let control: [String: Any] = ["kind": "panel", "panel": String(panel.dropFirst("drawer-panel-".count))]
                if let workspace = phoneObservedValue("persistence-workspace", in: app),
                    let id = phoneInventoryID(workspace: workspace, control: control, variant: "window", required: false) {
                    phoneDrawerCapture(in: app, id: id, theme: theme, fixture: actions, extra: ["opener": "phone-capy", "menu_row": identifier])
                }
            }
            if panels.contains("drawer-panel-sizes") {
                let tiles = phoneDrawer(in: app).buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "size-")).allElementsBoundByIndex
                XCTAssertEqual(tiles.count, 40)
                let first = try XCTUnwrap(tiles.first)
                for tile in tiles {
                    phoneDrawerReveal(tile, in: phoneDrawer(in: app))
                    XCTAssertEqual(tile.frame.width, first.frame.width, accuracy: 1)
                    XCTAssertEqual(tile.frame.height, first.frame.height, accuracy: 1)
                    XCTAssertGreaterThanOrEqual(tile.frame.height, 44)
                    XCTAssertGreaterThanOrEqual(tile.frame.width, 36)
                    let labels = tile.staticTexts.allElementsBoundByIndex
                    for caption in labels where caption.frame.height > 0 {
                        if caption.frame.height <= 18 {
                            XCTAssertEqual(tile.frame.height, 44, accuracy: 1)
                        } else {
                            XCTAssertGreaterThanOrEqual(tile.frame.height, 24 + caption.frame.height + 2 - 1)
                        }
                        XCTAssertGreaterThanOrEqual(caption.frame.minY - tile.frame.minY, 24 - 1)
                        XCTAssertLessThanOrEqual(caption.frame.maxY, tile.frame.maxY)
                    }
                }
            }
            try visitPhoneDrawerSets(in: app)
            if app.descendants(matching: .any)["color-panel-controls"].firstMatch.exists {
                visitPhoneDrawerColor(in: app, tile: nil, theme: theme, fixture: actions)
            }
            phoneDrawerOutsideContact(in: app)
        }
        app.terminate()
    }

    @MainActor private func phoneDrawerPencil(in app: XCUIApplication) {
        guard let tool = phoneObservedTools(in: app)?.first(where: {
            ($0["control"] as? [String: Any])?["command"] as? String == "drawing_brush"
        }), let id = tool["id"] as? Int else { return XCTFail("The workspace needs its observed drawing brush opener") }
        let tile = app.buttons["phone-tool-" + String(id)]
        phoneDrawerOpen(tile, in: app)
        workspaceActivate(tile)
        XCTAssertTrue(phoneDrawer(in: app).waitForNonExistence(timeout: 10))
    }

    @MainActor private func phoneDrawerOwnedImage(in app: XCUIApplication) throws -> Set<String> {
        let token = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_FILE_TEST_TOKEN"])
        let provider = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_UI_PACKAGE_PROVIDER"])
        XCTAssertEqual(provider, "Capy Test Files " + token)
        phoneWindowDrawer("Layers", in: app)
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let original = Set(rows.allElementsBoundByIndex.map(\.identifier))
        XCTAssertFalse(original.isEmpty)
        workspaceActivate(app.buttons["layer-Import Image as Layer…"])
        phoneChooseProviderFile("Inventory marks.png", token: token, provider: provider, in: app)
        let apply = app.buttons["canvas-bar-action-apply_transform"]
        XCTAssertTrue(apply.waitForExistence(timeout: 30))
        if phoneDrawer(in: app).exists { phoneDrawerOutsideContact(in: app) }
        activateCanvasBarAction("apply_transform", label: "Apply", in: app)
        XCTAssertTrue(apply.waitForNonExistence(timeout: 30))
        expectation(for: NSPredicate(format: "label == %@", "Saved"), evaluatedWith: app.staticTexts["persistence-status"])
        expectation(for: NSPredicate(format: "label == %@", "Recovery ready"), evaluatedWith: app.staticTexts["recovery-status"])
        waitForExpectations(timeout: 60)
        return original
    }

    @MainActor private func phoneDrawerRemoveOwnedImage(_ original: Set<String>, in app: XCUIApplication) {
        phoneAlertCommand("Edit", label: "Undo", in: app)
        phoneWindowDrawer("Layers", in: app)
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        expectation(for: NSPredicate { _, _ in Set(rows.allElementsBoundByIndex.map(\.identifier)) == original }, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        phoneDrawerOutsideContact(in: app)
        phoneDrawerPencil(in: app)
    }

    @MainActor private func checkPhoneCommandSettings(theme: String) throws {
        for orientation in phoneCaptureOrientations {
            requestedPhoneOrientation = orientation
            try visitPhoneCommandSettings(theme: theme)
        }
    }

    @MainActor private func visitPhoneCommandSettings(theme: String) throws {
        let baseline: [[String: Any]] = [["type": "set_theme", "theme": theme],
            ["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:photographer"]],
            ["type": "invoke", "command": "lasso"],
            ["type": "invoke", "command": "spot_heal"],
            ["type": "invoke", "command": "sculpt"], ["type": "select_brush_set", "group": "blend"],
            ["type": "select_brush", "id": 2]]
        let app = try phoneCaptureApplication(theme: theme, actions: baseline)
        defer { app.terminate() }
        for id in phoneInventoryIDs(variant: "settings") {
            guard let control = id.inventory?["control"] as? [String: Any], let command = control["command"] as? String else {
                return XCTFail("The settings inventory needs its shared command")
            }
            let transforming = command == "scale_rotate"
            var original: Set<String>?
            if transforming {
                try phoneRelaunch(app, theme: theme, actions: baseline)
                original = try phoneDrawerOwnedImage(in: app)
            }
            let actions = baseline + [["type": "invoke", "command": command]]
            let canvasMode = command == "crop" || transforming
            try phoneRelaunch(app, theme: theme, actions: canvasMode ? baseline : actions)
            if canvasMode { phoneMenuCommand(label: transforming ? "Transform" : "Crop", in: app) }
            workspaceActivate(app.buttons["phone-tool-settings"])
            XCTAssertTrue(phoneDrawer(in: app).waitForExistence(timeout: 10))
            XCTAssertTrue(app.descendants(matching: .any)["drawer-panel-tool_settings"].firstMatch.exists)
            phoneDrawerCapture(in: app, id: id, theme: theme, fixture: actions, extra: ["invoked_control": control])
            workspaceActivate(app.buttons["phone-tool-settings"])
            XCTAssertTrue(phoneDrawer(in: app).waitForNonExistence(timeout: 10))
            let cancel = app.buttons["canvas-bar-action-cancel_transform"]
            if cancel.exists && cancel.isEnabled {
                activateCanvasBarAction("cancel_transform", label: "Cancel", in: app)
                XCTAssertTrue(cancel.waitForNonExistence(timeout: 10))
            }
            if let original { phoneDrawerRemoveOwnedImage(original, in: app) }
        }
    }

    @MainActor private func visitPhoneLayerCounts(theme: String) throws {
        for count in [2, 5, 26] {
            let actions: [[String: Any]] = [["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:photographer"]],
                ["type": "select_brush", "id": 2]] + Array(repeating: ["type": "invoke", "command": "add_layer"], count: count - 2)
            let app = try phoneCaptureApplication(theme: theme, actions: actions)
            phoneWindowDrawer("Layers", in: app)
            let drawer = phoneDrawer(in: app)
            let rows = drawer.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            let paper = app.buttons["layer-add-paper"].exists ? 1 : 0
            XCTAssertGreaterThan(rows.count, 0)
            XCTAssertLessThanOrEqual(rows.count + paper, count)
            let height = drawer.frame.height
            visitPhoneDrawerProperties(in: app)
            XCTAssertEqual(drawer.frame.height, height, accuracy: 1)
            phoneDrawerOutsideContact(in: app)
            phoneWindowDrawer("Layers", in: app)
            XCTAssertEqual(drawer.frame.height, height, accuracy: 1)
            if count == 26 {
                phoneDrawerCapture(in: app, id: phoneCaptureID("drawers", "layers-live-capped"), theme: theme, fixture: actions,
                    extra: ["fixture_presented_rows": count, "accessible_rows": rows.count])
                let add = drawer.buttons["layer-New layer"]
                XCTAssertTrue(add.isEnabled)
                XCTAssertGreaterThan(add.frame.width, 0)
                XCTAssertGreaterThan(add.frame.height, 0)
                XCTAssertTrue(drawer.frame.contains(add.frame))
                let last = rows.firstMatch.identifier
                workspaceActivate(add)
                expectation(for: NSPredicate { _, _ in rows.firstMatch.identifier != last }, evaluatedWith: app)
                waitForExpectations(timeout: 10)
                XCTAssertEqual(drawer.frame.height, height, accuracy: 1)
                let scroll = app.descendants(matching: .any)["layer-rows"].firstMatch, top = rows.firstMatch.frame
                XCTAssertTrue(dragEditorScroll(scroll, snapshot: try scroll.snapshot(), towardEnd: true))
                XCTAssertNotEqual(rows.firstMatch.frame, top, "The capped layer rows must scroll inside their drawer")
                XCTAssertTrue(dragEditorScroll(scroll, snapshot: try scroll.snapshot(), towardEnd: false))
            }
            app.terminate()
        }
    }

    @MainActor private func visitPhoneDrawerAnchors(theme: String) throws {
        for swapped in [false, true] {
            let actions: [[String: Any]] = [["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:photographer"]],
                ["type": "select_brush", "id": 2], ["type": "preferences", "action": ["type": "edit", "id": "finger_drawing", "value": 1]]]
            let app = try phoneCaptureApplication(theme: theme, actions: actions)
            let initial = try XCTUnwrap(app.launchEnvironment["CAPY_INITIAL_ACTIONS"]?.data(using: .utf8))
            let baseline = try XCTUnwrap(try JSONSerialization.jsonObject(with: initial) as? [[String: Any]])
            for material in [0, 2] {
                let fixture = baseline + [["type": "preferences", "action": ["type": "edit", "id": "transparency", "value": material]]]
                try phoneRelaunch(app, theme: theme, actions: fixture)
                phoneSetSwapped(swapped, in: app)
                for opener in ["phone-tool-settings", "phone-capy"] {
                    if opener == "phone-capy" { phoneWindowDrawer("Layers", in: app) }
                    else { workspaceActivate(app.buttons[opener]) }
                    XCTAssertTrue(phoneDrawer(in: app).waitForExistence(timeout: 10))
                    _ = workspacePoint(app.buttons[opener])
                    let state = (opener == "phone-capy" ? "drawer-capy-layers-" : "drawer-settings-")
                        + (swapped ? "swapped-" : "normal-") + (material == 0 ? "opaque" : "glass")
                    phoneDrawerCapture(in: app, id: phoneCaptureID("drawers", state), theme: theme, fixture: fixture)
                    phoneDrawerOutsideContact(in: app)
                    XCTAssertTrue(phoneDrawer(in: app).waitForNonExistence(timeout: 10))
                }
            }
            workspaceActivate(app.buttons["phone-tool-settings"])
            XCTAssertTrue(phoneDrawer(in: app).waitForExistence(timeout: 10))
            phoneDrawerStroke(in: app)
            XCTAssertTrue(phoneDrawer(in: app).waitForNonExistence(timeout: 10))
            app.terminate()
        }
    }

    @MainActor func phoneDrawerCanvasRegion(in app: XCUIApplication) -> CGRect? {
        let drawer = phoneDrawer(in: app), canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let bounds = canvas.frame
        let bars = app.descendants(matching: .any).matching(NSPredicate(format:
            "identifier BEGINSWITH %@ OR identifier BEGINSWITH %@", "phone-bar-", "phone-slider-")).allElementsBoundByIndex
        let envelope = bars.filter { $0.exists && $0.identifier.hasPrefix("phone-bar-") }.map(\.frame)
            .reduce(CGRect.null) { $0.union($1) }
        let viewport = bounds.intersection(workspaceViewport(in: app).frame).intersection(envelope)
        let controls = phoneTools(in: app) + [app.buttons["phone-capy"], app.buttons["phone-tool-settings"]] + bars
        let obstacles = (drawer.exists ? [drawer.frame] : []) + controls.filter(\.exists).map(\.frame)
        let target = editorFreeRegion(viewport: viewport, excluding: obstacles)
        XCTAssertNotNil(target, "The editor window must expose canvas outside its drawer and controls")
        return target
    }

    @MainActor private func phoneDrawerCanvasOffset(in app: XCUIApplication) -> CGVector? {
        guard let target = phoneDrawerCanvasRegion(in: app) else { return nil }
        let bounds = app.descendants(matching: .any)["canvas"].firstMatch.frame
        return CGVector(dx: (target.midX - bounds.minX) / bounds.width, dy: (target.midY - bounds.minY) / bounds.height)
    }

    @MainActor private func phoneDrawerOutsideContact(in app: XCUIApplication) {
        guard let offset = phoneDrawerCanvasOffset(in: app) else { return }
        app.descendants(matching: .any)["canvas"].firstMatch.coordinate(withNormalizedOffset: offset).tap()
        XCTAssertTrue(phoneDrawer(in: app).waitForNonExistence(timeout: 10))
    }

    @MainActor private func phoneDrawerReveal(_ element: XCUIElement, in drawer: XCUIElement) {
        let scroll = drawer.elementType == .scrollView ? drawer
            : drawer.scrollViews.containing(element.elementType, identifier: element.identifier).allElementsBoundByIndex.last
        if let scroll { revealEditorControl(element, in: scroll) }
        let frame = element.frame
        XCTAssertTrue(frame.width > 0 && frame.height > 0 && drawer.frame.contains(frame),
            "The revealed control must fit inside its painted drawer: \(element.identifier), control \(frame), drawer \(drawer.frame)")
    }

    @MainActor private func phoneDrawerStroke(in app: XCUIApplication) {
        guard let offset = phoneDrawerCanvasOffset(in: app) else { return }
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let from = canvas.coordinate(withNormalizedOffset: offset), sample = CGPoint(x: offset.dx, y: offset.dy)
        let pixels = editorPixels(in: app, at: sample, size: 96)
        let to = from.withOffset(CGVector(dx: 28, dy: 0))
        let drawer = phoneDrawer(in: app)
        if drawer.exists {
            from.press(forDuration: 0, thenDragTo: to, withVelocity: .slow, thenHoldForDuration: 0)
            XCTAssertTrue(drawer.waitForNonExistence(timeout: 10))
            XCTAssertEqual(editorPixels(in: app, at: sample, size: 96), pixels,
                "The contact which dismisses a drawer must be consumed without painting")
        }
        from.press(forDuration: 0, thenDragTo: to, withVelocity: .slow, thenHoldForDuration: 0)
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: sample, size: 96) != pixels }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
    }
}
