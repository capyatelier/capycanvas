import XCTest

extension XCTestCase {
    @MainActor func checkSwitcherVisibility(in app: XCUIApplication) {
        app.launch()
        let options = app.buttons["workspace-switcher-options"]
        let menu = app.descendants(matching: .any)["workspace-switcher-options-menu"].firstMatch
        let photo = app.buttons["workspace-switch-builtin:workspace:photographer"]
        let paint = app.buttons["workspace-switch-builtin:workspace:illustrator"]
        XCTAssertTrue(options.waitForExistence(timeout: 30))
        XCTAssertTrue(photo.waitForExistence(timeout: 10))
        XCTAssertTrue(paint.isSelected)
        XCTAssertGreaterThan(options.frame.minX, photo.frame.maxX, "Options follow the workspace choices")
        XCTAssertEqual(options.frame.width, 20, accuracy: 1)
        let title = photo.label
        let row = app.buttons["menu-action-" + title]
        workspaceActivate(options)
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertTrue(row.waitForExistence(timeout: 5)); XCTAssertTrue(row.isSelected, "Visible workspaces are checked")
        XCTAssertTrue(app.buttons["menu-action-Manage Workspaces…"].exists)
        attachEditor(in: app, name: "switcher-options")
        workspaceActivate(row)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        XCTAssertTrue(photo.waitForNonExistence(timeout: 10), "Unchecking hides the workspace from the switcher")

        #if os(macOS)
        paint.rightClick()
        let items = app.menuItems.matching(NSPredicate(format: "title == %@", title))
        expectation(for: NSPredicate { _, _ in items.allElementsBoundByIndex.contains { $0.isHittable } }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        items.allElementsBoundByIndex.first { $0.isHittable }?.click()
        #else
        paint.press(forDuration: 0.8)
        XCTAssertTrue(row.waitForExistence(timeout: 10), "A hold on a choice opens the same checklist")
        XCTAssertFalse(row.isSelected)
        workspaceActivate(row)
        #endif
        XCTAssertTrue(photo.waitForExistence(timeout: 10), "Checking shows the workspace again")
        XCTAssertTrue(paint.isSelected, "Opening the checklist does not switch workspaces")

        app.terminate(); app.launch()
        XCTAssertTrue(photo.waitForExistence(timeout: 30), "Visibility persists across restarts")
        workspaceActivate(options)
        workspaceActivate(app.buttons["menu-action-Manage Workspaces…"])
        XCTAssertTrue(app.buttons["workspace-select-builtin:workspace:photographer"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
