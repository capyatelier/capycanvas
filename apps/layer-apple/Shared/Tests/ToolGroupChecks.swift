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
    @MainActor func checkEncloseFillTool(in app: XCUIApplication) {
        app.launch(); capturePaintEditor(in: app)
        editorTool("Fill", in: app)
        editorChoice("Lasso fill", group: true, in: app)
        editorChoice("Enclose and Fill", in: app)
        let tile = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", "Enclose and Fill")).firstMatch
        XCTAssertTrue(tile.waitForExistence(timeout: 10), "The Fill tile follows Enclose and Fill")
        for id in ["tolerance", "smoothing"] {
            XCTAssertTrue(app.buttons["number-value-tool-" + id].waitForExistence(timeout: 10), "Enclose and Fill shares Fill's \(id)")
        }
        for id in ["gap_closing", "expansion"] {
            XCTAssertTrue(app.textFields["number-entry-tool-" + id].waitForExistence(timeout: 10), "Enclose and Fill shares Fill's \(id)")
        }
        let reference = app.buttons["tool-action-selection_reference"], visible = app.buttons["tool-action-selection_visible"]
        revealEditorControl(reference, in: app.scrollViews.containing(.button, identifier: reference.identifier).firstMatch)
        XCTAssertTrue(reference.isSelected, "Enclose and Fill samples reference layers by default")
        workspaceActivate(visible)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: visible)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(tile.exists, "Changing the source keeps Enclose and Fill")
        attachEditor(in: app, name: "enclose-fill-tool")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
