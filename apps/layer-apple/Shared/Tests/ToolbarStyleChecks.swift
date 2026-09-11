import XCTest

extension XCTestCase {
    @MainActor func checkToolbarStylesAndActions(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"preferences","action":{"type":"edit","id":"show_clock","value":2}},{"type":"customize","action":{"type":"insert_tools","panel":"commands","before":null}},{"type":"customize","action":{"type":"picker_select","control":{"kind":"command","command":"zoom_in"},"selected":true}},{"type":"customize","action":{"type":"confirm_tools"}},{"type":"customize","action":{"type":"set_tile_style","panel":"commands","style":"medium_labeled"}}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let zoom = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", "Zoom in")).firstMatch
        XCTAssertTrue(zoom.waitForExistence(timeout: 15))
        XCTAssertEqual(zoom.frame.width, 108, accuracy: 1)
        XCTAssertEqual(zoom.frame.height, 54, accuracy: 1)
        #if os(macOS)
        let image = app.windows.firstMatch.screenshot()
        #else
        let image = XCUIScreen.main.screenshot()
        #endif
        let capture = XCTAttachment(screenshot: image)
        capture.name = "medium-labeled-toolbar"; capture.lifetime = .keepAlways; add(capture)
        func activate(_ control: XCUIElement) {
            XCTAssertTrue(control.waitForExistence(timeout: 10))
            #if os(macOS)
            control.click()
            #else
            control.tap()
            #endif
        }
        let camera = app.staticTexts["camera-status"]
        let usesValue = !(camera.value as? String ?? "").isEmpty
        let previous = usesValue ? camera.value as! String : camera.label
        XCTAssertFalse(previous.isEmpty, "The camera readout must provide a visible value before testing Zoom")
        activate(zoom)
        expectation(for: NSPredicate(format: (usesValue ? "value" : "label") + " != %@", previous), evaluatedWith: camera)
        waitForExpectations(timeout: 10)
        activate(app.descendants(matching: .any)["toolbar-options-commands"].firstMatch)
        activate(app.buttons["workspace-action-Medium Tiles"])
        expectation(for: NSPredicate { _, _ in abs(zoom.frame.width - 54) <= 1 }, evaluatedWith: zoom)
        waitForExpectations(timeout: 10)
        XCTAssertEqual(zoom.frame.height, 54, accuracy: 1)
        activate(app.buttons["zen-button"])
        XCTAssertTrue(app.staticTexts["document-title"].waitForNonExistence(timeout: 10))
        XCTAssertTrue(zoom.exists)
        XCTAssertEqual(zoom.frame.width, 54, accuracy: 1)
        activate(app.buttons["zen-button"])
        XCTAssertTrue(app.staticTexts["document-title"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
