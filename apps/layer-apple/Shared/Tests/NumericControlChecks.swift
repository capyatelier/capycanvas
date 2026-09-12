import XCTest

extension XCTestCase {
    @MainActor func checkBlendChoices(in app: XCUIApplication) {
        captureDefaultEditor(in: app)
        let property = app.buttons["property-blend"], compact = app.buttons["layer-blend"]
        func expect(_ value: String) {
            for control in [property, compact] {
                expectation(for: NSPredicate(format: "value == %@", value), evaluatedWith: control)
            }
            waitForExpectations(timeout: 5)
        }
        func command(_ name: String) {
            let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", name)).firstMatch
            XCTAssertTrue(button.isEnabled)
            workspaceActivate(button)
        }
        expect("Normal")
        workspaceActivate(property)
        let multiply = app.buttons["property-blend-option-1"]
        XCTAssertTrue(multiply.waitForExistence(timeout: 5)); workspaceActivate(multiply)
        expect("Multiply"); command("Undo"); expect("Normal")
        workspaceActivate(compact)
        let screen = app.buttons["layer-blend-option-2"]
        XCTAssertTrue(screen.waitForExistence(timeout: 5)); workspaceActivate(screen)
        expect("Screen"); command("Undo"); expect("Normal"); command("Redo"); expect("Screen")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func captureDefaultEditor(in app: XCUIApplication, scenario: String = "initial") {
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        if app.launchEnvironment["CAPY_PERSISTENCE_NAMESPACE"] != nil {
            for name in ["painter", "illustrator", "photographer"] {
                let workspace = app.buttons["workspace-switch-builtin:workspace:" + name]
                XCTAssertTrue(workspace.waitForExistence(timeout: 10), "Full captures require every workspace segment")
                XCTAssertEqual(workspace.isSelected, name == "illustrator")
            }
        }
        for panel in ["toolbar", "commands", "brushes", "tool_settings", "sizes", "color", "navigator", "properties", "layers"] {
            let control = panel == "toolbar" || panel == "commands"
                ? app.descendants(matching: .any)["toolbar-options-" + panel].firstMatch
                : app.buttons["panel-tab-" + panel]
            XCTAssertTrue(control.waitForExistence(timeout: 10),
                "The complete default workspace must expose \(panel)")
        }
        let overview = app.descendants(matching: .any)["navigator-overview"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Live preview"), evaluatedWith: overview)
        waitForExpectations(timeout: 10)
        // GPU submission and Navigator readiness precede thumbnail readback.
        // Wait for the real visible images, not the empty input-colored boxes.
        if app.launchEnvironment["CAPY_CAPTURE_PROBE"] == "1" {
            let thumbnails = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-thumbnail-"))
            XCTAssertGreaterThan(thumbnails.count, 0)
            for thumbnail in thumbnails.allElementsBoundByIndex where thumbnail.isHittable {
                expectation(for: NSPredicate(format: "value == %@", "Preview ready"), evaluatedWith: thumbnail)
            }
            waitForExpectations(timeout: 30)
        }
        #if os(macOS)
        let window = app.windows.firstMatch
        // Open and close panel configuration to dismiss native help without
        // touching ink. Empty header space can forward contacts to the canvas.
        app.buttons["panel-tab-brushes"].click()
        let closeConfiguration = app.buttons["close-panel-configuration"]
        XCTAssertTrue(closeConfiguration.waitForExistence(timeout: 5))
        closeConfiguration.click()
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: closeConfiguration)
        waitForExpectations(timeout: 5)
        app.buttons["panel-tab-brushes"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).hover()
        let viewport = window.frame
        #else
        let viewport = app.frame
        #endif
        for command in ["Undo", "Redo"] {
            let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", command)).firstMatch
            XCTAssertTrue(button.exists)
            XCTAssertFalse(button.isEnabled, "The initial capture must have no drawing history")
        }
        #if os(macOS)
        let screenshot = window.screenshot()
        #else
        let screenshot = XCUIScreen.main.screenshot()
        #endif
        let initial = XCTAttachment(screenshot: screenshot)
        initial.name = "complete-editor-" + scenario; initial.lifetime = .keepAlways; add(initial)
        let metadata = XCTAttachment(data: try! JSONSerialization.data(withJSONObject:
            ["scenario": scenario, "theme": "light", "viewport": [viewport.width, viewport.height]]), uniformTypeIdentifier: "public.json")
        metadata.name = "complete-editor-geometry-" + scenario; metadata.lifetime = .keepAlways; add(metadata)
    }

    @MainActor func checkEditorControlLayout(in app: XCUIApplication) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 5))
            XCTAssertTrue(element.isHittable)
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        captureDefaultEditor(in: app)
        for command in ["zoom_out", "zoom_in", "rotate_left", "rotate_right", "flip_horizontal", "flip_vertical"] {
            XCTAssertTrue(app.buttons["navigator-" + command].isHittable)
        }
        // The shared zoom step is sqrt(2); four steps put the paper behind the
        // header on both viewport sizes, without changing document history.
        for _ in 0..<4 { activate(app.buttons["navigator-zoom_in"]) }
        captureDefaultEditor(in: app, scenario: "canvas-under-header")

        let choice = app.buttons["property-blend"]
        activate(choice)
        // Multiply is the shared catalog's second blend mode.
        activate(app.buttons["property-blend-option-1"])
        expectation(for: NSPredicate(format: "value == %@", "Multiply"), evaluatedWith: choice)
        waitForExpectations(timeout: 5)
        expectation(for: NSPredicate(format: "value == %@", "Multiply"), evaluatedWith: app.buttons["Layer blend mode"])
        waitForExpectations(timeout: 5)
        let undo = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", "Undo")).firstMatch
        activate(undo)
        expectation(for: NSPredicate(format: "value == %@", "Normal"), evaluatedWith: choice)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkNumericToolControls(in app: XCUIApplication) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 5))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        captureDefaultEditor(in: app)
        let value = app.buttons["number-value-tool-size"]
        let entry = app.textFields["number-entry-tool-size"]
        activate(value)
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeText("20 + 22\n")
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", "42"), evaluatedWith: value)
        waitForExpectations(timeout: 5)
        let sizePanel = app.buttons["number-value-Brush size"]
        // Tool and Brush size share one tab group in the current default layout.
        // Inspect the independent readout only after mounting its actual tab.
        activate(app.buttons["panel-tab-sizes"])
        XCTAssertTrue(sizePanel.waitForExistence(timeout: 5))
        XCTAssertTrue((sizePanel.value as? String)?.hasPrefix("42") == true,
            "Tool Settings and Brush size must reflect the same accepted Rust edit")
        activate(app.buttons["panel-tab-tool_settings"])
        activate(app.buttons["number-increase-tool-size"])
        expectation(for: NSPredicate { _, _ in (value.value as? String)?.hasPrefix("42") == false }, evaluatedWith: value)
        waitForExpectations(timeout: 5)
        let accepted = value.value as? String
        activate(value)
        entry.typeText("1 / 0\n")
        let error = app.staticTexts["number-error-tool-size"]
        XCTAssertTrue(error.waitForExistence(timeout: 5))
        activate(app.buttons["panel-tab-sizes"])
        XCTAssertTrue(sizePanel.waitForExistence(timeout: 5))
        XCTAssertEqual(sizePanel.value as? String, accepted, "An invalid expression must preserve brush size")
        activate(app.buttons["panel-tab-tool_settings"])
        // Changing tabs retires the draft. Re-enter an invalid expression to
        // check Escape/correction independently of that view-lifecycle behavior.
        activate(value)
        entry.typeText("1 / 0\n")
        XCTAssertTrue(error.waitForExistence(timeout: 5))
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
        let remembered = value.value as? String
        activate(app.buttons["tool-group-1"])
        activate(app.buttons["brush-7"])
        XCTAssertTrue(app.buttons["brush-7"].isSelected, "The Marker brush must become selected")
        activate(app.buttons["tool-group-0"])
        XCTAssertTrue(app.buttons["brush-1"].isSelected, "Returning to Pen must restore its selected subtool")
        XCTAssertEqual(value.value as? String, remembered, "Changing groups must preserve each brush's edited size")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
