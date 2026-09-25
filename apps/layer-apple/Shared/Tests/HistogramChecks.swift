import XCTest

extension XCTestCase {
    @MainActor func checkNativeHistogram(in app: XCUIApplication) throws {
        #if os(macOS)
        let actions: [[String:Any]] = [
            ["type":"set_theme","theme":"light"],
            ["type":"customize","action":["type":"insert_tools","panel":"commands","before":NSNull()]],
            ["type":"customize","action":["type":"picker_select","control":["kind":"command","command":"histogram"],"selected":true]],
            ["type":"customize","action":["type":"confirm_tools"]],
            ["type":"color","action":["op":"definition","color":["space":"Srgb","rgba":[0.7,0.3,0.15,1]]]]]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)!
        app.launch(); capturePaintEditor(in: app)
        // Keep this isolated window clear of the unrelated system iCloud prompt.
        let window = app.windows.firstMatch
        let corner = window.coordinate(withNormalizedOffset: CGVector(dx: 1, dy: 1)).withOffset(CGVector(dx: -2, dy: -2))
        corner.press(forDuration: 0.1, thenDragTo: corner.withOffset(CGVector(dx: -320, dy: 0)))
        expectation(for: NSPredicate { _,_ in window.frame.width < 950 }, evaluatedWith: window)
        waitForExpectations(timeout: 10)
        func text(_ element: XCUIElement) -> String { element.value as? String ?? element.label }
        func open() {
            let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", "Histogram…")).firstMatch
            XCTAssertTrue(button.waitForExistence(timeout: 10)); workspaceActivate(button)
        }
        let status = app.staticTexts["histogram-status"]
        func ready() {
            expectation(for: NSPredicate { _,_ in text(status) == "Current committed drawing" }, evaluatedWith: status)
            waitForExpectations(timeout: 60)
        }
        open(); ready()
        XCTAssertTrue(app.buttons["histogram-close"].isHittable)
        workspaceActivate(app.buttons["histogram-details"])
        XCTAssertTrue(app.staticTexts["histogram-channel-0"].waitForExistence(timeout: 10), "Details list each channel")
        let before = text(app.staticTexts["histogram-channel-0"])
        // The inspector remains open while the real editor changes artwork.
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectation(for: NSPredicate { _,_ in text(app.staticTexts["histogram-channel-0"]) != before }, evaluatedWith: app)
        waitForExpectations(timeout: 60); ready()
        attachEditor(in: app, name: "histogram-rgb-nonmodal")
        workspaceActivate(app.checkBoxes["Log counts"])
        workspaceActivate(app.popUpButtons["histogram-channel"])
        workspaceActivate(app.menuItems["Luminance"].firstMatch)
        XCTAssertTrue(app.staticTexts["histogram-channel-3"].waitForExistence(timeout: 10))
        attachEditor(in: app, name: "histogram-luminance-log")
        workspaceActivate(app.checkBoxes["Auto update"])
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _,_ in text(status).contains("showing previous inspection") }, evaluatedWith: status)
        waitForExpectations(timeout: 15)
        workspaceActivate(app.buttons["histogram-refresh"]); ready()
        workspaceActivate(app.buttons["histogram-close"])
        XCTAssertTrue(status.waitForNonExistence(timeout: 10))
        editorTool("Eyedropper", in: app)
        for (setting, label) in [("size", "Single pixel"), ("size", "5 px circle"), ("size", "15 px circle"), ("size", "51 px circle"),
            ("size", "101 px circle"), ("source", "Selected layer"), ("source", "Visible color")] {
            let menu = app.popUpButtons["picker-setting-" + setting]
            XCTAssertTrue(menu.waitForExistence(timeout: 10))
            workspaceActivate(menu)
            workspaceActivate(app.menuItems[label].firstMatch)
            expectation(for: NSPredicate(format: "value == %@", label), evaluatedWith: menu); waitForExpectations(timeout: 10)
        }
        attachEditor(in: app, name: "eyedropper-sample-areas")
        editorTool("Eyedropper", in: app)
        open(); ready(); workspaceActivate(app.buttons["histogram-close"])
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #endif
    }
}
