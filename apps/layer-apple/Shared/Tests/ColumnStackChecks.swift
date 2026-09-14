import XCTest

extension XCTestCase {
    @MainActor func checkDefaultWorkspaceColumns(in app: XCUIApplication, photo: Bool = false) {
        let workspace = photo ? "photographer" : "illustrator"
        app.launchEnvironment.removeValue(forKey: "CAPY_DISABLE_PERSISTENCE")
        app.launchEnvironment["CAPY_PERSISTENCE_NAMESPACE"] = UUID().uuidString
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = "[{\"type\":\"set_theme\",\"theme\":\"light\"},{\"type\":\"workspace_manager\",\"command\":{\"type\":\"switch\",\"id\":\"builtin:workspace:\(workspace)\"}}]"
        func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        let scene = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "editor-scene-")).firstMatch
        var previousScene: String?
        for launch in 0..<2 {
            app.launch()
            let selected = app.buttons["workspace-switch-builtin:workspace:" + workspace]
            XCTAssertTrue(selected.waitForExistence(timeout: 30))
            XCTAssertTrue(scene.exists)
            // AppKit can create a new scene after XCTest terminates the app.
            // Existing scenes must restore their binding; new scenes can open
            // the same saved workspace through the ordinary switcher.
            if launch == 1 && scene.identifier != previousScene { workspaceActivate(selected) }
            let strip = element(photo ? "collapsed-column-4" : "collapsed-column-12")
            let color = element(photo ? "workspace-group-14" : "workspace-group-10"), properties = element("workspace-group-15"), layers = element("workspace-group-16")
            XCTAssertTrue(strip.waitForExistence(timeout: 30))
            XCTAssertTrue(color.waitForExistence(timeout: 10))
            XCTAssertTrue(properties.exists && layers.exists)
            if photo {
                XCTAssertLessThan(strip.frame.maxX, color.frame.minX)
                XCTAssertEqual(color.frame.minX, properties.frame.minX, accuracy: 1)
                XCTAssertLessThan(color.frame.maxY, properties.frame.minY)
            } else {
                XCTAssertLessThan(color.frame.maxX, properties.frame.minX)
                XCTAssertLessThan(properties.frame.maxX, strip.frame.minX)
                XCTAssertTrue(element("workspace-group-6").exists)
                XCTAssertTrue(app.buttons["panel-tab-navigator"].exists)
            }
            XCTAssertEqual(properties.frame.minX, layers.frame.minX, accuracy: 1)
            XCTAssertLessThan(properties.frame.maxY, layers.frame.minY)
            for panel in ["color", "stats", "properties", "adjustments", "layers"] {
                XCTAssertTrue(app.buttons["panel-tab-" + panel].firstMatch.exists)
            }
            // Photo has a short Color group; Paint puts Color below Tools.
            // The wheel and corner controls must fit without scrolling, as on Web.
            let wheel = element("color-wheel")
            XCTAssertTrue(wheel.waitForExistence(timeout: 10))
            XCTAssertEqual(wheel.frame.width, wheel.frame.height, accuracy: 1)
            for id in ["color-wheel", "color-readout", "color-foreground", "color-background",
                       "color-transparent", "color-swap", "color-shape-square", "color-shape-triangle"] {
                let control = element(id)
                XCTAssertTrue(control.exists, id)
                XCTAssertTrue(color.frame.insetBy(dx: -1, dy: -1).contains(control.frame), "Clipped Color control: \(id)")
            }
            if photo {
                let brushes = app.buttons["column-icon-brushes"], tool = app.buttons["column-icon-tool_settings"], navigator = app.buttons["column-icon-navigator"]
                XCTAssertLessThan(brushes.frame.minY, tool.frame.minY)
                XCTAssertLessThan(tool.frame.minY, navigator.frame.minY)
                XCTAssertFalse(element("workspace-group-6").exists)
                workspaceActivate(brushes)
                XCTAssertTrue(element("workspace-group-6").waitForExistence(timeout: 10))
                XCTAssertTrue(color.exists && properties.exists && layers.exists)
                workspaceActivate(brushes)
                XCTAssertTrue(element("workspace-group-6").waitForNonExistence(timeout: 10))
            } else {
                let icon = app.buttons["column-icon-layers"]
                workspaceActivate(icon)
                XCTAssertTrue(layers.waitForNonExistence(timeout: 10))
                workspaceActivate(icon)
                XCTAssertTrue(layers.waitForExistence(timeout: 10))
            }
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            #if os(macOS)
            let shot = app.windows.firstMatch.screenshot()
            #else
            let shot = XCUIScreen.main.screenshot()
            #endif
            let capture = XCTAttachment(screenshot: shot)
            capture.name = "\(workspace)-default-columns-\(launch)"; capture.lifetime = .keepAlways; add(capture)
            previousScene = scene.identifier
            app.terminate()
            app.launchEnvironment.removeValue(forKey: "CAPY_INITIAL_ACTIONS")
        }
    }

    @MainActor func checkColumnStacks(in app: XCUIApplication, theme: String = "light") {
        let actions: [[String: Any]] = [
            ["type":"set_theme", "theme":theme],
            ["type":"customize", "action":["type":"set_column_collapsed", "group":6, "collapsed":true]],
            ["type":"customize", "action":["type":"set_column_collapsed", "group":16, "collapsed":true]],
            ["type":"customize", "action":["type":"set_column_drawers", "column":4, "drawers":false]],
            ["type":"customize", "action":["type":"set_column_drawers", "column":12, "drawers":false]],
            ["type":"customize", "action":["type":"insert_tools", "panel":"toolbar", "before":1]],
            ["type":"customize", "action":["type":"picker_select", "control":["kind":"command", "command":"undo_workspace"], "selected":true]],
            ["type":"customize", "action":["type":"picker_select", "control":["kind":"command", "command":"redo_workspace"], "selected":true]],
            ["type":"customize", "action":["type":"confirm_tools"]],
        ]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try! JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        let left = element("collapsed-column-4"), right = element("collapsed-column-12")
        let leftGrip = element("column-grip-4"), rightGrip = element("column-grip-12")
        XCTAssertTrue(leftGrip.waitForExistence(timeout: 25))
        XCTAssertTrue(rightGrip.exists)
        XCTAssertGreaterThan(right.frame.minX - left.frame.minX, 500)
        rightGrip.press(forDuration: 0.05, thenDragTo: leftGrip)
        expectation(for: NSPredicate { _, _ in
            left.exists && right.exists && abs(left.frame.minX - right.frame.minX) < 1
                && right.frame.minY > left.frame.minY
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)

        let brushes = app.buttons["column-icon-brushes"], layers = app.buttons["column-icon-layers"]
        let brushGroup = element("workspace-group-6"), layerGroup = element("workspace-group-16")
        workspaceActivate(brushes)
        XCTAssertTrue(brushGroup.waitForExistence(timeout: 10))
        XCTAssertTrue(brushes.isSelected)
        XCTAssertTrue(app.buttons["column-icon-tool_settings"].isSelected, "Every visible tab group has a selected sidebar icon")
        XCTAssertFalse(element("column-drawer-4").exists, "Whole-column mode reuses ordinary dock groups")
        workspaceActivate(layers)
        XCTAssertTrue(layerGroup.waitForExistence(timeout: 10))
        XCTAssertTrue(brushGroup.waitForNonExistence(timeout: 10))
        XCTAssertTrue(layers.isSelected)
        XCTAssertFalse(brushes.isSelected)
        XCTAssertTrue(app.buttons["layer-New layer"].exists)
        let originalWidth = layerGroup.frame.width
        let handles = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-divider-"))
        guard let handle = handles.allElementsBoundByIndex.first(where: {
            $0.frame.height > $0.frame.width && abs($0.frame.minX - layerGroup.frame.maxX - 6) < 2
        }) else { XCTFail("The open member must expose its canvas-facing resize edge"); return }
        let start = handle.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 50, dy: 0)))
        expectation(for: NSPredicate { _, _ in layerGroup.frame.width > originalWidth + 25 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let resizedWidth = layerGroup.frame.width
        func history(_ name: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-toolbar-", name)).firstMatch
        }
        workspaceActivate(history("Undo Layout Change"))
        expectation(for: NSPredicate { _, _ in abs(layerGroup.frame.width - originalWidth) <= 1 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        workspaceActivate(history("Redo Layout Change"))
        expectation(for: NSPredicate { _, _ in abs(layerGroup.frame.width - resizedWidth) <= 1 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        #if os(macOS)
        let capture = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        #else
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        #endif
        capture.name = "stacked-columns-open-member-\(theme)"; capture.lifetime = .keepAlways; add(capture)

        // Stack preferences are the shared context menu on each member's grip.
        workspaceActivate(leftGrip)
        workspaceActivate(app.buttons["Auto-hide"].firstMatch)
        let outside = workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.65))
        #if os(macOS)
        outside.click()
        #else
        outside.tap()
        #endif
        XCTAssertTrue(layerGroup.waitForNonExistence(timeout: 10))
        workspaceActivate(leftGrip)
        workspaceActivate(app.buttons["Open individual panels"].firstMatch)
        workspaceActivate(brushes)
        let drawer = element("column-drawer-4")
        XCTAssertTrue(drawer.waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["drawer-tab-brushes"].exists)
        XCTAssertFalse(brushGroup.exists)
        workspaceActivate(brushes)
        XCTAssertTrue(drawer.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
