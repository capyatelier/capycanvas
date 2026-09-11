import XCTest

extension XCTestCase {
    @MainActor func checkSystemStatusSetting(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"preferences","action":{"type":"edit","id":"show_clock","value":1}}]"#
        app.launch()
        let clock = app.staticTexts["system-clock"]
        let title = app.staticTexts["document-title"]
        XCTAssertTrue(clock.waitForExistence(timeout: 20))
        XCTAssertTrue(title.waitForExistence(timeout: 10))
        XCTAssertLessThanOrEqual(title.frame.maxX + 5, clock.frame.minX)
        XCTAssertLessThanOrEqual(abs(clock.frame.midY - title.frame.midY), 1)
        #if os(macOS)
        let screenshot = app.windows.firstMatch.screenshot()
        #else
        let screenshot = XCUIScreen.main.screenshot()
        #endif
        let capture = XCTAttachment(screenshot: screenshot)
        capture.name = "header-system-status"; capture.lifetime = .keepAlways; add(capture)

        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 10))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        // Verify the editor's Zen and preference effects without OS menu tests.
        activate(app.buttons["zen-button"])
        XCTAssertTrue(clock.waitForNonExistence(timeout: 10))
        activate(app.buttons["zen-button"])
        XCTAssertTrue(clock.waitForExistence(timeout: 10))
        for choice in ["Never", "Always"] {
            activate(app.buttons["Settings"].firstMatch)
            activate(app.descendants(matching: .any)["preference-show_clock"].firstMatch)
            #if os(macOS)
            activate(app.menuItems[choice].firstMatch)
            #else
            activate(app.buttons[choice].firstMatch)
            #endif
            activate(app.buttons["settings-done"])
            if choice == "Never" { XCTAssertTrue(clock.waitForNonExistence(timeout: 10)) }
            else { XCTAssertTrue(clock.waitForExistence(timeout: 10)) }
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
