import XCTest

extension XCTestCase {
    @MainActor func checkColorControls(in app: XCUIApplication, capture: (String, CGRect, [String: Any]) -> Void) {
        let scroll = app.scrollViews.containing(.any, identifier: "color-panel-controls").firstMatch
        func reveal(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 5))
            guard scroll.exists else { return }
            // Scroll the panel's padding, outside the wheel/slider contact area.
            // A fully visible wheel is necessary for normalized picker contacts.
            for _ in 0..<8 {
                let viewport = scroll.frame.insetBy(dx: 0, dy: 2), frame = element.frame
                if frame.minY >= viewport.minY && frame.maxY <= viewport.maxY { return }
                let upward = frame.maxY > viewport.maxY
                #if os(macOS)
                scroll.scroll(byDeltaX: 0, deltaY: upward ? -80 : 80)
                #else
                let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.99, dy: upward ? 0.8 : 0.2))
                let end = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.99, dy: upward ? 0.2 : 0.8))
                start.press(forDuration: 0.05, thenDragTo: end)
                #endif
            }
            XCTFail("Color control did not become fully visible: \(element.identifier)")
        }
        func activate(_ element: XCUIElement) {
            reveal(element)
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
            reveal(app.descendants(matching: .any)["color-wheel"].firstMatch)
            #if os(macOS)
            point(x,y).click()
            #else
            point(x,y).tap()
            #endif
        }
        func expect(_ index: Int, _ value: String) {
            expectation(for: NSPredicate(format: "value == %@", value), evaluatedWith: app.buttons["number-value-color-\(index)"])
            waitForExpectations(timeout: 5)
        }
        let wheel = app.descendants(matching: .any)["color-wheel"].firstMatch
        func captureColor(_ space: String) {
            let panel = app.descendants(matching: .any)["color-panel-controls"].firstMatch
            guard let encoded = panel.value as? String,
                  let state = try? JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [String: Any],
                  state["space"] as? String == space,
                  (state["rgba"] as? [Double])?.count == 4, state["hue"] is NSNumber else {
                XCTFail("Missing accepted Rust color for the pixel oracle"); return
            }
            capture(space, wheel.frame, state)
        }
        XCTAssertTrue(wheel.waitForExistence(timeout: 10))
        XCTAssertEqual(wheel.frame.width, wheel.frame.height, accuracy: 1)
        tap(0.95,0.5); expect(0,"150")
        captureColor("hsv")
        activate(app.buttons["color-space"])
        expectation(for: NSPredicate(format: "value == %@", "HLS"), evaluatedWith: app.buttons["color-space"])
        waitForExpectations(timeout: 5)
        tap(0.5,0.5); expect(1,"50"); expect(2,"33")
        captureColor("hls")
        activate(app.buttons["number-value-color-0"])
        let hue = app.textFields["number-entry-color-0"]
        XCTAssertTrue(hue.waitForExistence(timeout: 5))
        hue.typeText("90 + 90\n"); expect(0,"180")
        activate(app.buttons["color-background"]); expect(1,"100")
        activate(app.buttons["color-transparent"])
        XCTAssertTrue(app.buttons["color-transparent"].isSelected)
        // Starting in the hue ring must keep editing hue through the field;
        // picking exits transparency without cancelling this same contact.
        reveal(wheel)
        point(0.95,0.5).press(forDuration: 0.05, thenDragTo: point(0.5,0.95))
        expect(0,"240")
        XCTAssertTrue(app.buttons["color-background"].isSelected)
        tap(0.02,0.02); expect(0,"240")
        activate(app.buttons["color-swap"]); expect(0,"180")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func attachColorFixture(name: String, space: String, state: [String: Any], screenshot: XCUIScreenshot, viewport: CGRect, wheel: CGRect) {
        let shot = XCTAttachment(screenshot: screenshot)
        shot.name = name; shot.lifetime = .keepAlways; add(shot)
        let metadata: [String: Any] = ["space": space,
            "rgba": state["rgba"]!, "hue": state["hue"]!,
            "viewport": [viewport.width,viewport.height],
            "wheel": [wheel.minX-viewport.minX,wheel.minY-viewport.minY,wheel.width,wheel.height]]
        let data = try! JSONSerialization.data(withJSONObject: metadata, options: [.sortedKeys])
        let geometry = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        geometry.name = name + "-geometry"; geometry.lifetime = .keepAlways; add(geometry)
    }
}
