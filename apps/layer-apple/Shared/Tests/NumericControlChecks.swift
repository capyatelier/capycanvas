import XCTest

extension XCTestCase {
    @MainActor func checkInlineLayerOpacity(in app: XCUIApplication) {
        capturePaintEditor(in: app)
        let value = app.buttons["number-value-layer-opacity"]
        let entry = app.textFields["number-entry-layer-opacity"]
        let property = app.buttons["number-value-property-opacity"]
        func expect(_ text: String) {
            expectation(for: NSPredicate(format: "value == %@", text), evaluatedWith: value)
            waitForExpectations(timeout: 5)
        }
        func command(_ name: String) {
            workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", name)).firstMatch)
        }
        #if os(macOS)
        let rows = app.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        #else
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        #endif
        let originalID = rows.element(boundBy: 0).identifier
        let original = rows.matching(identifier: originalID).firstMatch.buttons["Edit layer content"]
        expect("100")
        // UIKit accessibility reports the glyph bounds for this SwiftUI
        // readout. Compare complete control geometry in the layout fixture.
        workspaceActivate(value)
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeText("25 + 25\n"); expect("50")
        XCTAssertEqual(property.value as? String, "50.0 %")
        command("Undo"); expect("100"); command("Redo"); expect("50")
        workspaceActivate(value); entry.typeText("2 * (\n")
        XCTAssertTrue(entry.exists); XCTAssertEqual(entry.value as? String, "2 * (")
        XCTAssertEqual(property.value as? String, "50.0 %", "Invalid expressions must preserve opacity")
        entry.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: 5) + "20 + 22\n")
        expect("42"); XCTAssertEqual(property.value as? String, "42.0 %")

        let count = rows.count
        workspaceActivate(app.buttons["layer-New layer"])
        expectation(for: NSPredicate { _, _ in rows.count == count + 1 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let addedID = rows.element(boundBy: 0).identifier
        let added = rows.matching(identifier: addedID).firstMatch.buttons["Edit layer content"]
        expect("100")
        workspaceActivate(value); entry.typeText("7 +")
        #if os(iOS)
        let keyboard = app.keyboards.firstMatch
        XCTAssertTrue(keyboard.waitForExistence(timeout: 5))
        XCTAssertFalse(keyboard.frame.intersects(entry.frame), "Layer opacity must stay above the keyboard")
        XCTAssertFalse(keyboard.frame.intersects(original.frame), "Layer switching must stay available while editing")
        XCTAssertEqual(entry.value as? String, "7 +")
        attachEditor(in: app, name: "layer-opacity-draft-switch")
        #endif
        workspaceActivate(original); expect("42")
        XCTAssertFalse(entry.exists, "An unfinished draft must not move to another layer")
        workspaceActivate(added); expect("100")
        let track = app.descendants(matching: .any)["number-track-layer-opacity"].firstMatch
        workspaceActivate(track)
        expect("50")
        command("Undo"); expect("100")
        let start = track.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.5))
        let end = track.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.5))
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.1)
        expectation(for: NSPredicate(format: "value != %@", "100"), evaluatedWith: value)
        waitForExpectations(timeout: 5)
        let dragged = value.value as? String ?? ""
        XCTAssertNotNil(Double(dragged), "The completed slider drag must publish a numeric value")
        command("Undo"); expect("100")
        command("Redo"); expect(dragged)
        workspaceActivate(original); expect("42")
        let propertyEntry = app.textFields["number-entry-property-opacity"]
        let properties = app.descendants(matching: .any)["layer-properties"].firstMatch
        for draft in ["", "37", "2 + (\n"] {
            if !draft.isEmpty {
                workspaceActivate(property)
                XCTAssertTrue(propertyEntry.waitForExistence(timeout: 5))
                propertyEntry.typeText(draft)
                XCTAssertEqual(propertyEntry.value as? String, draft.trimmingCharacters(in: .newlines))
            }
            let label = properties.staticTexts["Opacity"].firstMatch
            #if os(macOS)
            label.rightClick()
            let reset = app.menuItems["Reset"]
            #else
            label.press(forDuration: 0.7)
            let reset = app.buttons["Reset"]
            #endif
            XCTAssertTrue(reset.waitForExistence(timeout: 5)); XCTAssertTrue(reset.isEnabled)
            workspaceActivate(reset)
            expect("100")
            XCTAssertTrue(propertyEntry.waitForNonExistence(timeout: 5), "Reset must discard the property draft")
            XCTAssertFalse(app.staticTexts["number-error-property-opacity"].exists)
            XCTAssertEqual(property.value as? String, "100.0 %")
            command("Undo"); expect("42")
            command("Redo"); expect("100")
            workspaceActivate(value); entry.typeText("42\n"); expect("42")
        }
        attachEditor(in: app, name: "property-opacity-reset")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkBlendChoices(in app: XCUIApplication) {
        capturePaintEditor(in: app)
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
        func open(_ control: XCUIElement) {
            let identifier = control.identifier
            workspaceActivate(control)
            let menu = app.descendants(matching: .any)["editor-action-menu"].firstMatch
            XCTAssertTrue(menu.waitForExistence(timeout: 5))
            XCTAssertTrue(workspaceViewport(in: app).frame.contains(menu.frame), "The choice menu must fit inside its editor window")
            attachEditor(in: app, name: identifier + "-menu")
        }
        expect("Normal")
        open(property)
        let multiply = app.buttons["property-blend-option-1"]
        XCTAssertTrue(multiply.waitForExistence(timeout: 5)); workspaceActivate(multiply)
        expect("Multiply"); command("Undo"); expect("Normal")
        open(compact)
        let screen = app.buttons["layer-blend-option-2"]
        XCTAssertTrue(screen.waitForExistence(timeout: 5)); workspaceActivate(screen)
        expect("Screen"); command("Undo"); expect("Normal"); command("Redo"); expect("Screen")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func waitForLayerPreviews(in app: XCUIApplication) {
        let thumbnails = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-thumbnail-"))
        let visible = thumbnails.allElementsBoundByIndex.filter(\.isHittable)
        XCTAssertFalse(visible.isEmpty)
        for thumbnail in visible {
            expectation(for: NSPredicate(format: "value == %@", "Preview ready"), evaluatedWith: thumbnail)
        }
        waitForExpectations(timeout: 30)
    }

    @MainActor func capturePaintEditor(in app: XCUIApplication, scenario: String = "paint-expanded", theme: String = "light") {
        #if os(macOS)
        // AppKit may restore this isolated test window partly offscreen after
        // earlier native window journeys. Move its unused header into view
        // before resolving any control hit points.
        let initialWindow = app.windows.firstMatch
        XCTAssertTrue(initialWindow.waitForExistence(timeout: 20))
        if initialWindow.frame.minX < 0 || initialWindow.frame.minY < 0 {
            let start = initialWindow.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.02))
            start.click(forDuration: 0.1, thenDragTo: start.withOffset(CGVector(
                dx: max(0, 20 - initialWindow.frame.minX), dy: max(0, 30 - initialWindow.frame.minY))))
            expectation(for: NSPredicate { _, _ in initialWindow.frame.minX >= 0 && initialWindow.frame.minY >= 0 }, evaluatedWith: initialWindow)
            waitForExpectations(timeout: 10)
        }
        #endif
        let paint = app.buttons["workspace-switch-builtin:workspace:illustrator"]
        XCTAssertTrue(paint.waitForExistence(timeout: 30))
        if !paint.isSelected { workspaceActivate(paint) }
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: paint)
        waitForExpectations(timeout: 10)
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
        for panel in ["toolbar", "commands", "brushes", "tool_settings", "sizes", "color", "stats", "navigator", "properties", "adjustments", "layers"] {
            let control = panel == "toolbar" || panel == "commands"
                ? app.descendants(matching: .any)["toolbar-options-" + panel].firstMatch
                : app.buttons["panel-tab-" + panel]
            XCTAssertTrue(control.waitForExistence(timeout: 10),
                "The complete default workspace must expose \(panel)")
        }
        let opacity = app.buttons["number-value-layer-opacity"]
        XCTAssertTrue(opacity.waitForExistence(timeout: 10))
        expectation(for: NSPredicate { _, _ in
            (opacity.value as? String)?.isEmpty == false
        }, evaluatedWith: opacity)
        waitForExpectations(timeout: 10)
        let overview = app.descendants(matching: .any)["navigator-overview"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Live preview"), evaluatedWith: overview)
        waitForExpectations(timeout: 10)
        // GPU submission and Navigator readiness precede thumbnail readback.
        // Wait for the real visible images, not the empty input-colored boxes.
        if app.launchEnvironment["CAPY_CAPTURE_PROBE"] == "1" {
            waitForLayerPreviews(in: app)
        }
        if scenario == "paint-expanded" {
            #if os(macOS)
            app.menuBars.menuBarItems["View"].click()
            app.menuItems["Fit canvas"].firstMatch.click()
            #else
            workspaceActivate(app.buttons["menu-View"])
            workspaceActivate(app.buttons["command-fit_canvas"])
            #endif
        }
        #if os(macOS)
        let window = app.windows.firstMatch
        editorDocumentTitle(in: app).hover()
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
            ["scenario": scenario, "theme": theme, "viewport": [viewport.width, viewport.height],
             "workspace_bottom": 0]), uniformTypeIdentifier: "public.json")
        metadata.name = "complete-editor-geometry-" + scenario; metadata.lifetime = .keepAlways; add(metadata)
    }

    @MainActor func checkEditorControlLayout(in app: XCUIApplication) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 5))
            XCTAssertTrue(element.isHittable)
            element.clickOrTap()
        }
        capturePaintEditor(in: app)
        for command in ["zoom_out", "zoom_in", "rotate_left", "rotate_right", "flip_horizontal", "flip_vertical"] {
            XCTAssertTrue(app.buttons["navigator-" + command].isHittable)
        }
        // The shared zoom step is sqrt(2); four steps put the paper behind the
        // header on both viewport sizes, without changing document history.
        for _ in 0..<4 { activate(app.buttons["navigator-zoom_in"]) }
        capturePaintEditor(in: app, scenario: "paint-canvas-under-header")

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
        capturePaintEditor(in: app)
        let value = app.buttons["number-value-tool-size"]
        let entry = app.textFields["number-entry-tool-size"]
        workspaceActivate(value)
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeText("20 + 22\n")
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", "42"), evaluatedWith: value)
        waitForExpectations(timeout: 5)
        let sizePanel = app.buttons["number-value-Brush size"]
        // Tool and Brush size share one tab group in the current default layout.
        // Inspect the independent readout only after mounting its actual tab.
        workspaceActivate(app.buttons["panel-tab-sizes"])
        XCTAssertTrue(sizePanel.waitForExistence(timeout: 5))
        XCTAssertTrue((sizePanel.value as? String)?.hasPrefix("42") == true,
            "Tool Settings and Brush size must reflect the same accepted Rust edit")
        workspaceActivate(app.buttons["panel-tab-tool_settings"])
        workspaceActivate(app.buttons["number-increase-tool-size"])
        expectation(for: NSPredicate { _, _ in (value.value as? String)?.hasPrefix("42") == false }, evaluatedWith: value)
        waitForExpectations(timeout: 5)
        let accepted = value.value as? String
        workspaceActivate(value)
        entry.typeText("1 / 0\n")
        let error = app.staticTexts["number-error-tool-size"]
        XCTAssertTrue(error.waitForExistence(timeout: 5))
        workspaceActivate(app.buttons["panel-tab-sizes"])
        XCTAssertTrue(sizePanel.waitForExistence(timeout: 5))
        XCTAssertEqual(sizePanel.value as? String, accepted, "An invalid expression must preserve brush size")
        workspaceActivate(app.buttons["panel-tab-tool_settings"])
        // Changing tabs retires the draft. Re-enter an invalid expression to
        // check Escape/correction independently of that view-lifecycle behavior.
        workspaceActivate(value)
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
        workspaceActivate(app.buttons["tool-group-1"])
        workspaceActivate(app.buttons["brush-7"])
        XCTAssertTrue(app.buttons["brush-7"].isSelected, "The Marker brush must become selected")
        let markerSize = value.value as? String
        workspaceActivate(app.buttons["tool-group-0"])
        XCTAssertTrue(app.buttons["brush-1"].isSelected, "Returning to Pen must restore its selected subtool")
        XCTAssertEqual(value.value as? String, remembered, "Changing groups must preserve each brush's edited size")
        workspaceActivate(app.buttons["panel-tab-sizes"])
        let sizeEntry = app.textFields["number-entry-Brush size"]
        for draft in ["37", "2 * ("] {
            workspaceActivate(sizePanel)
            XCTAssertTrue(sizeEntry.waitForExistence(timeout: 5))
            sizeEntry.typeText(draft)
            workspaceActivate(app.buttons["tool-group-1"])
            XCTAssertTrue(app.buttons["brush-7"].isSelected)
            XCTAssertTrue(sizeEntry.waitForNonExistence(timeout: 5), "The draft must not follow a brush switch")
            XCTAssertEqual(sizePanel.value as? String, markerSize, "The old size draft must not edit Marker")
            XCTAssertFalse(app.staticTexts["number-error-Brush size"].exists)
            workspaceActivate(app.buttons["tool-group-0"])
        }
        attachEditor(in: app, name: "brush-size-draft-switch")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
