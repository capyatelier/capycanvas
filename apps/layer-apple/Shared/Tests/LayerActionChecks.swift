import XCTest

extension XCTestCase {
    @MainActor func checkLayerControls(in container: XCUIElement) {
        #if os(macOS)
        let candidates = container.groups
        #else
        let candidates = container.otherElements
        #endif
        let rows = candidates.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let initialCount = rows.count
        XCTAssertGreaterThanOrEqual(initialCount, 2)
        let original = rows.element(boundBy: 0)
        let originalID = original.identifier
        activate(container.buttons["layer-New layer"])
        expectation(for: NSPredicate { _, _ in rows.count == initialCount + 1 }, evaluatedWith: container)
        waitForExpectations(timeout: 5)
        let current = rows.element(boundBy: 0)
        let currentID = current.identifier
        XCTAssertNotEqual(currentID, originalID)
        let originalRow = candidates[originalID].firstMatch
        let checked = originalRow.buttons.matching(NSPredicate(format: "label == %@", "Select layer without changing drawing target")).firstMatch
        let currentRow = candidates[currentID].firstMatch
        let content = currentRow.buttons["Edit layer content"]
        XCTAssertTrue(content.isSelected, "New layer must become the drawing target")
        activate(checked)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: checked)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(content.isSelected, "Checking another row must preserve the drawing target")
        activate(container.buttons["layer-Add layer mask"])
        let mask = currentRow.buttons["Edit layer mask"]
        XCTAssertTrue(mask.waitForExistence(timeout: 5))
        activate(mask)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: mask)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(content.isSelected)
        activate(content)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: content)
        waitForExpectations(timeout: 5)
        #if os(macOS)
        mask.rightClick()
        #else
        mask.press(forDuration: 0.6)
        #endif
        let deleteMask = container.buttons["Delete mask"]
        XCTAssertTrue(deleteMask.waitForExistence(timeout: 5), "The mask context gesture must open its mask menu")
        activate(deleteMask)
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: mask)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(container.staticTexts["Canvas error"].exists)
    }
    @MainActor private func activate(_ element: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(element.exists, "Missing layer control: \(element)", file: file, line: line)
        #if os(macOS)
        element.click()
        #else
        element.tap()
        #endif
    }
}
