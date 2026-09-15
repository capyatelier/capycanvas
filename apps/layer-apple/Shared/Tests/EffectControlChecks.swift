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
        func expectPixels(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
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
            editorHistory("Undo", in: app); expectPixels(before)
            editorHistory("Redo", in: app); expectPixels(after)
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
            XCTAssertTrue(field.waitForExistence(timeout: 5)); field.typeText(name); expectValue(field, name)
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
            workspaceActivate(app.buttons["layer-Delete selected layers"]); expectPixels(blue)
            history(before: filtered, after: blue)
            expectLayers(2)
        }

        addFilter("Brightness", id: "brightness_contrast"); expectPixels(blue)
        edit("property-brightness", "25 + 25", expected: "50")
        let brighter = changed(from: blue)
        XCTAssertGreaterThan(brighter[0], blue[0]); XCTAssertGreaterThan(brighter[1], blue[1])
        history(before: blue, after: brighter)
        expectValue(app.buttons["number-value-property-brightness"], "50")
        attachEditor(in: app, name: "filter-brightness-artwork")
        removeFilter(brighter)

        addFilter("Curves", id: "curves"); expectPixels(blue)
        workspaceActivate(app.buttons["property-channel"])
        workspaceActivate(app.buttons["property-channel-option-1"])
        expectValue(app.buttons["property-channel"], "Red")
        let curve = app.descendants(matching: .any)["effect-curve"].firstMatch
        // The plot can be taller than the panel. Its insertion point must be
        // visible; requiring the entire plot to fit cannot be met by scrolling.
        let insertion = CGPoint(x: curve.frame.midX, y: curve.frame.minY + curve.frame.height * 0.25)
        XCTAssertTrue(app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch.frame.contains(insertion))
        let point = curve.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.25))
        #if os(macOS)
        point.click()
        #else
        point.tap()
        #endif
        expectation(for: NSPredicate(format: "label == %@", "Red, 3 points"), evaluatedWith: curve)
        waitForExpectations(timeout: 10)
        let curved = changed(from: blue)
        XCTAssertGreaterThan(curved[0], blue[0])
        XCTAssertEqual(curved[1], blue[1]); XCTAssertEqual(curved[2], blue[2])
        let nearPoint = point.withOffset(CGVector(dx: 5, dy: 5))
        #if os(macOS)
        nearPoint.click()
        #else
        nearPoint.tap()
        #endif
        expectPixels(curved)
        history(before: blue, after: curved)
        attachEditor(in: app, name: "filter-red-curve-selection")
        let destination = curve.coordinate(withNormalizedOffset: CGVector(dx: 0.65, dy: 0.4))
        let destinationPoint = CGPoint(x: curve.frame.minX + curve.frame.width * 0.65,
            y: curve.frame.minY + curve.frame.height * 0.4)
        XCTAssertTrue(app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch.frame.contains(destinationPoint))
        nearPoint.press(forDuration: 0.05, thenDragTo: destination.withOffset(CGVector(dx: 5, dy: 5)),
            withVelocity: .slow, thenHoldForDuration: 0.1)
        let moved = changed(from: curved)
        XCTAssertLessThan(moved[0], curved[0]); XCTAssertEqual(moved[1], curved[1]); XCTAssertEqual(moved[2], curved[2])
        XCTAssertEqual(curve.label, "Red, 3 points", "Dragging an existing point must not insert another")
        editorHistory("Undo", in: app); expectPixels(curved)
        #if os(macOS)
        nearPoint.click()
        #else
        nearPoint.tap()
        #endif
        expectPixels(curved)
        editorHistory("Redo", in: app); expectPixels(moved)
        attachEditor(in: app, name: "filter-red-curve-drag")
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
        workspaceActivate(app.buttons["curve-remove"])
        expectPixels(blue); history(before: moved, after: blue)
        editorHistory("Undo", in: app); expectPixels(moved)
        workspaceActivate(app.buttons["curve-reset"])
        expectPixels(blue); history(before: moved, after: blue)
        workspaceActivate(app.buttons["layer-Delete selected layers"])
        expectLayers(2)

        addFilter("Gradient Map", id: "gradient_map")
        let gray = changed(from: blue)
        XCTAssertEqual(gray[0], gray[1]); XCTAssertEqual(gray[1], gray[2])
        let reverse = app.descendants(matching: .any)["property-reverse"].firstMatch
        func toggleReverse(_ enabled: Bool) {
            // The native switch's accessibility frame includes its label.
            // Activate the visible trailing switch, not the blank row center.
            let track = reverse.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5))
            #if os(macOS)
            track.click()
            // AppKit exposes the switch value as a number; UIKit uses text.
            expectation(for: NSPredicate(format: "value == %d", enabled ? 1 : 0), evaluatedWith: reverse)
            waitForExpectations(timeout: 10)
            #else
            track.tap()
            expectValue(reverse, enabled ? "1" : "0")
            #endif
        }
        reveal(reverse); toggleReverse(true)
        let reversed = changed(from: gray)
        history(before: gray, after: reversed)
        toggleReverse(false); expectPixels(gray)
        let color = app.buttons["gradient-stop-color"]
        reveal(color); workspaceActivate(color)
        edit("gradient-stop-rgba-0", "100", expected: "100.0 %")
        let red = changed(from: gray)
        XCTAssertGreaterThan(red[0], gray[0]); XCTAssertEqual(red[1], gray[1]); XCTAssertEqual(red[2], gray[2])
        history(before: gray, after: red)
        attachEditor(in: app, name: "filter-gradient-color-artwork")
        workspaceActivate(color)
        let gradient = app.descendants(matching: .any)["effect-gradient"].firstMatch
        reveal(gradient)
        let middle = gradient.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 43.0 / 52))
        #if os(macOS)
        middle.click()
        #else
        middle.tap()
        #endif
        expectation(for: NSPredicate(format: "label ENDSWITH %@", ", 3 stops"), evaluatedWith: gradient)
        waitForExpectations(timeout: 10); expectPixels(red)
        let nearMiddle = middle.withOffset(CGVector(dx: 3, dy: 0))
        #if os(macOS)
        nearMiddle.click()
        #else
        nearMiddle.tap()
        #endif
        expectPixels(red); expectValue(app.buttons["number-value-gradient-position"], "50.0 %")
        let stopDestination = gradient.coordinate(withNormalizedOffset:
            CGVector(dx: (6 + (gradient.frame.width - 12) * 0.7) / gradient.frame.width, dy: 43.0 / 52))
        middle.press(forDuration: 0.05, thenDragTo: stopDestination, withVelocity: .slow, thenHoldForDuration: 0.1)
        let shifted = changed(from: red)
        XCTAssertEqual(shifted[0], red[0]); XCTAssertLessThan(shifted[1], red[1]); XCTAssertLessThan(shifted[2], red[2])
        XCTAssertTrue(gradient.label.hasSuffix(", 3 stops"), "Dragging a stop must not insert another")
        history(before: red, after: shifted)
        attachEditor(in: app, name: "filter-gradient-stop-drag")
        reveal(app.buttons["gradient-remove"]); workspaceActivate(app.buttons["gradient-remove"])
        expectPixels(red); history(before: shifted, after: red)
        editorHistory("Undo", in: app); expectPixels(shifted)
        workspaceActivate(app.buttons["gradient-reset"])
        expectPixels(gray); history(before: shifted, after: gray)
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
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 10))
            #if os(macOS)
            element.click()
            #else
            element.tap()
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
            activate(app.buttons["filter-search-toggle"])
            let field = app.textFields["filter-search"]
            XCTAssertTrue(field.waitForExistence(timeout: 5))
            field.typeText(text); expect(field, text)
            XCTAssertEqual(canvas.frame.minY, before.minY, accuracy: 1, "Search keyboard must not translate the canvas")
            XCTAssertEqual(canvas.frame.height, before.height, accuracy: 1, "Search keyboard must not resize the canvas")
            XCTAssertTrue(viewport.contains(field.frame), "Filter search must remain onscreen with the keyboard open")
        }
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 20)); expect(canvas, "Metal ready")
        activate(app.buttons["panel-tab-adjustments"])
        let category = app.buttons["filter-category"]
        for index in [1, 0] {
            activate(category)
            let option = app.buttons["filter-category-option-\(index)"]
            XCTAssertTrue(option.waitForExistence(timeout: 5))
            let label = option.label
            activate(option); expect(category, label)
        }
        search("Gaussian Blur")
        let blur = app.buttons["adjustment-gaussian_blur"]
        XCTAssertTrue(blur.waitForExistence(timeout: 10)); expect(blur, "Preview ready")
        activate(blur)
        activate(app.buttons["number-value-property-sigma"])
        let sigma = app.textFields["number-entry-property-sigma"]
        XCTAssertTrue(sigma.waitForExistence(timeout: 10))
        sigma.typeText("2 + 3\n"); expect(app.buttons["number-value-property-sigma"], "5.0 px")

        activate(app.buttons["panel-tab-adjustments"])
        // Close search to clear its shared query, then open a fresh search.
        activate(app.buttons["filter-search-toggle"]); search("Curves")
        activate(app.buttons["adjustment-curves"])
        let curve = app.descendants(matching: .any)["effect-curve"].firstMatch
        XCTAssertTrue(curve.waitForExistence(timeout: 10)); expectGraphic(curve, "2 points")
        let point = curve.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.25))
        #if os(macOS)
        point.click()
        #else
        point.tap()
        #endif
        expectGraphic(curve, "3 points")
        activate(app.buttons["curve-reset"]); expectGraphic(curve, "2 points")

        activate(app.buttons["panel-tab-adjustments"])
        activate(app.buttons["filter-search-toggle"]); search("Gradient Map")
        activate(app.buttons["adjustment-gradient_map"])
        let gradient = app.descendants(matching: .any)["effect-gradient"].firstMatch
        XCTAssertTrue(gradient.waitForExistence(timeout: 10)); expectGraphic(gradient, "2 stops")
        let stop = gradient.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.4))
        #if os(macOS)
        stop.click()
        #else
        stop.tap()
        #endif
        expectGraphic(gradient, "3 stops")
        // Select the actual returned stop, then edit its shared numeric value.
        #if os(macOS)
        stop.click()
        #else
        stop.tap()
        #endif
        let position = app.textFields["number-entry-gradient-position"]
        activate(app.buttons["number-value-gradient-position"])
        XCTAssertTrue(position.waitForExistence(timeout: 5))
        position.typeText("25\n"); expect(app.buttons["number-value-gradient-position"], "25.0 %")
        activate(app.buttons["gradient-reset"]); expectGraphic(gradient, "2 stops")
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
