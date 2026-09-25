import XCTest

extension XCTestCase {
    @MainActor func checkTonalSelection(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let photo = app.buttons["workspace-switch-builtin:workspace:photographer"]
        workspaceActivate(photo)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: photo)
        waitForExpectations(timeout: 10)
        editorTool("Rectangle select", in: app)
        workspaceActivate(app.buttons["toolbar-choice-variant"])
        workspaceActivate(app.buttons["toolbar-choice-variant-7"])
        let more = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "toolbar-more-")).firstMatch
        let inline = app.descendants(matching: .any)["toolbar-segments-tonal-tones"].waitForExistence(timeout: 5)
        if !inline {
            workspaceActivate(more)
            XCTAssertTrue(app.descendants(matching: .any)["tool-drawer"].firstMatch.waitForExistence(timeout: 10))
            XCTAssertFalse(app.buttons["selection-menu-selection"].exists, "The compact tonal form has no Selection Actions")
        }
        let prefix = inline ? "toolbar" : "tool"
        let tones = app.descendants(matching: .any)["\(prefix)-segments-tonal-tones"]
        XCTAssertTrue(tones.waitForExistence(timeout: 10), "Tonal range shows its tone presets")
        XCTAssertEqual(tones.frame.height, inline ? 24 : 36, accuracy: 1)
        let segments = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "\(prefix)-segment-tonal-tones-"))
        XCTAssertEqual(segments.count, 6)
        let highlights = app.buttons["\(prefix)-segment-tonal-tones-4"]
        workspaceActivate(highlights)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: highlights)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "tonal-highlights")
        workspaceActivate(app.buttons["\(prefix)-segment-tonal-tones-5"])
        let range = app.descendants(matching: .any)["\(prefix)-range-tonal"]
        XCTAssertTrue(range.waitForExistence(timeout: 10), "Custom shows one range field")
        XCTAssertEqual(range.frame.height, 28, accuracy: 1)
        if inline { XCTAssertGreaterThanOrEqual(range.frame.width, 279) }
        let track = app.descendants(matching: .any)["\(prefix)-range-track"].firstMatch
        XCTAssertGreaterThan(track.frame.width, range.frame.width * 0.5, "The track takes most of the field")
        let lower = app.buttons["number-value-\(prefix)-tonal_lower"], upper = app.buttons["number-value-\(prefix)-tonal_upper"]
        XCTAssertLessThanOrEqual(lower.frame.width, 48); XCTAssertLessThanOrEqual(upper.frame.width, 48)
        func value(_ element: XCUIElement) -> Double { Double((element.value as? String ?? "").replacingOccurrences(of: "−", with: "-")) ?? .nan }
        let handle = app.descendants(matching: .any)["\(prefix)-range-handle-tonal_lower"].firstMatch
        let before = value(lower), otherBefore = value(upper)
        let from = handle.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        #if os(macOS)
        from.click(forDuration: 0.05, thenDragTo: from.withOffset(CGVector(dx: -30, dy: 0)))
        #else
        from.press(forDuration: 0.05, thenDragTo: from.withOffset(CGVector(dx: -30, dy: 0)))
        #endif
        expectation(for: NSPredicate { _, _ in value(lower) < before }, evaluatedWith: lower)
        waitForExpectations(timeout: 10)
        XCTAssertEqual(value(upper), otherBefore, "Dragging one handle leaves the other endpoint")
        workspaceActivate(lower)
        let entry = app.textFields["number-entry-\(prefix)-tonal_lower"]
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        #if os(macOS)
        entry.typeKey("a", modifierFlags: .command)
        #endif
        entry.typeText("-20\n")
        expectation(for: NSPredicate { _, _ in value(lower) == -20 }, evaluatedWith: lower)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "tonal-custom-range")
        if !inline { workspaceActivate(more) }
        editorTool("Eraser", in: app)
        XCTAssertTrue(range.waitForNonExistence(timeout: 10), "Leaving the tool removes the range")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
