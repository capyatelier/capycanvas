import XCTest

extension XCTestCase {
    @MainActor func checkToolGroups(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:photographer"}}]"#
        app.launch()
        func tile(_ label: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", label)).firstMatch
        }
        let rectangle = tile("Rectangle select"), ellipse = tile("Ellipse select")
        XCTAssertTrue(rectangle.waitForExistence(timeout: 30), "Photo groups the marquee tools in one tile")
        XCTAssertFalse(ellipse.exists, "The group shows only its remembered tool")
        attachEditor(in: app, name: "tool-groups-\(theme)")
        workspaceActivate(rectangle)
        expectation(for: NSPredicate { _, _ in rectangle.isSelected }, evaluatedWith: rectangle)
        waitForExpectations(timeout: 10)

        #if os(macOS)
        rectangle.rightClick()
        #else
        rectangle.press(forDuration: 0.8)
        #endif
        let choice = app.buttons["menu-action-Ellipse select"]
        XCTAssertTrue(choice.waitForExistence(timeout: 10), "Secondary click or hold offers the group's tools")
        attachEditor(in: app, name: "tool-groups-menu-\(theme)")
        workspaceActivate(choice)
        XCTAssertTrue(ellipse.waitForExistence(timeout: 10), "The tile follows the chosen tool")
        expectation(for: NSPredicate { _, _ in ellipse.isSelected }, evaluatedWith: ellipse)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(rectangle.exists)
        attachEditor(in: app, name: "tool-groups-chosen-\(theme)")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
