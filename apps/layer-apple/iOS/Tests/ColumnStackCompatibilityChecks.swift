import XCTest

// The replacement column presentation is currently GTK-first. Apple keeps the
// portable preference while presenting ordinary drawers.
final class ColumnStackCompatibilityChecks: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }

    @MainActor func testColumnPreferenceUsesOrdinaryDrawerUntilPorted() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        let actions: [[String: Any]] = [
            ["type": "customize", "action": ["type": "set_column_collapsed", "group": 6, "collapsed": true]],
            ["type": "customize", "action": ["type": "set_column_drawers", "column": 4, "drawers": false]],
            ["type": "customize", "action": ["type": "toggle_column_drawer", "group": 6, "panel": "brushes"]],
        ]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let drawer = app.descendants(matching: .any)["column-drawer-4"].firstMatch
        XCTAssertTrue(drawer.waitForExistence(timeout: 25))
        XCTAssertTrue(app.buttons["drawer-tab-brushes"].exists)
        XCTAssertFalse(app.buttons["expand-column-4"].exists)
        app.buttons["column-icon-brushes"].tap()
        let closed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: drawer)
        XCTAssertEqual(XCTWaiter.wait(for: [closed], timeout: 5), .completed)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
