import XCTest

extension EditorLaunchTests {
    @MainActor func testWorkspaceHeldTiles() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"customize","action":{"type":"insert_tools","panel":"toolbar","before":1}},{"type":"customize","action":{"type":"picker_select","control":{"kind":"command","command":"undo_workspace"},"selected":true}},{"type":"customize","action":{"type":"picker_select","control":{"kind":"command","command":"redo_workspace"},"selected":true}},{"type":"customize","action":{"type":"confirm_tools"}}]"#
        app.launch()
        func tile(_ panel: String, _ name: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-\(panel)-", name)).firstMatch
        }
        let eraser = tile("toolbar", "Eraser"), brush = tile("toolbar", "Brush")
        let undo = tile("toolbar", "Undo Layout Change"), redo = tile("toolbar", "Redo Layout Change")
        XCTAssertTrue(eraser.waitForExistence(timeout: 30)); XCTAssertTrue(brush.exists)
        let initial = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        initial.name = "held-tiles-initial-window"; initial.lifetime = .keepAlways; add(initial)
        XCTAssertTrue(app.windows.firstMatch.frame.contains(undo.frame), "The fixture's layout Undo must fit inside the actual window")
        eraser.tap(); XCTAssertTrue(eraser.isSelected)
        brush.tap(); XCTAssertTrue(brush.isSelected)
        let original = eraser.frame, brushBounds = brush.frame
        let menu = app.descendants(matching: .any)["workspace-context-menu"].firstMatch
        func drag(_ source: XCUIElement, held: Bool, to target: XCUICoordinate) {
            source.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                .press(forDuration: held ? 0.8 : 0.05, thenDragTo: target)
        }
        drag(eraser, held: false, to: brush.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: 0.05)))
        XCTAssertEqual(eraser.frame.minX, original.minX, accuracy: 1)
        XCTAssertEqual(eraser.frame.minY, original.minY, accuracy: 1)
        XCTAssertTrue(brush.isSelected); XCTAssertFalse(menu.exists)
        eraser.press(forDuration: 0.8)
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertTrue(brush.isSelected, "A held release must not activate the tile")
        workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.95, dy: 0.95)).tap()
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        XCTAssertTrue(brush.isSelected)
        drag(eraser, held: true, to: brush.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: 0.05)))
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        let vertical = abs(original.minY - brushBounds.minY) > abs(original.minX - brushBounds.minX)
        expectation(for: NSPredicate { _, _ in
            vertical ? eraser.frame.minY < brush.frame.minY : eraser.frame.minX < brush.frame.minX
        }, evaluatedWith: eraser)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(brush.isSelected, "The reorder must preserve the selected tool")
        undo.tap()
        XCTAssertEqual(eraser.frame.minX, original.minX, accuracy: 1)
        XCTAssertEqual(eraser.frame.minY, original.minY, accuracy: 1)
        redo.tap()
        XCTAssertTrue(vertical ? eraser.frame.minY < brush.frame.minY : eraser.frame.minX < brush.frame.minX)
        undo.tap()
        let grip = app.descendants(matching: .any)["toolbar-options-toolbar"].firstMatch
        XCTAssertTrue(grip.exists)
        let gripBounds = grip.frame
        let windowBounds = app.windows.firstMatch.frame
        drag(grip, held: false, to: workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.5)))
        XCTAssertEqual(app.windows.firstMatch.frame, windowBounds,
            "Picking up an editor grip must not resize or move the iPad window")
        expectation(for: NSPredicate { _, _ in
            abs(grip.frame.midX - gripBounds.midX) + abs(grip.frame.midY - gripBounds.midY) > 100
        }, evaluatedWith: grip)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(menu.exists, "The immediate grip drag must not require a menu")
        undo.tap()
        XCTAssertEqual(grip.frame.minX, gripBounds.minX, accuracy: 1)
        XCTAssertEqual(grip.frame.minY, gripBounds.minY, accuracy: 1)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        capture.name = "native-held-tiles-and-immediate-grip"; capture.lifetime = .keepAlways; add(capture)
    }
}
