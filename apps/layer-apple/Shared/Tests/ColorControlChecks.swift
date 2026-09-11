import XCTest

extension XCTestCase {
    @MainActor func checkColorControls(in app: XCUIApplication, capture: (String, CGRect) -> Void) {
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 5))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        func point(_ x: Double, _ y: Double) -> XCUICoordinate {
            app.descendants(matching: .any)["color-wheel"].firstMatch.coordinate(withNormalizedOffset: CGVector(dx: x, dy: y))
        }
        func tap(_ x: Double, _ y: Double) {
            #if os(macOS)
            point(x,y).click()
            #else
            point(x,y).tap()
            #endif
        }
        func expect(_ index: Int, _ value: String) {
            expectation(for: NSPredicate(format: "value == %@", value), evaluatedWith: app.textFields["number-entry-color-\(index)"])
            waitForExpectations(timeout: 5)
        }
        let wheel = app.descendants(matching: .any)["color-wheel"].firstMatch
        XCTAssertTrue(wheel.waitForExistence(timeout: 10))
        XCTAssertEqual(wheel.frame.width, wheel.frame.height, accuracy: 1)
        tap(0.95,0.5); expect(0,"150")
        capture("hsv", wheel.frame)
        activate(app.buttons["color-space"])
        expectation(for: NSPredicate(format: "value == %@", "HLS"), evaluatedWith: app.buttons["color-space"])
        waitForExpectations(timeout: 5)
        tap(0.5,0.5); expect(1,"50"); expect(2,"33")
        capture("hls", wheel.frame)
        let hue = app.textFields["number-entry-color-0"]
        activate(hue); hue.typeText("90 + 90\n"); expect(0,"180")
        activate(app.buttons["color-background"]); expect(1,"100")
        activate(app.buttons["color-transparent"])
        XCTAssertTrue(app.buttons["color-transparent"].isSelected)
        // Starting in the hue ring must keep editing hue through the field;
        // picking exits transparency without cancelling this same contact.
        point(0.95,0.5).press(forDuration: 0.05, thenDragTo: point(0.5,0.95))
        expect(0,"240")
        XCTAssertTrue(app.buttons["color-background"].isSelected)
        tap(0.02,0.02); expect(0,"240")
        activate(app.buttons["color-swap"]); expect(0,"180")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func attachColorFixture(name: String, space: String, screenshot: XCUIScreenshot, viewport: CGRect, wheel: CGRect) {
        let shot = XCTAttachment(screenshot: screenshot)
        shot.name = name; shot.lifetime = .keepAlways; add(shot)
        let metadata: [String: Any] = ["space": space,
            "rgba": space == "hsv" ? [0,1,0.5,1] : [1.0/3,2.0/3,0.5,1],
            "viewport": [viewport.width,viewport.height],
            "wheel": [wheel.minX-viewport.minX,wheel.minY-viewport.minY,wheel.width,wheel.height]]
        let data = try! JSONSerialization.data(withJSONObject: metadata, options: [.sortedKeys])
        let geometry = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        geometry.name = name + "-geometry"; geometry.lifetime = .keepAlways; add(geometry)
    }
}
