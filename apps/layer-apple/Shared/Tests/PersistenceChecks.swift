import XCTest

extension XCTestCase {
    /// Each application keeps its files in a new folder inside the app's
    /// temporary folder, never in the installation's storage.
    @MainActor func editorTestApplication() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["CAPY_STORAGE_DIR"] = "capy-test-" + UUID().uuidString
        // Clean up the app even when an assertion interrupts the workflow.
        // Each test owns this application; unrelated app instances stay open.
        addTeardownBlock {
            await MainActor.run {
                if app.state != .notRunning { app.terminate() }
            }
        }
        return app
    }

    @MainActor func editorCaptureApplication() -> XCUIApplication {
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_CAPTURE_PROBE"] = "1"
        return app
    }

    @MainActor func checkSettingsAndWorkspaceRestart(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"},{"type":"customize","action":{"type":"set_panel_visible","panel":"color","visible":true}}]"#
        for launch in 0..<2 {
            if launch == 1 { app.launchEnvironment.removeValue(forKey: "CAPY_INITIAL_ACTIONS") }
            app.launch()
            XCTAssertTrue(app.descendants(matching: .any)["color-wheel"].firstMatch.waitForExistence(timeout: 15),
                "The restored workspace must retain the Color panel")
            let status = app.staticTexts["persistence-status"]
            expectation(for: NSPredicate(format: "label == %@ AND value == %@", "Saved", "dark"), evaluatedWith: status)
            waitForExpectations(timeout: 15)
            XCTAssertFalse(app.alerts.firstMatch.exists, "Valid saved state must restore without errors")
            app.terminate()
        }
    }
}
