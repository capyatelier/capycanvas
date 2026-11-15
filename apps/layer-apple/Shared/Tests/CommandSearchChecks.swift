import XCTest

extension XCTestCase {
    @MainActor func checkCommandSearch(in app: XCUIApplication, fromMenu: Bool = false) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let field = app.textFields["command-search"], detail = app.staticTexts["command-detail"]
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        func selected(_ label: String) {
            let tool = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-toolbar-", label)).firstMatch
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: tool)
            waitForExpectations(timeout: 5)
        }
        func key(_ key: XCUIKeyboardKey) { app.typeKey(key.rawValue, modifierFlags: []) }
        func open(_ letter: String = "k", shift: Bool = false) {
            if fromMenu { editorMenu(in: app, menu: "Edit", id: "search_commands", label: "Search Commands…") }
            else { app.typeKey(letter, modifierFlags: shift ? [.command, .shift] : .command) }
            focused()
        }
        func focused() {
            XCTAssertTrue(field.waitForExistence(timeout: 5), "The command bar must open")
            expectation(for: NSPredicate(format: "hasKeyboardFocus == true"), evaluatedWith: field)
            waitForExpectations(timeout: 5)
        }
        func closed() { XCTAssertTrue(field.waitForNonExistence(timeout: 5), "The command bar must close") }
        func detailText() -> String {
            #if os(macOS)
            detail.value as? String ?? ""
            #else
            detail.label
            #endif
        }
        func replace(_ text: String) {
            field.typeKey("a", modifierFlags: .command); field.typeText(text)
        }

        for _ in 0..<3 {
            editorTool("Pen", in: app); selected("Pen")
            open(); runCommandSearch("eraser", field: field, in: app); closed(); selected("Eraser")
        }
        app.typeKey("h", modifierFlags: []); selected("Hand")

        open(); field.typeText("undo")
        let first = app.buttons["command-result-0"]
        expectation(for: NSPredicate(format: "label BEGINSWITH %@", "Undo"), evaluatedWith: first)
        waitForExpectations(timeout: 5)
        XCTAssertEqual(first.value as? String, "Unavailable", "A new drawing has nothing to undo")
        XCTAssertTrue(detail.waitForExistence(timeout: 5))
        XCTAssertFalse(detailText().isEmpty, "The selected unavailable command explains why")
        attachEditor(in: app, name: "command-search-light")

        replace("select")
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: first)
        waitForExpectations(timeout: 5)
        key(.downArrow)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: app.buttons["command-result-1"])
        waitForExpectations(timeout: 5)
        key(.upArrow)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: first)
        waitForExpectations(timeout: 5)

        editorToolAfterSearch("Pen", field: field, in: app)
        open(); runCommandSearch("brush size", field: field, in: app)
        expectation(for: NSPredicate(format: "label BEGINSWITH %@", "Brush size"), evaluatedWith: field)
        waitForExpectations(timeout: 5)
        let range = detailText()
        XCTAssertTrue(range.contains("Current"), "The value step describes the setting: \(range)")
        replace("bad input\n")
        expectation(for: NSPredicate { _, _ in detailText() != range }, evaluatedWith: detail)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(field.exists, "Invalid parameter text keeps the value step open")
        #if os(macOS)
        key(.escape)
        expectation(for: NSPredicate(format: "value == %@", "brush size"), evaluatedWith: field)
        waitForExpectations(timeout: 5)
        field.typeText("\n")
        expectation(for: NSPredicate(format: "label BEGINSWITH %@", "Brush size"), evaluatedWith: field)
        waitForExpectations(timeout: 5)
        #else
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 5), "Keyboard Search hides the keyboard")
        workspaceActivate(field)
        #endif
        replace("24\n"); closed()
        let size = app.buttons["number-value-tool-size"]
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", "24"), evaluatedWith: size)
        waitForExpectations(timeout: 5)

        app.typeKey("e", modifierFlags: []); selected("Eraser")
        editorTool("Pen", in: app); selected("Pen")
        open(); field.typeText("hand")
        workspaceActivate(app.buttons["command-result-0"]); closed(); selected("Hand")

        editorTool("Pen", in: app); selected("Pen")
        open()
        let card = app.descendants(matching: .any)["command-bar"].firstMatch.frame, area = canvas.frame
        let outside = CGPoint(x: (card.minX - 24 - area.minX) / area.width, y: (card.midY - area.minY) / area.height)
        dismissCommandSearch(field, in: app)
        let before = settledEditorPixels(in: app, at: outside, size: 16)
        open()
        #if os(macOS)
        canvas.coordinate(withNormalizedOffset: CGVector(dx: outside.x, dy: outside.y)).click()
        #else
        canvas.coordinate(withNormalizedOffset: CGVector(dx: outside.x, dy: outside.y)).tap()
        #endif
        closed()
        XCTAssertEqual(editorPixels(in: app, at: outside, size: 16), before, "Dismissal must not paint")

        editorMenu(in: app, menu: "Edit", id: "search_commands", label: "Search Commands…")
        focused()
        dismissCommandSearch(field, in: app)
        app.typeKey("e", modifierFlags: []); selected("Eraser")

        open("p", shift: true); field.typeText("select")
        workspaceActivate(app.buttons["command-search-close"]); closed()
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor private func dismissCommandSearch(_ field: XCUIElement, in app: XCUIApplication) {
        #if os(macOS)
        let empty = { (field.value as? String).map { $0.isEmpty || $0 == field.placeholderValue } ?? true }
        if !empty() {
            app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
            expectation(for: NSPredicate { _, _ in empty() }, evaluatedWith: field)
            waitForExpectations(timeout: 5)
        }
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        #else
        workspaceActivate(app.buttons["command-search-close"])
        #endif
        XCTAssertTrue(field.waitForNonExistence(timeout: 5), "The command bar must close")
    }
    @MainActor func runCommandSearch(_ query: String, field: XCUIElement, in app: XCUIApplication) {
        #if os(macOS)
        field.typeText(query + "\n")
        #else
        field.typeText(query); workspaceActivate(app.buttons["command-result-0"])
        #endif
    }
    @MainActor private func editorToolAfterSearch(_ label: String, field: XCUIElement, in app: XCUIApplication) {
        if field.exists { dismissCommandSearch(field, in: app) }
        editorTool(label, in: app)
    }
}
