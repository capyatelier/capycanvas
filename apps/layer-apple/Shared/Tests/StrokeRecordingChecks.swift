import XCTest

extension XCTestCase {
    @MainActor func checkStrokeRecording(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let tab = app.buttons["panel-tab-stats"]
        XCTAssertTrue(tab.waitForExistence(timeout: 10), "Paint keeps Diagnostics beside Tool Set")
        workspaceActivate(tab)
        let button = app.buttons["stroke-recording"]
        XCTAssertTrue(button.waitForExistence(timeout: 10))
        revealEditorControl(button, in: app.scrollViews.containing(.button, identifier: "stroke-recording").firstMatch)
        func expectLabel(_ label: String) {
            expectation(for: NSPredicate(format: "label == %@", label), evaluatedWith: button)
            waitForExpectations(timeout: 10)
        }
        expectLabel("Start stroke recording")
        workspaceActivate(button)
        expectLabel("Stop stroke recording")
        let canvas = workspaceViewport(in: app)
        let start = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.45, dy: 0.5))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 80, dy: 20)))
        #else
        start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 80, dy: 20)))
        #endif
        workspaceActivate(button)
        #if os(macOS)
        let panel = app.sheets.firstMatch
        XCTAssertTrue(panel.waitForExistence(timeout: 10), "Stopping opens the save panel")
        panel.buttons["Cancel"].click()
        XCTAssertTrue(panel.waitForNonExistence(timeout: 10))
        expectLabel("Save stroke recording")
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
