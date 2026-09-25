import XCTest

extension XCTestCase {
    @MainActor func editorDocumentTitle(in app: XCUIApplication) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "identifier == %@ OR (identifier BEGINSWITH %@ AND selected == YES)", "document-title", "drawing-tab-")).firstMatch
    }

    @MainActor func checkNativeDrawingTabs(in app: XCUIApplication) throws {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"customize","action":{"type":"insert_tools","panel":"commands","before":null}},{"type":"customize","action":{"type":"picker_select","control":{"kind":"command","command":"drawings"},"selected":true}},{"type":"customize","action":{"type":"confirm_tools"}}]"#
        app.launch(); capturePaintEditor(in: app)
        func command(_ label: String) {
            workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", label)).firstMatch)
        }
        command("New…")
        let create = app.buttons["new-document-create"]
        XCTAssertTrue(create.waitForExistence(timeout: 15))
        #if os(macOS)
        workspaceActivate(app.descendants(matching: .any)["new-document-depth"].firstMatch)
        workspaceActivate(app.menuItems["32-bit float HDR"].firstMatch)
        #else
        workspaceActivate(app.buttons["new-document-depth"].firstMatch)
        workspaceActivate(app.buttons["32-bit float HDR"].firstMatch)
        #endif
        workspaceActivate(create)
        XCTAssertTrue(create.waitForNonExistence(timeout: 60))
        func drawings() {
            command("Drawings…")
            XCTAssertTrue(app.buttons["Done"].firstMatch.waitForExistence(timeout: 15))
        }
        let stripFirst = app.buttons["drawing-tab-1"].firstMatch, stripSecond = app.buttons["drawing-tab-2"].firstMatch
        if stripFirst.waitForExistence(timeout: 5) && stripSecond.exists {
            func slide(_ source: XCUIElement, onto target: XCUIElement, dx: CGFloat) {
                let from = source.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                let to = target.coordinate(withNormalizedOffset: CGVector(dx: dx, dy: 0.5))
                #if os(macOS)
                from.click(forDuration: 0.05, thenDragTo: to)
                #else
                from.press(forDuration: 0.05, thenDragTo: to)
                #endif
            }
            slide(stripSecond, onto: stripFirst, dx: 0.1)
            expectation(for: NSPredicate { _, _ in stripSecond.frame.midX < stripFirst.frame.midX }, evaluatedWith: stripSecond)
            waitForExpectations(timeout: 15)
            attachEditor(in: app, name: "drawing-strip-reordered")
            slide(stripSecond, onto: stripFirst, dx: 0.9)
            expectation(for: NSPredicate { _, _ in stripFirst.frame.midX < stripSecond.frame.midX }, evaluatedWith: stripFirst)
            waitForExpectations(timeout: 15)
        }
        drawings()
        let selector = app.descendants(matching: .any)["drawing-selector"].firstMatch
        let first = selector.buttons["drawing-tab-1"].firstMatch
        let second = selector.buttons["drawing-tab-2"].firstMatch
        XCTAssertTrue(first.exists && second.exists)
        XCTAssertTrue(second.isSelected)
        attachEditor(in: app, name: "drawing-selector-two-documents")
        // Row body follows native mouse/touch arbitration; one committed move
        // has one independent order-history entry.
        let start = second.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let end = first.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        #else
        start.press(forDuration: 0.7, thenDragTo: end)
        #endif
        let undo = app.buttons["Undo Tab Order"]
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: undo); waitForExpectations(timeout: 15)
        XCTAssertLessThan(second.frame.midY, first.frame.midY)
        workspaceActivate(undo)
        expectation(for: NSPredicate { _, _ in first.frame.midY < second.frame.midY }, evaluatedWith: first); waitForExpectations(timeout: 15)
        workspaceActivate(app.buttons["Redo Tab Order"])
        expectation(for: NSPredicate { _, _ in second.frame.midY < first.frame.midY }, evaluatedWith: first); waitForExpectations(timeout: 15)
        workspaceActivate(first)
        XCTAssertTrue(app.buttons["Done"].firstMatch.waitForNonExistence(timeout: 30))
        drawings(); XCTAssertTrue(first.isSelected)
        workspaceActivate(selector.buttons["drawing-close-2"].firstMatch)
        // A newly created drawing is clean and closes without a discard prompt.
        XCTAssertTrue(app.buttons["Done"].firstMatch.waitForNonExistence(timeout: 30))
        drawings(); XCTAssertFalse(second.exists); XCTAssertTrue(first.isSelected)
        attachEditor(in: app, name: "drawing-selector-after-close")
        workspaceActivate(app.buttons["Done"].firstMatch)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
