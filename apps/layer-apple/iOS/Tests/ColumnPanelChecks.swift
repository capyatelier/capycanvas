import XCTest

final class ColumnPanelChecks: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }

    @MainActor func testAttachedColumnResizingAndHistory() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        let actions: [[String: Any]] = [
            ["type": "set_theme", "theme": "light"],
            ["type": "customize", "action": ["type": "insert_tools", "panel": "toolbar", "before": 1]],
            ["type": "customize", "action": ["type": "picker_select", "control": ["kind": "command", "command": "undo_workspace"], "selected": true]],
            ["type": "customize", "action": ["type": "picker_select", "control": ["kind": "command", "command": "redo_workspace"], "selected": true]],
            ["type": "customize", "action": ["type": "confirm_tools"]],
            ["type": "move_panel", "panel": "sizes", "target": ["kind": "tab", "group": 6], "viewport": [1376, 1032]],
            ["type": "customize", "action": ["type": "set_column_collapsed", "group": 6, "collapsed": true]],
            ["type": "customize", "action": ["type": "set_column_mode", "column": 4, "mode": "group_panel"]],
            ["type": "customize", "action": ["type": "toggle_column_drawer", "group": 6, "panel": "brushes"]],
        ]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let drawer = app.descendants(matching: .any)["column-drawer-4"].firstMatch
        let width = app.descendants(matching: .any)["column-panel-resize-4-width"].firstMatch
        let split = app.descendants(matching: .any)["column-panel-resize-4-brushes"].firstMatch
        XCTAssertTrue(width.waitForExistence(timeout: 25)); XCTAssertTrue(split.exists)
        func command(_ label: String) {
            let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-toolbar-", label)).firstMatch
            XCTAssertTrue(button.isEnabled); button.tap()
        }
        func reopen() {
            if !drawer.exists { app.buttons["column-icon-brushes"].tap() }
            XCTAssertTrue(width.waitForExistence(timeout: 5)); XCTAssertTrue(split.exists)
        }
        func wait(_ description: String, _ ready: @escaping () -> Bool) {
            let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in ready() }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [expectation], timeout: 5), .completed, description)
        }
        let initialWidth = drawer.frame.width
        let start = width.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.2))
        start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 44, dy: 0)))
        wait("Immediate touch grip changes width") { drawer.frame.width > initialWidth + 25 }
        let changedWidth = drawer.frame.width
        command("Undo Layout Change"); reopen()
        wait("Undo restores width") { abs(drawer.frame.width - initialWidth) < 1 }
        command("Redo Layout Change"); reopen()
        wait("Redo restores width") { abs(drawer.frame.width - changedWidth) < 1 }
        let initialY = split.frame.midY
        let splitStart = split.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        splitStart.press(forDuration: 0.05, thenDragTo: splitStart.withOffset(CGVector(dx: 0, dy: 44)))
        wait("Immediate touch grip changes the panel split") { split.frame.midY > initialY + 25 }
        let changedY = split.frame.midY
        command("Undo Layout Change"); reopen()
        wait("Undo restores panel split") { abs(split.frame.midY - initialY) < 1 }
        command("Redo Layout Change"); reopen()
        wait("Redo restores panel split") { abs(split.frame.midY - changedY) < 1 }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
