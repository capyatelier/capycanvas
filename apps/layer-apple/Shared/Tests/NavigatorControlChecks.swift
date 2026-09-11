import XCTest

extension XCTestCase {
    @MainActor func checkNavigatorAndDiagnostics(in app: XCUIApplication) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 10))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let overview = app.descendants(matching: .any)["navigator-overview"].firstMatch
        XCTAssertTrue(overview.waitForExistence(timeout: 10))
        expectation(for: NSPredicate(format: "value == %@", "Preview ready"), evaluatedWith: overview)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.frame.contains(overview.frame))
        let status = app.staticTexts["camera-status"]
        let original = status.label
        activate(app.buttons["navigator-zoom_in"])
        expectation(for: NSPredicate(format: "label != %@", original), evaluatedWith: status)
        waitForExpectations(timeout: 5)
        activate(app.buttons["navigator-rotate_right"])
        activate(app.buttons["navigator-flip_horizontal"])
        let start = overview.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let end = overview.coordinate(withNormalizedOffset: CGVector(dx: 0.62, dy: 0.55))
        #if os(macOS)
        start.click(forDuration: 0.1, thenDragTo: end)
        #else
        start.press(forDuration: 0.1, thenDragTo: end)
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        activate(app.buttons["panel-tab-stats"])
        let frames = app.staticTexts["stats-value-2"]
        XCTAssertTrue(frames.waitForExistence(timeout: 10))
        XCTAssertGreaterThan(Int(frames.label) ?? 0, 0)
        activate(app.buttons["panel-tab-navigator"])
        XCTAssertTrue(overview.waitForExistence(timeout: 10))
        expectation(for: NSPredicate(format: "value == %@", "Preview ready"), evaluatedWith: overview)
        waitForExpectations(timeout: 10)
        #if os(macOS)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        #else
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        #endif
        shot.name = "navigator-controls"; shot.lifetime = .keepAlways; add(shot)
    }
}
