import XCTest

extension XCTestCase {
    @MainActor func checkZoomReadout(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let status = app.buttons["camera-status"]
        let menu = app.descendants(matching: .any)["zoom-menu"].firstMatch
        func readout() -> String { (status.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? status.label }
        func expectReadout(_ prefix: String) {
            expectation(for: NSPredicate { _, _ in readout().hasPrefix(prefix) }, evaluatedWith: status)
            waitForExpectations(timeout: 10)
        }
        XCTAssertTrue(status.waitForExistence(timeout: 10))
        workspaceActivate(status)
        XCTAssertTrue(menu.waitForExistence(timeout: 10), "The readout opens the zoom menu")
        attachEditor(in: app, name: "zoom-readout-menu")
        workspaceActivate(menu.buttons["command-actual_pixels"])
        expectReadout("100%")
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5), "Choosing a zoom closes the menu")

        workspaceActivate(status)
        let value = app.buttons["number-value-zoom"], entry = app.textFields["number-entry-zoom"]
        XCTAssertTrue(value.waitForExistence(timeout: 10), "The menu offers a typed zoom")
        workspaceActivate(value)
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeKey("a", modifierFlags: .command); entry.typeText("250\n")
        expectation(for: NSPredicate { _, _ in ((value.value as? String) ?? value.label).contains("250") }, evaluatedWith: value)
        waitForExpectations(timeout: 10)
        workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.12)).clickOrTap()
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5), "A tap outside closes the zoom menu")
        expectReadout("250%")

        workspaceActivate(status)
        let rotation = app.buttons["number-value-rotation"], rotate = app.buttons["zoom-rotate_right"]
        let rotationMenu = app.descendants(matching: .any)["rotation-menu"].firstMatch
        XCTAssertTrue(rotation.waitForExistence(timeout: 10), "The menu offers the rotation slider")
        XCTAssertTrue(rotate.exists, "The menu repeats Navigator's buttons")
        XCTAssertLessThan(value.frame.maxY, menu.frame.minY + 1)
        XCTAssertLessThan(menu.frame.maxY, rotation.frame.minY + 1, "Zoom controls stay together above rotation")
        XCTAssertLessThan(rotation.frame.maxY, rotationMenu.frame.minY + 1)
        XCTAssertLessThan(rotationMenu.frame.maxY, rotate.frame.minY + 1, "Navigator's buttons close the menu")
        attachEditor(in: app, name: "zoom-readout-rotation")
        workspaceActivate(rotate)
        expectation(for: NSPredicate { _, _ in !readout().hasSuffix(" 0°") }, evaluatedWith: status)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(menu.exists, "Navigation buttons keep the menu open")
        let lock = app.buttons["menu-action-Lock rotation"]
        XCTAssertFalse(lock.isSelected)
        workspaceActivate(lock)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
        workspaceActivate(status)
        XCTAssertTrue(lock.waitForExistence(timeout: 10))
        XCTAssertTrue(lock.isSelected, "The lock shows its current state")
        workspaceActivate(app.buttons["menu-action-Reset rotation"])
        expectation(for: NSPredicate { _, _ in readout().hasSuffix(" 0°") }, evaluatedWith: status)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
