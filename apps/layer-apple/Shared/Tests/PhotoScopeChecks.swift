import XCTest

extension XCTestCase {
    @MainActor func checkPhotoScopes(in app: XCUIApplication, theme: String) {
        func launch(_ effect: String) {
            useFreshStorage(app)
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:photographer"}},{"type":"effect","action":{"op":"insert","effect":"\#(effect)"}}]"#
            app.launch()
            let canvas = app.descendants(matching: .any)["canvas"].firstMatch
            expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
            waitForExpectations(timeout: 30)
        }
        func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        func press(_ frame: CGRect) {
            let viewport = workspaceViewport(in: app)
            viewport.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: frame.midX - viewport.frame.minX,
                dy: frame.midY - viewport.frame.minY)).clickOrTap()
        }
        launch("levels")
        XCTAssertTrue(element("scope-histogram").waitForExistence(timeout: 30), "Photo shows the Histogram above Properties")
        let status = app.staticTexts["scope-histogram-status"]
        expectation(for: NSPredicate { _, _ in !status.label.isEmpty || !((status.value as? String) ?? "").isEmpty }, evaluatedWith: status)
        waitForExpectations(timeout: 30)
        XCTAssertTrue(element("scope-histogram-chart").exists)
        XCTAssertTrue(app.buttons["scope-histogram-channel"].exists && app.buttons["scope-histogram-source"].exists)
        XCTAssertTrue(element("scope-tonal_histogram").waitForExistence(timeout: 10), "Levels shows its tonal histogram")
        XCTAssertTrue(app.buttons["property-action-auto_levels"].exists, "Levels offers Auto")
        let calibration = app.buttons["property-action-group-calibration"]
        XCTAssertTrue(calibration.exists, "Levels offers the calibration pickers")
        let frame = calibration.frame
        workspaceActivate(calibration)
        XCTAssertTrue(app.buttons["property-calibrate-gray"].waitForExistence(timeout: 5))
        attachEditor(in: app, name: "photo-scopes-levels-\(theme)")
        press(frame)
        XCTAssertTrue(app.buttons["property-calibrate-gray"].waitForNonExistence(timeout: 5))
        let highlights = app.buttons["scope-histogram-highlights"]
        XCTAssertFalse(highlights.isSelected)
        workspaceActivate(highlights)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: highlights)
        waitForExpectations(timeout: 10)
        workspaceActivate(app.buttons["panel-tab-waveform"])
        XCTAssertTrue(element("scope-waveform-chart").waitForExistence(timeout: 10), "The Waveform shares the scope column")
        attachEditor(in: app, name: "photo-scopes-waveform-\(theme)")
        app.terminate()

        launch("curves")
        XCTAssertTrue(app.staticTexts["scope-tonal_histogram-status"].waitForExistence(timeout: 30), "Curves shows its histogram status")
        XCTAssertTrue(app.buttons["scope-tonal_histogram-highlights"].exists, "Curves offers the clipping overlays")
        XCTAssertTrue(app.buttons["property-action-target_curve"].exists, "Curves offers targeted adjustment")
        attachEditor(in: app, name: "photo-scopes-curves-\(theme)")
        app.terminate()

        launch("color_lookup")
        let choice = app.buttons["property-resource-choice"]
        XCTAssertTrue(choice.waitForExistence(timeout: 30), "Color lookup offers its tables")
        XCTAssertTrue(app.buttons["import-lookup"].exists, "Color lookup offers .cube import")
        let original = choice.value as? String
        workspaceActivate(choice); workspaceActivate(app.buttons["property-resource-choice-option-1"])
        expectation(for: NSPredicate { _, _ in choice.value as? String != original }, evaluatedWith: choice)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "photo-scopes-lookup-\(theme)")
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in choice.value as? String == original }, evaluatedWith: choice)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
