import XCTest

extension XCTestCase {
    @MainActor func checkCanvasSize(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Edit", id: "canvas_size", label: "Canvas Size…", submenu: "Image")
        let apply = app.buttons["canvas-size-apply"], message = app.staticTexts["canvas-size-message"]
        let title = editorDocumentTitle(in: app)
        func expect(_ element: XCUIElement, _ format: String, _ argument: String) {
            expectation(for: NSPredicate(format: format, argument), evaluatedWith: element); waitForExpectations(timeout: 10)
        }
        func shown(_ element: XCUIElement) -> String { (element.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? element.label }
        XCTAssertTrue(apply.waitForExistence(timeout: 15), "Canvas Size opens its dialog")
        XCTAssertFalse(apply.isEnabled, "The current size has nothing to apply")
        XCTAssertEqual(shown(message), "Current size: 2048 × 1536 px")
        let topLeft = app.buttons["canvas-size-anchor-top_left"]
        workspaceActivate(topLeft)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: topLeft); waitForExpectations(timeout: 5)
        workspaceActivate(app.buttons["number-decrease-canvas-size-width"])
        expectation(for: NSPredicate { _, _ in shown(message) == "New size: 2047 × 1536 px" }, evaluatedWith: message)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "canvas-size")
        workspaceActivate(apply)
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10), "Apply closes the dialog")
        expect(title, "label CONTAINS %@", "2047 × 1536")
        editorHistory("Undo", in: app)
        expect(title, "label CONTAINS %@", "2048 × 1536")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
