import XCTest

extension XCTestCase {
    @MainActor func checkFilterArtworkAndHistory(in app: XCUIApplication) {
        // Only the starting color/theme are seeded. Create artwork and effects
        // through native controls, then sample the actual displayed canvas.
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]},{"type":"customize","action":{"type":"set_panel_visible","panel":"adjustments","visible":true}}]"#
        app.launch(); capturePaintEditor(in: app)
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        let layers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        func pixels() -> Data { editorPixels(in: app) }
        func expectValue(_ element: XCUIElement, _ value: String) {
            expectation(for: NSPredicate(format: "value == %@", value), evaluatedWith: element)
            waitForExpectations(timeout: 10)
        }
        func expectLayers(_ count: Int) {
            expectation(for: NSPredicate(format: "count == %d", count), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
        }
        func changed(from before: Data) -> Data {
            expectation(for: NSPredicate { _, _ in pixels() != before }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            return pixels()
        }
        func history(before: Data, after: Data) {
            editorHistory("Undo", in: app); expectPixels(before, in: app)
            editorHistory("Redo", in: app); expectPixels(after, in: app)
        }
        func reveal(_ element: XCUIElement) {
            revealEditorControl(element, in: app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch)
        }
        func edit(_ id: String, _ text: String, expected: String) {
            let readout = app.buttons["number-value-" + id]
            reveal(readout); workspaceActivate(readout)
            let field = app.textFields["number-entry-" + id]
            XCTAssertTrue(field.waitForExistence(timeout: 5)); field.typeText(text + "\n")
            expectValue(readout, expected)
        }
        var searched = false
        func addFilter(_ name: String, id: String) {
            workspaceActivate(app.buttons["panel-tab-adjustments"])
            // Search remains in the shared picker while Properties is shown.
            // Clear the preceding query after returning to the Filters tab.
            if searched { workspaceActivate(app.buttons["filter-search-toggle"]) }
            workspaceActivate(app.buttons["filter-search-toggle"])
            let field = app.textFields["filter-search"]
            XCTAssertTrue(field.waitForExistence(timeout: 5)); typeConfirmed(field, name); expectValue(field, name)
            searched = true
            let filter = app.buttons["adjustment-" + id]
            XCTAssertTrue(filter.waitForExistence(timeout: 10)); expectValue(filter, "Preview ready")
            workspaceActivate(filter)
            expectLayers(3)
        }
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectation(for: NSPredicate { _, _ in let p = pixels(); return Int(p[2]) > Int(p[0]) + 50 }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let blue = pixels()
        func removeFilter(_ filtered: Data) {
            workspaceActivate(app.buttons["layer-Delete selected layers"]); expectPixels(blue, in: app)
            history(before: filtered, after: blue)
            expectLayers(2)
        }

        addFilter("Brightness", id: "brightness_contrast"); expectPixels(blue, in: app)
        edit("property-brightness", "25 + 25", expected: "50")
        let brighter = changed(from: blue)
        XCTAssertGreaterThan(brighter[0], blue[0]); XCTAssertGreaterThan(brighter[1], blue[1])
        history(before: blue, after: brighter)
        expectValue(app.buttons["number-value-property-brightness"], "50")
        attachEditor(in: app, name: "filter-brightness-artwork")
        removeFilter(brighter)

        addFilter("Curves", id: "curves"); expectPixels(blue, in: app)
        workspaceActivate(app.buttons["properties-page"])
        workspaceActivate(app.buttons["properties-page-option-1"])
        expectValue(app.buttons["properties-page"], "Red")
        let curve = app.descendants(matching: .any)["effect-curve"].firstMatch
        // The plot can be taller than the panel. Its insertion point must be
        // visible; requiring the entire plot to fit cannot be met by scrolling.
        let insertion = CGPoint(x: curve.frame.midX, y: curve.frame.minY + curve.frame.height * 0.25)
        XCTAssertTrue(app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch.frame.contains(insertion))
        let point = curve.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.25))
        point.clickOrTap()
        expectation(for: NSPredicate(format: "label == %@", "Red, 3 points"), evaluatedWith: curve)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.buttons["curve-reset"].waitForExistence(timeout: 5), "An edited curve offers reset on the chart")
        let curved = changed(from: blue)
        XCTAssertGreaterThan(curved[0], blue[0])
        XCTAssertEqual(Int(curved[1]), Int(blue[1]), accuracy: 1); XCTAssertEqual(Int(curved[2]), Int(blue[2]), accuracy: 1)
        let nearPoint = point.withOffset(CGVector(dx: 5, dy: 5))
        nearPoint.clickOrTap()
        expectPixels(curved, in: app)
        history(before: blue, after: curved)
        attachEditor(in: app, name: "filter-red-curve-selection")
        let destination = curve.coordinate(withNormalizedOffset: CGVector(dx: 0.65, dy: 0.4))
        let destinationPoint = CGPoint(x: curve.frame.minX + curve.frame.width * 0.65,
            y: curve.frame.minY + curve.frame.height * 0.4)
        XCTAssertTrue(app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch.frame.contains(destinationPoint))
        nearPoint.press(forDuration: 0.05, thenDragTo: destination.withOffset(CGVector(dx: 5, dy: 5)),
            withVelocity: .slow, thenHoldForDuration: 0.1)
        let moved = changed(from: curved)
        XCTAssertLessThan(moved[0], curved[0])
        XCTAssertEqual(Int(moved[1]), Int(curved[1]), accuracy: 1); XCTAssertEqual(Int(moved[2]), Int(curved[2]), accuracy: 1)
        XCTAssertEqual(curve.label, "Red, 3 points", "Dragging an existing point must not insert another")
        editorHistory("Undo", in: app); expectPixels(curved, in: app)
        nearPoint.clickOrTap()
        expectPixels(curved, in: app)
        editorHistory("Redo", in: app); expectPixels(moved, in: app)
        attachEditor(in: app, name: "filter-red-curve-drag")
        #if os(macOS)
        destination.doubleClick()
        #else
        destination.doubleTap()
        #endif
        expectPixels(blue, in: app); history(before: moved, after: blue)
        XCTAssertEqual(curve.label, "Red, 2 points", "Double-clicking a point removes it")
        editorHistory("Undo", in: app); expectPixels(moved, in: app)
        #if os(iOS)
        // A touch on the plot edits its curve. Scroll from the noninteractive
        // heading instead of starting the gesture inside that editing surface.
        let heading = app.descendants(matching: .any)["layer-properties"].firstMatch.staticTexts["Curves"]
        let scrollStart = heading.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        scrollStart.press(forDuration: 0.05, thenDragTo: scrollStart.withOffset(CGVector(dx: 0, dy: -160)),
            withVelocity: .slow, thenHoldForDuration: 0.1)
        #else
        reveal(app.buttons["curve-reset"])
        #endif
        workspaceActivate(app.buttons["curve-reset"])
        expectPixels(blue, in: app); history(before: moved, after: blue)
        func expectPoints(_ count: Int) {
            expectation(for: NSPredicate(format: "label == %@", "Red, \(count) points"), evaluatedWith: curve)
            waitForExpectations(timeout: 10)
        }
        let press = curve.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.35))
        let pressed = press.withOffset(CGVector(dx: 12, dy: -30))
        press.press(forDuration: 0.05, thenDragTo: pressed, withVelocity: .slow, thenHoldForDuration: 0.1)
        expectPoints(3)
        let inserted = changed(from: blue)
        editorHistory("Undo", in: app); expectPixels(blue, in: app); expectPoints(2)
        editorHistory("Redo", in: app); expectPixels(inserted, in: app); expectPoints(3)
        pressed.press(forDuration: 0.05, thenDragTo: curve.coordinate(withNormalizedOffset: CGVector(dx: 0.35, dy: 1.4)),
            withVelocity: .slow, thenHoldForDuration: 0.1)
        expectPoints(2); expectPixels(blue, in: app)
        editorHistory("Undo", in: app); expectPixels(inserted, in: app); expectPoints(3)
        editorHistory("Redo", in: app); expectPixels(blue, in: app)
        workspaceActivate(app.buttons["layer-Delete selected layers"])
        expectLayers(2)

        addFilter("Gradient Map", id: "gradient_map")
        let gray = changed(from: blue)
        XCTAssertEqual(gray[0], gray[1]); XCTAssertEqual(gray[1], gray[2])
        let reverse = app.buttons["gradient-reverse"]
        reveal(reverse); workspaceActivate(reverse)
        let reversed = changed(from: gray)
        history(before: gray, after: reversed)
        workspaceActivate(reverse); expectPixels(gray, in: app)
        let interpolation = app.buttons["gradient-interpolation"]
        reveal(interpolation); workspaceActivate(interpolation)
        workspaceActivate(app.buttons["gradient-interpolation-option-1"])
        _ = XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in pixels() != gray }, object: app)], timeout: 5)
        let linear = pixels()
        let color = app.buttons["gradient-stop-color"]
        reveal(color); workspaceActivate(color)
        let use = app.buttons["color-use"]
        XCTAssertTrue(use.waitForExistence(timeout: 5), "A stop color opens the shared Edit Color form")
        editColorValue("0-0", "255", in: app)
        workspaceActivate(use)
        XCTAssertTrue(use.waitForNonExistence(timeout: 10))
        let red = changed(from: linear)
        XCTAssertGreaterThan(red[0], linear[0]); XCTAssertEqual(Int(red[1]), Int(linear[1]), accuracy: 2); XCTAssertEqual(Int(red[2]), Int(linear[2]), accuracy: 2)
        history(before: linear, after: red)
        attachEditor(in: app, name: "filter-gradient-color-artwork")
        let gradient = app.descendants(matching: .any)["gradient-strip"].firstMatch
        let markers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "gradient-marker-"))
        reveal(gradient)
        let middle = gradient.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 39.0 / 46))
        middle.clickOrTap()
        expectation(for: NSPredicate { _, _ in markers.count == 3 }, evaluatedWith: gradient)
        waitForExpectations(timeout: 10); expectPixels(red, in: app)
        expectValue(app.buttons["number-value-gradient-position"], "50.0 %")
        XCTAssertTrue(app.buttons["gradient-remove"].isEnabled, "A new stop must be selected without another tap")
        let nearMiddle = middle.withOffset(CGVector(dx: 3, dy: 0))
        nearMiddle.clickOrTap()
        expectPixels(red, in: app); expectValue(app.buttons["number-value-gradient-position"], "50.0 %")
        let stopDestination = gradient.coordinate(withNormalizedOffset:
            CGVector(dx: (6 + (gradient.frame.width - 12) * 0.7) / gradient.frame.width, dy: 39.0 / 46))
        middle.press(forDuration: 0.05, thenDragTo: stopDestination, withVelocity: .slow, thenHoldForDuration: 0.1)
        let shifted = changed(from: red)
        XCTAssertEqual(Int(shifted[0]), Int(red[0]), accuracy: 2); XCTAssertLessThan(shifted[1], red[1]); XCTAssertLessThan(shifted[2], red[2])
        XCTAssertEqual(markers.count, 3, "Dragging a stop must not insert another")
        history(before: red, after: shifted)
        attachEditor(in: app, name: "filter-gradient-stop-drag")
        reveal(app.buttons["gradient-remove"]); workspaceActivate(app.buttons["gradient-remove"])
        expectPixels(red, in: app); history(before: shifted, after: red)
        editorHistory("Undo", in: app); expectPixels(shifted, in: app)
        workspaceActivate(app.buttons["gradient-reset"])
        expectPixels(gray, in: app); history(before: shifted, after: gray)
        removeFilter(gray)
        XCTAssertEqual(viewport.frame, originalFrame)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkFilterSearchPreviewAndProperties(in app: XCUIApplication) {
        var viewport: CGRect {
            #if os(macOS)
            app.windows.firstMatch.frame
            #else
            app.frame
            #endif
        }
        func expect(_ element: XCUIElement, _ value: String) {
            expectation(for: NSPredicate(format: "value == %@", value), evaluatedWith: element)
            waitForExpectations(timeout: 30)
        }
        func expectGraphic(_ element: XCUIElement, _ description: String) {
            expectation(for: NSPredicate(format: "label ENDSWITH %@", ", " + description), evaluatedWith: element)
            waitForExpectations(timeout: 30)
        }
        func search(_ text: String) {
            let canvas = app.descendants(matching: .any)["canvas"].firstMatch
            let before = canvas.frame
            workspaceActivate(app.buttons["filter-search-toggle"])
            let field = app.textFields["filter-search"]
            XCTAssertTrue(field.waitForExistence(timeout: 5))
            typeConfirmed(field, text); expect(field, text)
            XCTAssertEqual(canvas.frame.minY, before.minY, accuracy: 1, "Search keyboard must not translate the canvas")
            XCTAssertEqual(canvas.frame.height, before.height, accuracy: 1, "Search keyboard must not resize the canvas")
            XCTAssertTrue(viewport.contains(field.frame), "Filter search must remain onscreen with the keyboard open")
        }
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 20)); expect(canvas, "Canvas ready")
        workspaceActivate(app.buttons["panel-tab-adjustments"])
        let category = app.buttons["filter-category"]
        for index in [1, 0] {
            workspaceActivate(category)
            let option = app.buttons["filter-category-option-\(index)"]
            XCTAssertTrue(option.waitForExistence(timeout: 5))
            let label = option.label
            workspaceActivate(option); expect(category, label)
        }
        search("Gaussian Blur")
        let blur = app.buttons["adjustment-gaussian_blur"]
        XCTAssertTrue(blur.waitForExistence(timeout: 10)); expect(blur, "Preview ready")
        workspaceActivate(blur)
        workspaceActivate(app.buttons["number-value-property-sigma"])
        let sigma = app.textFields["number-entry-property-sigma"]
        XCTAssertTrue(sigma.waitForExistence(timeout: 10))
        sigma.typeText("2 + 3\n"); expect(app.buttons["number-value-property-sigma"], "5.0 px")

        workspaceActivate(app.buttons["panel-tab-adjustments"])
        // Close search to clear its shared query, then open a fresh search.
        workspaceActivate(app.buttons["filter-search-toggle"]); search("Curves")
        workspaceActivate(app.buttons["adjustment-curves"])
        let curve = app.descendants(matching: .any)["effect-curve"].firstMatch
        XCTAssertTrue(curve.waitForExistence(timeout: 10)); expectGraphic(curve, "2 points")
        let point = curve.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.25))
        point.clickOrTap()
        expectGraphic(curve, "3 points")
        let reset = app.buttons["curve-reset"]
        XCTAssertTrue(reset.waitForExistence(timeout: 10))
        #if os(iOS)
        let heading = app.descendants(matching: .any)["layer-properties"].firstMatch.staticTexts["Curves"]
        let scrollStart = heading.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        scrollStart.press(forDuration: 0.05, thenDragTo: scrollStart.withOffset(CGVector(dx: 0, dy: -160)),
            withVelocity: .slow, thenHoldForDuration: 0.1)
        #else
        revealEditorControl(reset, in: app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch)
        #endif
        workspaceActivate(reset); expectGraphic(curve, "2 points")

        workspaceActivate(app.buttons["panel-tab-adjustments"])
        workspaceActivate(app.buttons["filter-search-toggle"]); search("Gradient Map")
        workspaceActivate(app.buttons["adjustment-gradient_map"])
        let gradient = app.descendants(matching: .any)["gradient-strip"].firstMatch
        let markers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "gradient-marker-"))
        func expectStops(_ count: Int) {
            expectation(for: NSPredicate { _, _ in markers.count == count }, evaluatedWith: gradient)
            waitForExpectations(timeout: 30)
        }
        XCTAssertTrue(gradient.waitForExistence(timeout: 10)); expectStops(2)
        let stop = gradient.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.4))
        stop.clickOrTap()
        expectStops(3)
        // Select the actual returned stop, then edit its shared numeric value.
        stop.clickOrTap()
        let position = app.textFields["number-entry-gradient-position"]
        workspaceActivate(app.buttons["number-value-gradient-position"])
        XCTAssertTrue(position.waitForExistence(timeout: 5))
        position.typeText("25\n"); expect(app.buttons["number-value-gradient-position"], "25.0 %")
        let gradientReset = app.buttons["gradient-reset"]
        XCTAssertTrue(gradientReset.waitForExistence(timeout: 10))
        revealEditorControl(gradientReset, in: app.scrollViews.containing(.button, identifier: "gradient-reset").firstMatch)
        workspaceActivate(gradientReset); expectStops(2)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #if os(macOS)
        let screenshot = app.windows.firstMatch.screenshot()
        #else
        let screenshot = XCUIScreen.main.screenshot()
        #endif
        let shot = XCTAttachment(screenshot: screenshot)
        shot.name = "apple-filter-properties"; shot.lifetime = .keepAlways; add(shot)
        let metadata: [String: Any] = ["scenario": "filter-properties", "theme": "light", "viewport": [viewport.width, viewport.height]]
        let geometry = XCTAttachment(data: try! JSONSerialization.data(withJSONObject: metadata, options: [.sortedKeys]), uniformTypeIdentifier: "public.json")
        geometry.name = "apple-filter-properties-geometry"; geometry.lifetime = .keepAlways; add(geometry)
    }
}
