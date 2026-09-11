import XCTest

extension XCTestCase {
    @MainActor func checkFilterSearchPreviewAndProperties(in app: XCUIApplication) {
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
        func search(_ text: String) {
            let canvas = app.descendants(matching: .any)["canvas"].firstMatch
            let before = canvas.frame
            activate(app.buttons["filter-search-toggle"])
            let field = app.textFields["filter-search"]
            XCTAssertTrue(field.waitForExistence(timeout: 5))
            field.typeText(text)
            XCTAssertEqual(canvas.frame.minY, before.minY, accuracy: 1, "Search keyboard must not translate the canvas")
            XCTAssertEqual(canvas.frame.height, before.height, accuracy: 1, "Search keyboard must not resize the canvas")
            XCTAssertTrue(app.frame.contains(field.frame), "Filter search must remain onscreen with the keyboard open")
        }
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 20)); expect(canvas, "Metal ready")
        activate(app.buttons["panel-tab-adjustments"])
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
        XCTAssertTrue(curve.waitForExistence(timeout: 10)); expect(curve, "2 points")
        let point = curve.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.25))
        #if os(macOS)
        point.click()
        #else
        point.tap()
        #endif
        expect(curve, "3 points")
        activate(app.buttons["curve-reset"]); expect(curve, "2 points")

        activate(app.buttons["panel-tab-adjustments"])
        activate(app.buttons["filter-search-toggle"]); search("Gradient Map")
        activate(app.buttons["adjustment-gradient_map"])
        let gradient = app.descendants(matching: .any)["effect-gradient"].firstMatch
        XCTAssertTrue(gradient.waitForExistence(timeout: 10)); expect(gradient, "2 stops")
        let stop = gradient.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.4))
        #if os(macOS)
        stop.click()
        #else
        stop.tap()
        #endif
        expect(gradient, "3 stops")
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
        activate(app.buttons["gradient-reset"]); expect(gradient, "2 stops")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #if os(macOS)
        let viewport = app.windows.firstMatch.frame
        let screenshot = app.windows.firstMatch.screenshot()
        #else
        let viewport = app.frame
        let screenshot = XCUIScreen.main.screenshot()
        #endif
        let shot = XCTAttachment(screenshot: screenshot)
        shot.name = "apple-filter-properties"; shot.lifetime = .keepAlways; add(shot)
        let metadata: [String: Any] = ["scenario": "filter-properties", "theme": "light", "viewport": [viewport.width, viewport.height]]
        let geometry = XCTAttachment(data: try! JSONSerialization.data(withJSONObject: metadata, options: [.sortedKeys]), uniformTypeIdentifier: "public.json")
        geometry.name = "apple-filter-properties-geometry"; geometry.lifetime = .keepAlways; add(geometry)
    }
}
