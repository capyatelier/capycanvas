import XCTest

final class EditorLaunchTests: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }

    @MainActor func testMetalLaunchAndEditorCapture() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = XCUIApplication()
        app.launch()
        let canvas = app.otherElements["canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        let ready = NSPredicate(format: "value == %@", "Metal ready")
        expectation(for: ready, evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let zen = app.buttons["zen-button"]
        XCTAssertTrue(zen.exists)
        XCTAssertEqual(zen.frame.width, 36, accuracy: 1)
        XCTAssertEqual(zen.frame.height, 36, accuracy: 1)
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = "ipad-editor-initial"
        shot.lifetime = .keepAlways
        add(shot)
    }
}
