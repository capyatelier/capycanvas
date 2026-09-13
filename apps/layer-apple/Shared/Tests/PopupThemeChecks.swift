import XCTest

extension XCTestCase {
    @MainActor func checkPopupThemeFollowsExplicitAndSystem() {
        #if os(iOS)
        XCUIDevice.shared.orientation = .landscapeLeft
        #endif
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":null}]"#
        app.launch()
        let status = app.staticTexts["persistence-status"]
        XCTAssertTrue(status.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value IN %@", ["light", "dark"]), evaluatedWith: status)
        waitForExpectations(timeout: 10)
        let system = status.value as! String
        let explicit = system == "dark" ? "light" : "dark"
        app.buttons["settings-button"].tap()
        let picker = app.descendants(matching: .any)["preference-theme"].firstMatch
        if !picker.waitForExistence(timeout: 3) { app.staticTexts["Appearance"].firstMatch.tap() }
        XCTAssertTrue(picker.waitForExistence(timeout: 5))
        picker.tap()
        #if os(macOS)
        app.menuItems[explicit.capitalized].tap()
        #else
        app.buttons[explicit.capitalized].tap()
        #endif
        expectation(for: NSPredicate(format: "value == %@", explicit), evaluatedWith: status)
        waitForExpectations(timeout: 10)
        popupThemeCapture(in: app, "settings-explicit-" + explicit)
        picker.tap()
        #if os(macOS)
        app.menuItems["System"].tap()
        #else
        app.buttons["System"].tap()
        #endif
        expectation(for: NSPredicate(format: "value == %@", system), evaluatedWith: status)
        waitForExpectations(timeout: 10)
        popupThemeCapture(in: app, "settings-system-" + system)
        app.buttons["settings-done"].tap()
        app.buttons["layer-Layer actions"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["layer-context-menu"].firstMatch.waitForExistence(timeout: 5))
        popupThemeCapture(in: app, "layer-system-" + system)
    }
    @MainActor private func popupThemeCapture(in app: XCUIApplication, _ name: String) {
        #if os(macOS)
        // XCUIApplication.screenshot() can include unrelated desktop windows.
        let capture = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        #else
        // Capture the full-screen fixture; app.screenshot() can crop landscape
        // content to a portrait rectangle on iPad.
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        #endif
        capture.name = name; capture.lifetime = .keepAlways; add(capture)
    }
}
