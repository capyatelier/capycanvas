import XCTest

extension XCTestCase {
    @MainActor func checkWorkspaceLibraryHistory(in app: XCUIApplication) {
        app.launchEnvironment.removeValue(forKey: "CAPY_DISABLE_PERSISTENCE")
        app.launchEnvironment["CAPY_PERSISTENCE_NAMESPACE"] = UUID().uuidString
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"customize","action":{"type":"set_panel_visible","panel":"navigator","visible":false}},{"type":"workspace_manager","command":{"type":"layout_history"}}]"#
        app.launch()
        let manager = app.descendants(matching: .any)["workspace-library-manager"].firstMatch
        XCTAssertTrue(manager.waitForExistence(timeout: 30))
        let start = app.buttons["workspace-history-r0"]
        XCTAssertTrue(start.waitForExistence(timeout: 10))
        #if os(macOS)
        start.click()
        #else
        start.tap()
        #endif
        let restore = app.buttons["workspace-history-restore"]
        expectation(for: NSPredicate(format: "enabled == true"), evaluatedWith: restore)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(start.isSelected, "The previewed history version must expose its selection")
        attachEditor(in: app, name: "workspace-history-selected")
        #if os(macOS)
        restore.click()
        #else
        restore.tap()
        #endif
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: manager)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.buttons["panel-tab-navigator"].firstMatch.waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "workspace-history-restored")
    }
}
