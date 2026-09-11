import XCTest

extension XCTestCase {
    @MainActor func checkShortcutConflictAndEditorEffect(in app: XCUIApplication) {
        func activate(_ item: XCUIElement) {
            XCTAssertTrue(item.waitForExistence(timeout: 15))
            #if os(macOS)
            item.click()
            #else
            item.tap()
            #endif
        }
        let search = app.textFields["shortcut-search"]
        activate(search); search.typeText("Zen")
        activate(app.buttons["shortcut-command.ZenMode"])
        activate(app.buttons["shortcut-add"])
        XCTAssertTrue(app.staticTexts["shortcut-captured"].waitForExistence(timeout: 10))
        // Capture must intercept an existing accelerator before it invokes Undo.
        app.typeKey("z", modifierFlags: .command)
        let confirm = app.buttons["shortcut-confirm"]
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: confirm)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(confirm.label == "Replace" || confirm.value as? String == "Replace")
        activate(confirm)
        XCTAssertTrue(confirm.waitForNonExistence(timeout: 10))
        activate(app.buttons["shortcut-editor-done"])
        activate(app.buttons["settings-done"])
        let title = app.staticTexts["document-title"]
        XCTAssertTrue(title.waitForExistence(timeout: 10))
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(title.waitForNonExistence(timeout: 10), "The new shortcut must execute Zen after the editor closes")
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(title.waitForExistence(timeout: 10))
    }
}
