import XCTest

final class EditorLaunchTests: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }

    @MainActor func testColorControls() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = XCUIApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"preferences","action":{"type":"edit","id":"theme","value":1}},{"type":"customize","action":{"type":"set_panel_visible","panel":"color","visible":true}},{"type":"set_color","rgba":[1,0,0,1]}]"#
        app.launch()
        checkColorControls(in: app) { mode, wheel in
            attachColorFixture(name: "ipad-color-" + mode, space: mode,
                screenshot: XCUIScreen.main.screenshot(), viewport: app.frame, wheel: wheel)
        }
    }

    @MainActor func testNumericToolControls() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = XCUIApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"customize","action":{"type":"set_panel_visible","panel":"tool_settings","visible":true}}]"#
        app.launch()
        checkNumericToolControls(in: app)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "ipad-tool-controls"; shot.lifetime = .keepAlways; add(shot)
    }

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
        XCTAssertGreaterThan(app.frame.width, app.frame.height, "Landscape capture must use a settled landscape window")
        XCTAssertEqual(canvas.frame.width, app.frame.width, accuracy: 1)
        XCTAssertEqual(canvas.frame.height, app.frame.height, accuracy: 1)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "ipad-editor-initial"
        shot.lifetime = .keepAlways
        add(shot)
        checkLayerControls(in: app)
        let layers = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        layers.name = "ipad-layer-added"; layers.lifetime = .keepAlways; add(layers)
    }
}
