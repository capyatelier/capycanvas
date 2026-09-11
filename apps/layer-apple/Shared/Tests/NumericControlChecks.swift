import XCTest

extension XCTestCase {
    @MainActor func checkNumericToolControls(in app: XCUIApplication) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 5))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        let value = app.buttons["number-value-tool-size"]
        let entry = app.textFields["number-entry-tool-size"]
        activate(value)
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeText("20 + 22\n")
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", "42"), evaluatedWith: value)
        waitForExpectations(timeout: 5)
        let sizePanel = app.buttons["number-value-Brush size"]
        XCTAssertTrue((sizePanel.value as? String)?.hasPrefix("42") == true,
            "Tool Settings and Brush size must reflect the same accepted Rust edit")
        activate(app.buttons["number-increase-tool-size"])
        expectation(for: NSPredicate { _, _ in (value.value as? String)?.hasPrefix("42") == false }, evaluatedWith: value)
        waitForExpectations(timeout: 5)
        let accepted = value.value as? String
        activate(value)
        entry.typeText("1 / 0\n")
        let error = app.staticTexts["number-error-tool-size"]
        XCTAssertTrue(error.waitForExistence(timeout: 5))
        XCTAssertEqual(sizePanel.value as? String, accepted, "An invalid expression must preserve brush size")
        #if os(macOS)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: error)
        waitForExpectations(timeout: 5)
        XCTAssertEqual(value.value as? String, accepted, "Escape must discard the draft")
        #else
        // Simulator XCTest Escape reached neither UIKit key commands, presses
        // nor text insertion in the traced run. Keep physical Escape acceptance
        // open; verify actual keyboard correction through the focused field.
        let count = (entry.value as? String)?.count ?? 0
        entry.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: count) + "20 + 22\n")
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", "42"), evaluatedWith: value)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(error.exists, "A corrected expression must clear local feedback")
        #endif
        activate(app.buttons["brush-7"])
        XCTAssertTrue(app.buttons["brush-7"].isSelected, "The Marker brush must become selected")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
