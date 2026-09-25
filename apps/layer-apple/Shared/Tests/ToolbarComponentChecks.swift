import XCTest

extension XCTestCase {
    @MainActor func checkToolbarComponents(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let sketch = app.buttons["workspace-switch-builtin:workspace:painter"]
        workspaceActivate(sketch)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: sketch)
        waitForExpectations(timeout: 10)
        let sliders = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "component-slider-"))
        expectation(for: NSPredicate(format: "count == 2"), evaluatedWith: sliders)
        waitForExpectations(timeout: 15)
        let size = sliders.element(boundBy: 0), opacity = sliders.element(boundBy: 1)
        func tap(_ element: XCUIElement, _ offset: CGVector) {
            let point = element.coordinate(withNormalizedOffset: offset)
            #if os(macOS)
            point.click()
            #else
            point.tap()
            #endif
        }
        tap(size, CGVector(dx: 0.5, dy: 0.25))
        let preview = app.descendants(matching: .any)["brush-slider-preview"].firstMatch
        XCTAssertTrue(preview.waitForExistence(timeout: 10), "A slider tap opens the stamp preview")
        let caption = app.staticTexts["slider-preview-caption"]
        XCTAssertTrue(caption.waitForExistence(timeout: 5))
        func text(_ element: XCUIElement) -> String { element.label.isEmpty ? element.value as? String ?? "" : element.label }
        let first = text(caption)
        XCTAssertFalse(first.isEmpty, "The preview names the value")
        tap(size, CGVector(dx: 0.5, dy: 0.75))
        expectation(for: NSPredicate { _, _ in text(caption) != first }, evaluatedWith: caption)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "brush-size-preview")
        let bookmark = app.buttons["slider-bookmark"]
        XCTAssertEqual(bookmark.label, "Bookmark this value")
        workspaceActivate(bookmark)
        expectation(for: NSPredicate(format: "label == %@", "Remove bookmark"), evaluatedWith: bookmark)
        waitForExpectations(timeout: 10)
        workspaceActivate(bookmark)
        expectation(for: NSPredicate(format: "label == %@", "Bookmark this value"), evaluatedWith: bookmark)
        waitForExpectations(timeout: 10)
        tap(workspaceViewport(in: app), CGVector(dx: 0.6, dy: 0.6))
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: preview)
        waitForExpectations(timeout: 10)
        let start = opacity.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.8))
        let end = opacity.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.3))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        #else
        start.press(forDuration: 0.05, thenDragTo: end)
        #endif
        XCTAssertFalse(preview.exists, "A completed drag closes its preview")
        func order() -> [String] { sliders.allElementsBoundByIndex.map(\.identifier) }
        let original = order()
        let caps = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "slider-cap-"))
        func dragCap(hold: TimeInterval) {
            let from = caps.element(boundBy: 1).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            let to = caps.element(boundBy: 0).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1))
            #if os(macOS)
            from.click(forDuration: hold, thenDragTo: to)
            #else
            from.press(forDuration: hold, thenDragTo: to)
            #endif
        }
        dragCap(hold: 0.05)
        XCTAssertEqual(order(), original, "A quick cap drag never reorders")
        dragCap(hold: 0.8)
        expectation(for: NSPredicate { _, _ in order() == original.reversed() }, evaluatedWith: sliders)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(preview.exists)
        let photo = app.buttons["workspace-switch-builtin:workspace:photographer"]
        workspaceActivate(photo)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: photo)
        waitForExpectations(timeout: 10)
        let more = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "toolbar-more-")).firstMatch
        XCTAssertTrue(more.waitForExistence(timeout: 10), "Photo's command bar carries Tool Options")
        attachEditor(in: app, name: "photo-tool-options")
        let options = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "toolbar-component-")).firstMatch
        func openDisplayMenu() {
            let blank = options.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.5))
            #if os(macOS)
            blank.rightClick()
            #else
            blank.press(forDuration: 0.8)
            #endif
        }
        for _ in 0..<2 {
            openDisplayMenu()
            let sliders = app.buttons["menu-action-Show Sliders"]
            XCTAssertTrue(sliders.waitForExistence(timeout: 5), "Empty options space opens the display menu")
            workspaceActivate(sliders)
            expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: sliders)
            waitForExpectations(timeout: 5)
        }
        editorTool("Rectangle select", in: app)
        let segments = app.descendants(matching: .any)["toolbar-segments-selection-mode"]
        XCTAssertTrue(segments.waitForExistence(timeout: 10), "Selection modes join the options bar")
        let dropdown = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "toolbar-choice-")).firstMatch
        XCTAssertTrue(dropdown.exists, "Selection tools keep a list choice")
        XCTAssertEqual(segments.frame.height, 24, accuracy: 1, "Segments match the dropdown height")
        XCTAssertEqual(segments.frame.midY, dropdown.frame.midY, accuracy: 1, "Segments center with the dropdown")
        attachEditor(in: app, name: "photo-selection-options")
        workspaceActivate(more)
        XCTAssertTrue(app.descendants(matching: .any)["tool-drawer"].firstMatch.waitForExistence(timeout: 10), "More opens the tool drawer")
        attachEditor(in: app, name: "photo-tool-options-drawer")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
