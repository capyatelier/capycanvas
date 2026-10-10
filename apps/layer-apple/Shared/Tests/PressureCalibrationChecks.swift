import XCTest

extension XCTestCase {
    @MainActor func checkPressureCalibration(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"reveal","id":"pen_pressure"}}]"#
        app.launch()
        let panel = app.descendants(matching: .any)["pen-pressure-dialog"].firstMatch
        func open() {
            let adjust = app.buttons["preference-pen_pressure"]
            XCTAssertTrue(adjust.waitForExistence(timeout: 20)); workspaceActivate(adjust)
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 10))
            XCTAssertTrue(panel.waitForExistence(timeout: 10))
        }
        func reopen() {
            workspaceActivate(app.buttons["settings-button"])
            let input = app.staticTexts["settings-page-input"]
            XCTAssertTrue(input.waitForExistence(timeout: 10)); workspaceActivate(input)
            let adjust = app.buttons["preference-pen_pressure"]
            revealEditorControl(adjust, in: app.scrollViews.firstMatch); open()
        }
        open()
        let graph = panel.descendants(matching: .any)["effect-curve"].firstMatch
        func count(_ n: Int) {
            expectation(for: NSPredicate(format: "value == %@", String(n)), evaluatedWith: graph)
            waitForExpectations(timeout: 5)
        }
        count(3)
        XCTAssertFalse(panel.textFields.firstMatch.exists)
        let original = panel.frame
        let title = panel.staticTexts["Pen pressure"].firstMatch
        let press = title.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        press.press(forDuration: 0.05, thenDragTo: press.withOffset(CGVector(dx: -100, dy: 30)), withVelocity: .slow, thenHoldForDuration: 0.05)
        XCTAssertLessThan(panel.frame.minX, original.minX - 50)
        let point = graph.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5))
        point.tap(); count(4)
        point.press(forDuration: 0.05, thenDragTo: graph.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: -0.3)), withVelocity: .slow, thenHoldForDuration: 0.05)
        count(3)
        workspaceActivate(app.buttons["pen-pressure-reset"]); count(3)
        workspaceActivate(app.buttons["pen-pressure-firmer"])
        workspaceActivate(app.buttons["pen-pressure-lighter"])
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas); waitForExpectations(timeout: 30)
        #if os(macOS)
        let before = editorPixels(in: app)
        let stroke = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.25, dy: 0.8))
        stroke.press(forDuration: 0.05, thenDragTo: stroke.withOffset(CGVector(dx: 80, dy: 0)), withVelocity: .slow, thenHoldForDuration: 0.05)
        XCTAssertNotEqual(editorPixels(in: app), before)
        #endif
        attachEditor(in: app, name: "pen-pressure-\(theme)")
        for close in ["cancel", "close", "apply"] {
            workspaceActivate(app.buttons["pen-pressure-" + close]); XCTAssertTrue(panel.waitForNonExistence(timeout: 5))
            reopen(); count(3)
        }
        workspaceActivate(app.buttons["pen-pressure-close"])
        XCTAssertTrue(panel.waitForNonExistence(timeout: 5))
    }
}
