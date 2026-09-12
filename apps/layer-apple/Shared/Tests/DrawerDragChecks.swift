import XCTest

extension XCTestCase {
    @MainActor func checkDrawerDragAndDock(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"move_panel","panel":"toolbar","target":{"kind":"tab","group":6},"viewport":[1376,1032]},{"type":"customize","action":{"type":"set_column_collapsed","group":6,"collapsed":true}}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        func activate(_ control: XCUIElement) {
            workspaceActivate(control)
        }
        func drag(_ control: XCUIElement, to destination: XCUICoordinate) {
            XCTAssertTrue(control.waitForExistence(timeout: 10))
            let start = control.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            #if os(macOS)
            start.click(forDuration: 0.1, thenDragTo: destination)
            #else
            start.press(forDuration: 0.1, thenDragTo: destination)
            #endif
        }
        activate(app.buttons["column-icon-toolbar"])
        let toolbar = app.buttons["drawer-tab-toolbar"], brushes = app.buttons["drawer-tab-brushes"]
        XCTAssertTrue(toolbar.waitForExistence(timeout: 10)); XCTAssertTrue(brushes.exists)
        XCTAssertGreaterThan(toolbar.frame.midX, brushes.frame.midX)
        drag(toolbar, to: brushes.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: 0.5)))
        expectation(for: NSPredicate { _, _ in toolbar.exists && brushes.exists && toolbar.frame.midX < brushes.frame.midX }, evaluatedWith: toolbar)
        waitForExpectations(timeout: 10)

        let viewport = workspaceViewport(in: app)
        let floatingPoint = viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.58, dy: 0.5))
        drag(toolbar, to: floatingPoint)
        // A standalone toolbar uses its shared footer grip, without a tab bar.
        let floating = app.descendants(matching: .any)["toolbar-options-toolbar"].firstMatch
        XCTAssertTrue(floating.waitForExistence(timeout: 10), "A drawer tab must tear off into a live floating panel")
        XCTAssertTrue(toolbar.waitForNonExistence(timeout: 10))
        if !brushes.exists { activate(app.buttons["column-icon-brushes"]) }
        XCTAssertTrue(brushes.waitForExistence(timeout: 10))
        drag(floating, to: brushes.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)))
        XCTAssertTrue(toolbar.waitForExistence(timeout: 10), "Dropping on an open drawer must join its dock group")
        XCTAssertTrue(floating.waitForNonExistence(timeout: 10))

        let grip = app.descendants(matching: .any)["drawer-group-options-6"].firstMatch
        drag(grip, to: floatingPoint)
        XCTAssertTrue(app.buttons["panel-tab-toolbar"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.buttons["panel-tab-brushes"].waitForExistence(timeout: 10))
        XCTAssertEqual(app.buttons["panel-tab-toolbar"].frame.minY, app.buttons["panel-tab-brushes"].frame.minY, accuracy: 1)
        XCTAssertTrue(app.descendants(matching: .any)["column-drawer-4"].firstMatch.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #if os(macOS)
        let shot = app.windows.firstMatch.screenshot()
        #else
        let shot = XCUIScreen.main.screenshot()
        #endif
        let capture = XCTAttachment(screenshot: shot)
        capture.name = "drawer-group-after-drag"; capture.lifetime = .keepAlways; add(capture)
    }
}
