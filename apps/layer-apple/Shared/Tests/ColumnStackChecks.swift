import XCTest

extension XCTestCase {
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
        XCTAssertEqual(layerGroup.frame.width, originalWidth, accuracy: 1)
        workspaceActivate(history("Redo Layout Change"))
        XCTAssertEqual(layerGroup.frame.width, resizedWidth, accuracy: 1)
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
