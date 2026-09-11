import XCTest

extension XCTestCase {
    @MainActor func checkIndependentEditorWindows(in app: XCUIApplication) {
        // Install ordinary toolbar commands so the test exercises editor
        // effects without navigating the macOS system menu bar.
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"customize","action":{"type":"insert_tools","panel":"commands","before":null}},{"type":"customize","action":{"type":"picker_select","control":{"kind":"command","command":"new_window"},"selected":true}},{"type":"customize","action":{"type":"picker_select","control":{"kind":"command","command":"close_document"},"selected":true}},{"type":"customize","action":{"type":"confirm_tools"}}]"#
        app.launch()
        let scenes = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "editor-scene-"))
        XCTAssertTrue(scenes.firstMatch.waitForExistence(timeout: 20))
        let firstID = scenes.firstMatch.identifier
        let first = app.descendants(matching: .any)[firstID].firstMatch
        func activate(_ button: XCUIElement) {
            XCTAssertTrue(button.waitForExistence(timeout: 10))
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: button)
            waitForExpectations(timeout: 20)
            #if os(macOS)
            button.click()
            #else
            button.tap()
            #endif
        }
        func command(_ scene: XCUIElement, _ label: String) -> XCUIElement {
            scene.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", label)).firstMatch
        }
        func rows(_ scene: XCUIElement) -> XCUIElementQuery {
            #if os(macOS)
            return scene.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            #else
            return scene.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            #endif
        }
        func expectRows(_ scene: XCUIElement, _ count: Int) {
            expectation(for: NSPredicate { _, _ in rows(scene).count == count }, evaluatedWith: scene)
            waitForExpectations(timeout: 20)
        }
        expectRows(first, 2)
        activate(first.buttons["layer-New layer"])
        activate(first.buttons["layer-New layer"])
        expectRows(first, 4)
        activate(command(first, "New Window"))
        let second = scenes.matching(NSPredicate(format: "identifier != %@", firstID)).firstMatch
        XCTAssertTrue(second.waitForExistence(timeout: 30), "New Window must create another editor scene")
        XCTAssertNotEqual(second.identifier, firstID)
        expectRows(second, 2)
        activate(second.buttons["layer-New layer"])
        expectRows(second, 3)
        activate(command(second, "Undo"))
        expectRows(second, 2)
        #if os(macOS)
        expectRows(first, 4)
        #endif
        let opened = XCTAttachment(screenshot: app.screenshot())
        opened.name = "independent-editor-windows"; opened.lifetime = .keepAlways; add(opened)
        activate(command(second, "Close"))
        XCTAssertTrue(second.waitForNonExistence(timeout: 20), "Close must destroy only the second editor scene")
        // Closing the foreground iPad scene can leave the surviving scene in
        // the app switcher. Bring the application forward before its next edit.
        app.activate()
        XCTAssertTrue(first.waitForExistence(timeout: 20))
        expectRows(first, 4)
        activate(command(first, "Undo"))
        expectRows(first, 3)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        // Leave this disposable test document clean before app termination.
        activate(command(first, "Undo"))
        expectRows(first, 2)
        app.terminate()
    }
}
