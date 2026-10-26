import XCTest

extension XCTestCase {
    @MainActor func editorToolActionChoice(_ command: String, group: String = "selection-source", in app: XCUIApplication) {
        let choice = app.buttons["tool-action-group-" + group]
        revealEditorControl(choice, in: app.scrollViews.containing(.button, identifier: choice.identifier).firstMatch)
        workspaceActivate(choice)
        let option = app.buttons["tool-action-" + command]
        XCTAssertTrue(option.waitForExistence(timeout: 5))
        let label = option.label
        workspaceActivate(option)
        expectation(for: NSPredicate(format: "value == %@", label), evaluatedWith: choice)
        waitForExpectations(timeout: 5)
    }

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
    @MainActor func checkEncloseFillTool(in app: XCUIApplication, theme: String = "light") {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        editorTool("Fill", in: app)
        editorChoice("Enclose and Fill", group: true, in: app)
        let tile = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", "Enclose and Fill")).firstMatch
        XCTAssertTrue(tile.waitForExistence(timeout: 10), "The Fill tile follows Enclose and Fill")
        for id in ["tolerance", "smoothing"] {
            XCTAssertTrue(app.buttons["number-value-tool-" + id].waitForExistence(timeout: 10), "Enclose and Fill shares Fill's \(id)")
        }
        for id in ["gap_closing", "expansion"] {
            XCTAssertTrue(app.textFields["number-entry-tool-" + id].waitForExistence(timeout: 10), "Enclose and Fill shares Fill's \(id)")
        }
        let source = app.buttons["tool-action-group-selection-source"]
        XCTAssertTrue(source.waitForExistence(timeout: 5))
        XCTAssertEqual(source.value as? String, "Reference layers", "Enclose and Fill samples reference layers by default")
        editorToolActionChoice("selection_visible", in: app)
        XCTAssertTrue(tile.exists, "Changing the source keeps Enclose and Fill")
        attachEditor(in: app, name: "enclose-fill-tool")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
