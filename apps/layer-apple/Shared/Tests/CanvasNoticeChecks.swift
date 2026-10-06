import XCTest

extension XCTestCase {
    @MainActor func checkCanvasNotice(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let notice = app.descendants(matching: .any)["canvas-notice"].firstMatch
        let text = app.staticTexts["canvas-notice-text"], action = app.buttons["canvas-notice-action-use_reference"]
        let viewport = workspaceViewport(in: app)
        func tapCanvas() { viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.45)).clickOrTap() }
        workspaceActivate(app.buttons["layer-New layer"])
        editorTool("Auto select", in: app)
        let reference = app.buttons["tool-action-selection_reference"]
        revealEditorControl(reference, in: app.scrollViews.containing(.button, identifier: reference.identifier).firstMatch)
        workspaceActivate(reference)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: reference)
        waitForExpectations(timeout: 5)

        tapCanvas()
        XCTAssertTrue(text.waitForExistence(timeout: 10), "A Wand without a marked reference explains itself")
        let shown = (text.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? text.label
        XCTAssertEqual(shown, "This tool samples reference layers, and none is marked")
        XCTAssertEqual(action.label, "Use Current ink as Reference", "The notice offers the layer below")
        attachEditor(in: app, name: "canvas-notice")
        workspaceActivate(action)
        XCTAssertTrue(notice.waitForNonExistence(timeout: 5), "Accepting the notice hides it")
        tapCanvas()
        XCTAssertFalse(text.waitForExistence(timeout: 2), "The Wand samples the new reference without a notice")

        editorHistory("Undo", in: app); editorHistory("Undo", in: app)
        tapCanvas()
        XCTAssertTrue(text.waitForExistence(timeout: 10), "Undoing the selection and the reference restores the notice")
        XCTAssertTrue(notice.waitForNonExistence(timeout: 8), "An unanswered notice declines itself")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
