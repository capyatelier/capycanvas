import XCTest

final class EditorMenuChecks: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }
    @MainActor func testMainMenuThemesAndAction() {
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            let app = editorTestApplication()
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = "[{\"type\":\"set_theme\",\"theme\":\"\(theme)\"}]"
            app.launch()
            let newLayer = app.buttons["layer-New layer"]
            XCTAssertTrue(newLayer.waitForExistence(timeout: 20)); newLayer.tap()
            let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
            waitForExpectations(timeout: 5)
            app.buttons["menu-Edit"].tap()
            let menu = app.descendants(matching: .any)["application-menu-content"].firstMatch
            XCTAssertTrue(menu.waitForExistence(timeout: 5), "Main menu button state: \(String(describing: app.buttons["menu-Edit"].value))")
            XCTAssertLessThan(menu.frame.width, 380)
            let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            capture.name = "flat-main-menu-" + theme; capture.lifetime = .keepAlways; add(capture)
            app.buttons["command-undo"].tap()
            XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
            expectation(for: NSPredicate { _, _ in rows.count == 2 }, evaluatedWith: app)
            waitForExpectations(timeout: 5)
            app.buttons["layer-blend"].tap()
            let normal = app.buttons["layer-blend-option-0"]
            XCTAssertTrue(normal.waitForExistence(timeout: 5)); normal.tap()
            XCTAssertTrue(normal.waitForNonExistence(timeout: 5))
            app.terminate()
        }
    }
    @MainActor func testSubmenusAndShortcuts() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launch()
        let edit = app.buttons["menu-Edit"]
        XCTAssertTrue(edit.waitForExistence(timeout: 20))
        // The padded label is part of the button's target, including its edges.
        edit.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.1)).tap()
        let menu = app.descendants(matching: .any)["application-menu-content"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["command-undo"].isEnabled)
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.7)).tap()
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5))

        app.buttons["menu-Filter"].tap()
        let blur = app.buttons["menu-action-Blur"]
        XCTAssertTrue(blur.waitForExistence(timeout: 5)); blur.tap()
        let back = app.buttons["editor-menu-back"]
        XCTAssertTrue(back.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["menu-action-Gaussian Blur"].exists)
        back.tap()
        XCTAssertTrue(blur.waitForExistence(timeout: 5))
        app.typeKey(XCUIKeyboardKey.rightArrow, modifierFlags: [])
        XCTAssertTrue(back.waitForExistence(timeout: 5), "Right enters the focused submenu")
        app.typeKey(XCUIKeyboardKey.leftArrow, modifierFlags: [])
        XCTAssertTrue(back.waitForNonExistence(timeout: 5))
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.7)).tap()
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5))

        app.buttons["layer-New layer"].tap()
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        edit.tap()
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5), "The menu shortcut executes Undo and dismisses")
        expectation(for: NSPredicate { _, _ in rows.count == 2 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        app.typeKey("z", modifierFlags: [.command, .shift])
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
    }
    @MainActor func testCompactMenuShortcutAcrossPages() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"customize","action":{"type":"header","action":{"type":"remove","id":2}}},{"type":"customize","action":{"type":"header","action":{"type":"add","zone":"left","before":null,"item":{"kind":"menu"}}}},{"type":"invoke","command":"add_layer"}]"#
        app.launch()
        let trigger = app.buttons["application-menus"]
        XCTAssertTrue(trigger.waitForExistence(timeout: 20))
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        trigger.tap()
        let menu = app.descendants(matching: .any)["application-menu-content"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        let file = app.buttons["menu-action-File"]
        XCTAssertTrue(file.waitForExistence(timeout: 5)); file.tap()
        XCTAssertTrue(app.buttons["editor-menu-back"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["command-undo"].exists)
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5), "Undo must execute from the other submenu and dismiss")
        expectation(for: NSPredicate { _, _ in rows.count == 2 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        trigger.tap()
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["command-redo"].exists)
        app.typeKey("z", modifierFlags: [.command, .shift])
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5), "Redo must execute before its submenu is opened")
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
    @MainActor func testWorkspaceMenuDragBothDirections() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"workspace_manager","command":{"type":"manage"}}]"#
        app.launch()
        let painter = app.buttons["workspace-select-builtin:workspace:painter"]
        let illustrator = app.buttons["workspace-select-builtin:workspace:illustrator"]
        let photographer = app.buttons["workspace-select-builtin:workspace:photographer"]
        XCTAssertTrue(photographer.waitForExistence(timeout: 30))
        XCTAssertTrue(illustrator.isSelected)
        photographer.press(forDuration: 0.8)
        let menu = app.descendants(matching: .any)["workspace-row-menu"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 5), "Stationary release retains the same vertical menu helper")
        app.buttons["menu-action-Move Up"].tap()
        expectation(for: NSPredicate { _, _ in photographer.frame.minY < illustrator.frame.minY }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        photographer.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.8,
            thenDragTo: painter.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1)), withVelocity: .slow, thenHoldForDuration: 0.1)
        expectation(for: NSPredicate { _, _ in photographer.frame.minY < painter.frame.minY }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(menu.exists)
        photographer.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.8,
            thenDragTo: illustrator.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.9)), withVelocity: .slow, thenHoldForDuration: 0.1)
        expectation(for: NSPredicate { _, _ in photographer.frame.minY > illustrator.frame.minY }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(illustrator.isSelected, "Menu actions and held dragging must not select another workspace")
    }
}
