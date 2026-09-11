import XCTest

extension XCTestCase {
    @MainActor func checkArtworkRecoveryAfterRestart(in app: XCUIApplication) {
        app.launchEnvironment.removeValue(forKey: "CAPY_DISABLE_PERSISTENCE")
        app.launchEnvironment["CAPY_PERSISTENCE_NAMESPACE"] = UUID().uuidString
        app.launchEnvironment["CAPY_PERSISTENCE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"invoke","command":"add_layer"},{"type":"set_layer_opacity","opacity":0.42}]"#
        app.launch()
        let ready = app.staticTexts["recovery-status"]
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Recovery ready", "Recovery ready"), evaluatedWith: ready)
        waitForExpectations(timeout: 30)
        app.terminate()
        app.launchEnvironment.removeValue(forKey: "CAPY_INITIAL_ACTIONS")
        app.launch()
        let open = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "open-recovery-")).firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 20), "An unclosed drawing must be offered after process restart")
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: open)
        waitForExpectations(timeout: 30)
        let picker = XCTAttachment(screenshot: app.screenshot())
        picker.name = "recovered-drawings-picker"; picker.lifetime = .keepAlways; add(picker)
        #if os(macOS)
        open.click()
        let rows = app.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        #else
        open.tap()
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        #endif
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        XCTAssertTrue(open.waitForNonExistence(timeout: 5))
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Recovery ready", "Recovery ready"), evaluatedWith: ready)
        waitForExpectations(timeout: 30)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        let restored = XCTAttachment(screenshot: app.screenshot())
        restored.name = "recovered-drawing-editor"; restored.lifetime = .keepAlways; add(restored)
        app.terminate()
    }
}
