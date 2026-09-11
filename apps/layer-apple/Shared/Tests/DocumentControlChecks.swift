import XCTest

extension XCTestCase {
    @MainActor func checkNewDrawingAndExportCancellation(in app: XCUIApplication) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 15))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        func replace(_ field: XCUIElement, _ text: String) {
            activate(field)
            #if os(macOS)
            field.typeKey("a", modifierFlags: .command)
            field.typeText(text)
            #else
            let count = (field.value as? String)?.count ?? 0
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: count) + text)
            #endif
        }
        let width = app.textFields["new-document-width"]
        let height = app.textFields["new-document-height"]
        let create = app.buttons["new-document-create"]
        replace(width, "0")
        XCTAssertFalse(create.isEnabled, "Invalid dimensions must not allocate a canvas")
        replace(width, "63"); replace(height, "47")
        XCTAssertEqual(width.value as? String, "63")
        XCTAssertEqual(height.value as? String, "47")
        activate(create)
        #if os(iOS)
        // The floating number pad consumes the outside tap. If the form remains
        // after dismissal settles, activate Create with the pad out of the way.
        if !create.waitForNonExistence(timeout: 2) { activate(create) }
        #endif
        let title = app.staticTexts["document-title"]
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Untitled · 63 × 47", "Untitled · 63 × 47"), evaluatedWith: title)
        waitForExpectations(timeout: 30)
        #if os(macOS)
        // Exercise the editor action through its shortcut, never system-menu coordinates.
        app.typeKey("e", modifierFlags: [.command, .shift])
        #else
        activate(app.descendants(matching: .any)["menu-File"].firstMatch)
        activate(app.buttons["command-export_document"])
        #endif
        let cancel = app.buttons["Cancel"].firstMatch
        #if os(macOS)
        XCTAssertTrue(cancel.waitForExistence(timeout: 15))
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        #else
        activate(cancel)
        #endif
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: cancel)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(title.label == "Untitled · 63 × 47" || title.value as? String == "Untitled · 63 × 47",
            "Export cancellation must retain the drawing")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #if os(iOS)
        activate(app.descendants(matching: .any)["menu-File"].firstMatch)
        activate(app.buttons["command-close_document"])
        expectation(for: NSPredicate { _, _ in
            [.runningBackground, .runningBackgroundSuspended, .notRunning].contains(app.state)
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        #endif
    }
}
