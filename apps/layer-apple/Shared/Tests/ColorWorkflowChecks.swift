import XCTest

extension XCTestCase {
    @MainActor func checkNativeSDRCreationAndPalettes(in app: XCUIApplication) {
        // Initial actions run only after native GPU startup completes.
        guard app.textFields["new-document-width"].waitForExistence(timeout: 60) else {
            XCTFail("The initial New Drawing form did not open after canvas startup")
            return
        }
        func replace(_ field: XCUIElement, _ value: String) {
            workspaceActivate(field)
            #if os(macOS)
            field.typeKey("a", modifierFlags: .command)
            #else
            field.tap(withNumberOfTaps: 3, numberOfTouches: 1)
            #endif
            field.typeText(value)
            XCTAssertEqual(field.value as? String, value)
        }
        func choose(_ id: String, _ label: String) {
            workspaceActivate(app.descendants(matching: .any).matching(identifier: id).firstMatch)
            #if os(macOS)
            workspaceActivate(app.menuItems[label].firstMatch)
            #else
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
        workspaceActivate(app.buttons["paint-palettes"].firstMatch)
        let name = app.textFields["color-library-name"]
        replace(name, "Studio colors")
        workspaceActivate(app.buttons["color-library-new"])
        replace(name, "Retained ink")
        workspaceActivate(app.buttons["color-library-store"])
        let swatch = app.buttons.matching(NSPredicate(format: "identifier MATCHES %@", "color-swatch-[0-9]+" )).firstMatch
        XCTAssertTrue(swatch.waitForExistence(timeout: 10))
        attachEditor(in: app, name: "sdr-palette-stored")
        workspaceActivate(swatch)
        XCTAssertTrue(name.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["paint-edit-color"].firstMatch)
        XCTAssertTrue(alpha.waitForExistence(timeout: 10))
        XCTAssertEqual(alpha.value as? String, "37", "Using a palette color must preserve alpha")
        workspaceActivate(app.buttons["Cancel"].firstMatch)
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
