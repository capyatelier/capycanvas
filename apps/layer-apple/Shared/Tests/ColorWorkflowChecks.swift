import XCTest

extension XCTestCase {
    @MainActor func checkNativeSDRCreationAndColorEditing(in app: XCUIApplication) {
        // Initial actions run only after native GPU startup completes.
        guard app.textFields["new-document-width"].waitForExistence(timeout: 60) else {
            XCTFail("The initial New Drawing form did not open after canvas startup")
            return
        }
        func replace(_ field: XCUIElement, _ value: String) {
            #if os(macOS)
            workspaceActivate(field)
            field.typeKey("a", modifierFlags: .command)
            field.typeText(value)
            #else
            // Keyboard avoidance can move the iPad sheet during the first tap.
            // Reacquire the field at its settled frame before sending keys.
            let focused = app.textFields.matching(identifier: field.identifier)
                .matching(NSPredicate(format: "hasKeyboardFocus == true")).firstMatch
            let scroll = app.scrollViews.containing(.textField, identifier: field.identifier).firstMatch
            for _ in 0..<2 {
                if scroll.exists { revealEditorControl(field, in: scroll) }
                workspaceActivate(field)
                if focused.waitForExistence(timeout: 1) { break }
            }
            XCTAssertTrue(focused.waitForExistence(timeout: 3))
            let current = field.value as? String ?? ""
            let count = current == field.placeholderValue ? 0 : current.count
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: count) + value)
            #endif
            XCTAssertEqual(field.value as? String, value)
        }
        func choose(_ id: String, _ label: String) {
            #if os(macOS)
            workspaceActivate(app.descendants(matching: .any).matching(identifier: id).firstMatch)
            workspaceActivate(app.menuItems[label].firstMatch)
            #else
            workspaceActivate(app.buttons[id].firstMatch)
            workspaceActivate(app.buttons[label].firstMatch)
            #endif
        }
        choose("new-document-preset", "Wide color")
        choose("new-document-depth", "16-bit SDR")
        choose("new-document-background", "Transparent")
        let width = app.textFields["new-document-width"]
        let create = app.buttons["new-document-create"]
        replace(width, "0"); XCTAssertFalse(create.isEnabled)
        replace(width, "79"); replace(app.textFields["new-document-height"], "53")
        replace(app.textFields["new-document-preset-name"], "Studio P3")
        #if os(macOS)
        workspaceActivate(app.checkBoxes["new-document-defaults"])
        #else
        workspaceActivate(app.switches["new-document-defaults"])
        #endif
        attachEditor(in: app, name: "sdr-new-options")
        workspaceActivate(create)
        #if os(iOS)
        if !create.waitForNonExistence(timeout: 2) { workspaceActivate(create) }
        #endif
        XCTAssertTrue(create.waitForNonExistence(timeout: 30))
        let title = app.staticTexts["document-title"]
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Untitled · 79 × 53", "Untitled · 79 × 53"), evaluatedWith: title)
        waitForExpectations(timeout: 30)
        workspaceActivate(app.buttons["paint-edit-color"].firstMatch)
        let alpha = app.textFields["color-input-3"]
        replace(alpha, "37")
        workspaceActivate(app.buttons["color-input-use"])
        XCTAssertTrue(alpha.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["paint-edit-color"].firstMatch)
        XCTAssertTrue(alpha.waitForExistence(timeout: 10))
        XCTAssertEqual(alpha.value as? String, "37")
        attachEditor(in: app, name: "sdr-tagged-paint")
        workspaceActivate(app.buttons["Cancel"].firstMatch)
        XCTAssertTrue(alpha.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.buttons["paint-palettes"].exists, "The picker uses Edit Color instead of a palette popup")
        #if os(macOS)
        app.typeKey("n", modifierFlags: .command)
        #else
        editorMenu(in: app, menu: "File", id: "new_document", label: "New drawing")
        #endif
        XCTAssertTrue(width.waitForExistence(timeout: 10))
        XCTAssertEqual(width.value as? String, "79")
        XCTAssertEqual(app.textFields["new-document-height"].value as? String, "53")
        choose("new-document-preset", "Studio P3")
        workspaceActivate(app.buttons["new-document-cancel"])
        XCTAssertTrue(create.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
