import XCTest

extension XCTestCase {
    @MainActor func checkCanvasSize(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Image", id: "canvas_size", label: "Canvas Size…")
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

extension XCTestCase {
    @MainActor func checkImageSize(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Image", id: "image_size", label: "Image Size…")
        let apply = app.buttons["image-size-apply"], message = app.staticTexts["image-size-message"]
        let title = editorDocumentTitle(in: app)
        func shown(_ element: XCUIElement) -> String { (element.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? element.label }
        XCTAssertTrue(apply.waitForExistence(timeout: 15), "Image Size opens its dialog")
        XCTAssertFalse(apply.isEnabled, "The current size has nothing to apply")
        XCTAssertTrue(app.descendants(matching: .any)["image-size-resample"].exists)
        workspaceActivate(app.buttons["number-decrease-image-size-width"])
        expectation(for: NSPredicate { _, _ in shown(message).contains("2047") }, evaluatedWith: message)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(apply.isEnabled)
        attachEditor(in: app, name: "image-size")
        workspaceActivate(apply)
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10), "Apply closes the dialog")
        expectation(for: NSPredicate(format: "label CONTAINS %@", "2047 × 1535"), evaluatedWith: title); waitForExpectations(timeout: 20)
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate(format: "label CONTAINS %@", "2048 × 1536"), evaluatedWith: title); waitForExpectations(timeout: 20)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkFrequencySeparation(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        app.launch(); capturePaintEditor(in: app)
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let before = rows.count
        editorMenu(in: app, menu: "Filter", id: "frequency_separation", label: "Frequency Separation…")
        let panel = app.descendants(matching: .any)["frequency-separation-panel"].firstMatch
        XCTAssertTrue(panel.waitForExistence(timeout: 15), "Frequency Separation opens its panel over the canvas")
        workspaceActivate(app.buttons["number-increase-frequency-separation-value"])
        attachEditor(in: app, name: "frequency-separation")
        workspaceActivate(app.buttons["frequency-separation-apply"])
        XCTAssertTrue(panel.waitForNonExistence(timeout: 20), "Apply closes the panel")
        expectation(for: NSPredicate { _, _ in rows.count > before }, evaluatedWith: app); waitForExpectations(timeout: 20)
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in rows.count == before }, evaluatedWith: app); waitForExpectations(timeout: 20)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
