import XCTest

extension XCTestCase {
    @MainActor func checkColorControls(in app: XCUIApplication, capture: (String, CGRect, [String: Any]) -> Void) {
        let scroll = app.scrollViews.containing(.any, identifier: "color-panel-controls").firstMatch
        func reveal(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 5))
            guard scroll.exists else { return }
            // Scroll the panel's padding, outside the wheel contact area.
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
            // The readout's rectangular AX frame includes the excluded wheel.
            // Activate its visible label, which lies inside the curved button.
            if element.identifier == "color-readout" {
                let label = element.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.06))
                label.clickOrTap()
                return
            }
            element.clickOrTap()
        }
        func point(_ x: Double, _ y: Double) -> XCUICoordinate {
            app.descendants(matching: .any)["color-wheel"].firstMatch.coordinate(withNormalizedOffset: CGVector(dx: x, dy: y))
        }
        func tap(_ x: Double, _ y: Double) {
            reveal(app.descendants(matching: .any)["color-wheel"].firstMatch)
            point(x,y).clickOrTap()
        }
        let panel = app.descendants(matching: .any)["color-panel-controls"].firstMatch
        let wheel = app.descendants(matching: .any)["color-wheel"].firstMatch
        func state() -> [String: Any] {
            guard let encoded = wheel.value as? String,
                  let decoded = try? JSONSerialization.jsonObject(with: Data(encoded.utf8)) as? [String: Any] else { return [:] }
            return decoded
        }
        func expect(_ index: Int, _ value: Double) {
            // XCTest rounds touch positions and accessibility bounds to pixels.
            // At the wheel's minimum size that can move hue by about one degree.
            let tolerance = index == 0 ? 1.0 : 0.2
            let predicate = NSPredicate { _, _ in
                guard let values = state()["components"] as? [Double], values.indices.contains(index) else { return false }
                return abs(values[index] - value) < tolerance
            }
            let result = XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: predicate, object: panel)], timeout: 5)
            XCTAssertEqual(result, .completed, "Expected Color component \(index) = \(value); accepted state: \(state())")
        }
        func captureColor(_ shape: String) {
            let accepted = state()
            guard accepted["shape"] as? String == shape,
                  (accepted["rgba"] as? [Double])?.count == 4, accepted["hue"] is NSNumber else {
                XCTFail("Missing accepted Rust color for \(shape): \(String(describing: wheel.value))"); return
            }
            capture(shape, wheel.frame, accepted)
        }
        XCTAssertTrue(wheel.waitForExistence(timeout: 10))
        XCTAssertEqual(wheel.frame.width, wheel.frame.height, accuracy: 1)
        XCTAssertEqual(app.buttons["color-readout"].value as? String, "OKLCH")
        captureColor("circle")
        let paint = state()["rgba"] as? [Double]
        activate(app.buttons["color-readout"])
        expectation(for: NSPredicate(format: "value == %@", "RGB"), evaluatedWith: app.buttons["color-readout"])
        waitForExpectations(timeout: 5)
        XCTAssertEqual(state()["rgba"] as? [Double], paint, "Changing readout must not change paint")
        activate(app.buttons["color-shape-square"])
        tap(0.95,0.5); expect(0,150)
        captureColor("square")
        activate(app.buttons["color-shape-triangle"])
        tap(0.5,0.5); expect(1,50); expect(2,100.0/3)
        captureColor("triangle")
        tap(0.8897114,0.725); expect(0,180)
        activate(app.buttons["color-background"]); expect(1,100)
        activate(app.buttons["color-transparent"])
        XCTAssertTrue(app.buttons["color-transparent"].isSelected)
        // A hue contact remains latched through the field and transparent exit.
        reveal(wheel)
        point(0.95,0.5).press(forDuration: 0.05, thenDragTo: point(0.5,0.95))
        expect(0,240)
        XCTAssertTrue(app.buttons["color-background"].isSelected)
        tap(0.02,0.02); expect(0,240)
        activate(app.buttons["color-swap"]); expect(0,180)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func attachColorFixture(name: String, state: [String: Any], screenshot: XCUIScreenshot, viewport: CGRect, wheel: CGRect) {
        let shot = XCTAttachment(screenshot: screenshot)
        shot.name = name; shot.lifetime = .keepAlways; add(shot)
        let metadata: [String: Any] = ["space": state["space"]!, "shape": state["shape"]!,
            "marker_radius": min(10, max(6, wheel.width * 0.04)),
            "field_corner_radius": state["shape"] as? String == "square" ? min(6, wheel.width * 0.02) : 0,
            "rgba": state["rgba"]!, "hue": state["hue"]!,
            "viewport": [viewport.width,viewport.height],
            "wheel": [wheel.minX-viewport.minX,wheel.minY-viewport.minY,wheel.width,wheel.height]]
        let data = try! JSONSerialization.data(withJSONObject: metadata, options: [.sortedKeys])
        let geometry = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        geometry.name = name + "-geometry"; geometry.lifetime = .keepAlways; add(geometry)
    }
}
