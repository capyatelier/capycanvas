import XCTest
import CoreGraphics

extension XCTestCase {
    @MainActor func checkLassoControls(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        #if os(macOS)
        func clickWithoutArea() {
            let layers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            let count = layers.count
            workspaceActivate(app.buttons["layer-New layer"])
            expectation(for: NSPredicate(format: "count == %d", count + 1), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
            editorHistory("Undo", in: app)
            expectation(for: NSPredicate(format: "count == %d", count), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
            workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.55)).click()
            editorDocumentTitle(in: app).hover()
            editorHistory("Redo", in: app)
            expectation(for: NSPredicate(format: "count == %d", count + 1), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
            XCTAssertFalse(app.staticTexts["Canvas error"].exists, "A lasso click must not report a canvas error")
            editorHistory("Undo", in: app)
            expectation(for: NSPredicate(format: "count == %d", count), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
        }
        #endif
        editorTool("Lasso selection", in: app)
        editorChoice("Lasso selection", in: app)
        #if os(macOS)
        clickWithoutArea()
        #endif
        workspaceActivate(app.buttons["layer-Layer actions"])
        workspaceActivate(app.buttons["menu-action-Pixel Selection"])
        for label in ["Fill Selection", "Invert Selection", "Deselect Pixels"] {
            XCTAssertTrue(app.buttons["menu-action-" + label].waitForExistence(timeout: 5), label)
        }
        attachEditor(in: app, name: "layer-selection-menu")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkHandAndEyedropper(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        #if os(macOS)
        app.launchEnvironment["CAPY_COLOR_PROBE"] = "1"
        #endif
        app.launch(); capturePaintEditor(in: app)
        editorTool("Hand", in: app)
        let hand = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "tool-group-", "Hand")).firstMatch
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: hand)
        waitForExpectations(timeout: 5)
        let viewport = workspaceViewport(in: app)
        let originalFrame = viewport.frame
        let sheet = editorPaper(in: app)
        let point = sheet.point(0.06, 0.5)
        func pixels() -> Data { editorPixels(in: app, at: point) }
        let paper = pixels()
        XCTAssertTrue(paper.prefix(3).allSatisfy { $0 == 255 }, "The initial sample must lie on white paper")
        let start = viewport.coordinate(withNormalizedOffset: sheet.offset(0.3, 0.5))
        let end = viewport.coordinate(withNormalizedOffset: sheet.offset(0.7, 0.5))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        editorDocumentTitle(in: app).hover()
        #else
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.2)
        #endif
        attachEditor(in: app, name: "hand-after-drag")
        XCTAssertEqual(viewport.frame, originalFrame, "Hand must move the canvas within the same OS window")
        expectation(for: NSPredicate { _, _ in pixels() != paper }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "hand-panned")
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        expectation(for: NSPredicate { _, _ in pixels() == paper }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let undo = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-commands-", "Undo")).firstMatch
        XCTAssertFalse(undo.isEnabled, "Panning and Fit must not create artwork history")
        let flip = app.buttons["navigator-flip_horizontal"]
        for selected in [true, false] {
            workspaceActivate(flip)
            expectation(for: NSPredicate(format: "selected == %@", NSNumber(value: selected)), evaluatedWith: flip)
            waitForExpectations(timeout: 5)
        }
        #if os(macOS)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        workspaceActivate(app.buttons["layer-New layer"])
        workspaceActivate(app.buttons["color-swap"])
        editorTool("Line", in: app); editorChoice("Rectangle", group: true, in: app); editorChoice("Fill", in: app)
        viewport.coordinate(withNormalizedOffset: sheet.offset(0.25, 0.2)).click(forDuration: 0.05,
            thenDragTo: viewport.coordinate(withNormalizedOffset: sheet.offset(0.6, 0.8)))
        workspaceActivate(app.buttons["number-value-layer-opacity"])
        app.textFields["number-entry-layer-opacity"].typeText("50\n")
        expectation(for: NSPredicate(format: "value == %@", "50"), evaluatedWith: app.buttons["number-value-layer-opacity"])
        waitForExpectations(timeout: 5)
        func color() -> [Double] {
            let wheel = app.descendants(matching: .any)["color-wheel"].firstMatch
            guard let text = wheel.value as? String,
                let state = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { return [] }
            return state["rgba"] as? [Double] ?? []
        }
        func pick(_ x: CGFloat, expected: [Double]) {
            viewport.coordinate(withNormalizedOffset: sheet.offset(x, 0.5)).click()
            editorDocumentTitle(in: app).hover()
            expectation(for: NSPredicate { _, _ in
                let actual = color()
                return actual.count == 4 && zip(actual, expected).allSatisfy { abs($0.0 - $0.1) < 0.01 }
            }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        func source(_ label: String) {
            editorTool("Eyedropper", in: app)
            let menu = app.popUpButtons["picker-setting-source"]
            XCTAssertTrue(menu.waitForExistence(timeout: 10), "Picking shows its source setting")
            workspaceActivate(menu)
            workspaceActivate(app.menuItems[label].firstMatch)
            expectation(for: NSPredicate(format: "value == %@", label), evaluatedWith: menu)
            waitForExpectations(timeout: 5)
        }
        source("Visible color")
        pick(0.45, expected: [0.55, 0.35, 0.5, 1])
        attachEditor(in: app, name: "eyedropper-visible")
        source("Selected layer")
        pick(0.45, expected: [0.9, 0.25, 0.2, 1])
        editorTool("Eyedropper", in: app)
        pick(0.8, expected: [0.9, 0.25, 0.2, 1]) // Transparent layer pixels preserve the current color.
        attachEditor(in: app, name: "eyedropper-layer")
        source("Visible color")
        pick(0.8, expected: [0.2, 0.45, 0.8, 1])
        #else
        editorTool("Eyedropper", in: app)
        XCTAssertTrue(app.buttons["picker-setting-source"].waitForExistence(timeout: 10), "Picking shows its source setting")
        #endif
        attachEditor(in: app, name: "eyedropper-controls")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkCanvasNavigationControls(in app: XCUIApplication, theme: String) throws {
        var actions: [[String: Any]] = [
            ["type": "set_theme", "theme": theme],
            ["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:illustrator"]],
            ["type": "set_color", "rgba": [0.2, 0.45, 0.8, 1]],
            ["type": "invoke", "command": "select_all"],
            ["type": "invoke", "command": "fill_selection"],
            ["type": "invoke", "command": "deselect"],
            ["type": "customize", "action": ["type": "insert_tools", "panel": "commands", "before": NSNull()]]
        ]
        for command in ["reset_view", "fit_canvas"] {
            actions.append(["type": "customize", "action": ["type": "picker_select",
                "control": ["kind": "command", "command": command], "selected": true]])
        }
        actions.append(["type": "customize", "action": ["type": "confirm_tools"]])
        for command in ["hand", "reset_view"] {
            actions.append(["type": "customize", "action": ["type": "header", "action": ["type": "add",
                "zone": "right", "before": NSNull(), "item": ["kind": "tool", "control": ["kind": "command", "command": command]]]]])
        }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let status = app.buttons["camera-status"]
        let drawer = app.descendants(matching: .any)["tool-drawer"].firstMatch
        let flip = app.buttons["navigator-flip_horizontal"]
        func readout() -> String { (status.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? status.label }
        func expectCamera(_ expected: String) {
            let matching = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in readout() == expected }, object: status)
            let matched = XCTWaiter.wait(for: [matching], timeout: 10) == .completed
            if !matched { attachEditor(in: app, name: "navigation-camera-mismatch-" + theme) }
            XCTAssertTrue(matched, "Expected camera \(expected), actual \(status.exists ? readout() : "missing camera readout")")
        }
        func tile(_ label: String, panel: String = "toolbar") -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-" + panel + "-", label)).firstMatch
        }
        func activateCommand(_ label: String) {
            let button = tile(label, panel: "commands")
            XCTAssertTrue(button.waitForExistence(timeout: 10))
            let scroll = app.scrollViews.containing(.button, identifier: button.identifier).firstMatch
            if scroll.exists { revealEditorControl(button, in: scroll) }
            workspaceActivate(button)
        }
        func setCamera() {
            XCTAssertTrue(status.waitForExistence(timeout: 10))
            let frame = status.frame, window = workspaceViewport(in: app)
            let close = window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: frame.midX - window.frame.minX, dy: frame.midY - window.frame.minY))
            workspaceActivate(status)
            let menu = app.descendants(matching: .any)["zoom-menu"].firstMatch
            XCTAssertTrue(menu.waitForExistence(timeout: 10))
            for (id, text) in [("zoom", "250"), ("rotation", "30")] {
                let value = app.buttons["number-value-" + id]
                let scroll = app.scrollViews.containing(.button, identifier: value.identifier).firstMatch
                if scroll.exists { revealEditorControl(value, in: scroll) }
                workspaceActivate(value)
                let entry = app.textFields["number-entry-" + id]
                XCTAssertTrue(entry.waitForExistence(timeout: 5))
                entry.typeKey("a", modifierFlags: .command); entry.typeText(text + "\n")
            }
            close.clickOrTap()
            XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
            expectCamera("250% · 30°")
        }
        func navigation(_ label: String, header: Bool) -> XCUIElement {
            header ? app.buttons["header-tool-hand"] : tile(label)
        }
        func chooseNavigation(_ label: String, current: String, header: Bool) {
            let button = navigation(current, header: header)
            XCTAssertTrue(button.waitForExistence(timeout: 10))
            if !header {
                let scroll = app.scrollViews.containing(.button, identifier: button.identifier).firstMatch
                if scroll.exists { revealEditorControl(button, in: scroll) }
            }
            #if os(macOS)
            button.rightClick()
            if header {
                let items = app.menuItems.matching(identifier: label)
                expectation(for: NSPredicate { _, _ in items.allElementsBoundByIndex.contains { $0.isHittable } }, evaluatedWith: app)
                waitForExpectations(timeout: 10)
                guard let item = items.allElementsBoundByIndex.first(where: { $0.isHittable }) else {
                    XCTFail("The presented native menu must offer \(label)"); return
                }
                item.click()
            } else { workspaceActivate(app.buttons["menu-action-" + label]) }
            #else
            button.press(forDuration: 0.8)
            workspaceActivate(app.buttons["menu-action-" + label])
            #endif
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: navigation(label, header: header))
            waitForExpectations(timeout: 10)
        }
        activateCommand("Reset view")
        XCTAssertTrue(status.waitForExistence(timeout: 10))
        let reset = readout(), painted = editorPixels(in: app)
        XCTAssertLessThan(Int(painted[0]), Int(painted[2]), "The authored fill must be visible")
        var current = "Hand"
        for header in [false, true] {
            for label in ["Hand", "Zoom", "Rotate view"] {
                chooseNavigation(label, current: current, header: header); current = label
                workspaceActivate(flip)
                expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: flip)
                waitForExpectations(timeout: 5)
                setCamera()
                var expected = label == "Zoom" ? "100% · 30°" : "250% · 0°"
                if label == "Hand" {
                    activateCommand("Fit canvas")
                    expectation(for: NSPredicate { _, _ in readout().hasSuffix(" 30°") && readout() != "250% · 30°" }, evaluatedWith: status)
                    waitForExpectations(timeout: 10)
                    expected = readout()
                    setCamera()
                }
                let button = navigation(label, header: header)
                #if os(macOS)
                button.doubleClick()
                #else
                button.doubleTap()
                #endif
                expectCamera(expected)
                XCTAssertTrue(button.isSelected)
                XCTAssertTrue(flip.isSelected, "Navigation double activation preserves the mirrored view")
                XCTAssertTrue(drawer.waitForNonExistence(timeout: 10), "Double activation closes tool settings")
                if header { workspaceActivate(app.buttons["header-tool-reset_view"]) }
                else { activateCommand("Reset view") }
                expectCamera(reset)
                expectation(for: NSPredicate(format: "selected == NO"), evaluatedWith: flip)
                waitForExpectations(timeout: 5)
                expectPixels(painted, in: app)
                attachEditor(in: app, name: "navigation-\(header ? "header" : "toolbar")-\(label)-\(theme)")
            }
        }
        workspaceActivate(app.descendants(matching: .any)["zen-button"].firstMatch)
        XCTAssertTrue(status.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.buttons["header-tool-hand"].waitForNonExistence(timeout: 10))
        workspaceActivate(app.descendants(matching: .any)["zen-button"].firstMatch)
        XCTAssertTrue(status.waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["header-tool-hand"].waitForExistence(timeout: 10))
        expectCamera(reset); expectPixels(painted, in: app)
        editorHistory("Undo", in: app); expectPixels(painted, in: app)
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app).allSatisfy { $0 == 255 } }, evaluatedWith: canvas)
        waitForExpectations(timeout: 10)
        let paper = editorPixels(in: app)
        editorHistory("Undo", in: app); expectPixels(paper, in: app)
        let undo = tile("Undo", panel: "commands")
        expectation(for: NSPredicate(format: "enabled == NO"), evaluatedWith: undo)
        waitForExpectations(timeout: 10)
        editorHistory("Redo", in: app); expectPixels(paper, in: app)
        editorHistory("Redo", in: app); expectPixels(painted, in: app)
        editorHistory("Redo", in: app); expectPixels(painted, in: app)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkNavigationDrawingCycle(in app: XCUIApplication, theme: String) throws {
        var actions: [[String: Any]] = [
            ["type": "set_theme", "theme": theme],
            ["type": "workspace_manager", "command": ["type": "switch", "id": "builtin:workspace:illustrator"]],
            ["type": "customize", "action": ["type": "insert_tools", "panel": "commands", "before": NSNull()]]
        ]
        for command in ["new_document", "next_drawing", "previous_drawing", "drawings"] {
            actions.append(["type": "customize", "action": ["type": "picker_select",
                "control": ["kind": "command", "command": command], "selected": true]])
        }
        actions.append(["type": "customize", "action": ["type": "confirm_tools"]])
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        func command(_ label: String) {
            let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-commands-", label)).firstMatch
            XCTAssertTrue(button.waitForExistence(timeout: 10))
            let scroll = app.scrollViews.containing(.button, identifier: button.identifier).firstMatch
            if scroll.exists { revealEditorControl(button, in: scroll) }
            workspaceActivate(button)
        }
        command("New…")
        let create = app.buttons["new-document-create"]
        XCTAssertTrue(create.waitForExistence(timeout: 15)); workspaceActivate(create)
        XCTAssertTrue(create.waitForNonExistence(timeout: 60))
        func expectDrawing(_ id: Int) {
            command("Drawings…")
            let selector = app.descendants(matching: .any)["drawing-selector"].firstMatch
            XCTAssertTrue(selector.waitForExistence(timeout: 15))
            for candidate in [1, 2] {
                let row = selector.buttons["drawing-tab-\(candidate)"].firstMatch
                XCTAssertTrue(row.waitForExistence(timeout: 10))
                XCTAssertEqual(row.isSelected, candidate == id)
            }
            workspaceActivate(app.buttons["Done"].firstMatch)
            XCTAssertTrue(selector.waitForNonExistence(timeout: 10))
        }
        expectDrawing(2)
        for _ in 0..<2 {
            command("Next drawing"); expectDrawing(1)
            command("Previous drawing"); expectDrawing(2)
        }
        #if os(macOS)
        app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [.control]); expectDrawing(1)
        app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [.control, .shift]); expectDrawing(2)
        #endif
        attachEditor(in: app, name: "navigation-drawing-cycle-\(theme)")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
