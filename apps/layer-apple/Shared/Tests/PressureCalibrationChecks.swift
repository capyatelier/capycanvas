import XCTest

extension XCTestCase {
    @MainActor func checkTiltCalibration(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"reveal","id":"pen_tilt"}}]"#
        app.launch()
        let panel = app.descendants(matching: .any)["pen-tilt-dialog"].firstMatch
        func reveal(_ id: String, label: String) {
            let row = app.buttons["preference-" + id]
            if !row.waitForExistence(timeout: 20) {
                let search = app.textFields["settings-search"]
                workspaceActivate(search)
                search.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: (search.value as? String ?? "").count))
                search.typeText(label)
                workspaceActivate(app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", label)).firstMatch)
            }
            workspaceActivate(row)
        }
        func open() {
            reveal("pen_tilt", label: "Pen tilt")
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 10))
            XCTAssertTrue(panel.waitForExistence(timeout: 10))
        }
        func settings() {
            #if os(macOS)
            workspaceActivate(app.buttons["settings-button"])
            #else
            editorMenu(in: app, menu: "Edit", id: "settings", label: "Settings…")
            #endif
            workspaceActivate(app.staticTexts["settings-page-input"])
        }
        func pointCount(_ count: Int) {
            let graph = panel.descendants(matching: .any)["effect-curve"].firstMatch
            let expected = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", String(count)), object: graph)
            XCTAssertEqual(XCTWaiter.wait(for: [expected], timeout: 5), .completed)
        }
        let axisIDs = ["pen-tilt-reverse-horizontal", "pen-tilt-reverse-vertical", "pen-tilt-swap-axes"]
        func axis(_ id: String) -> XCUIElement { panel.descendants(matching: .any)[id].firstMatch }
        func values() -> [String: String] {
            Dictionary(uniqueKeysWithValues: axisIDs.map { id in
                let control = axis(id)
                XCTAssertTrue(control.waitForExistence(timeout: 5)); XCTAssertNotNil(control.value)
                return (id, String(describing: control.value))
            })
        }
        open(); pointCount(2)
        XCTAssertFalse(app.buttons["pen-tilt-apply"].isEnabled)
        XCTAssertFalse(panel.textFields.firstMatch.exists)
        let initial = values()
        var expected = initial
        for id in axisIDs {
            let control = axis(id), before = expected[id]!
            workspaceActivate(control)
            expectation(for: NSPredicate { _, _ in String(describing: control.value) != before }, evaluatedWith: control)
            waitForExpectations(timeout: 5)
            expected[id] = String(describing: control.value)
            XCTAssertEqual(values(), expected, "Axis choices must remain independent")
        }
        let graph = panel.descendants(matching: .any)["effect-curve"].firstMatch
        graph.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap(); pointCount(3)
        workspaceActivate(app.buttons["pen-tilt-reset"]); pointCount(2)
        XCTAssertEqual(values(), initial)
        for id in ["pen-tilt-firmer", "pen-tilt-lighter"] { workspaceActivate(app.buttons[id]) }
        workspaceActivate(axis(axisIDs[0])); XCTAssertNotEqual(values(), initial)
        graph.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap(); pointCount(3)
        XCTAssertFalse(app.buttons["pen-tilt-apply"].isEnabled)
        let original = panel.frame
        let title = panel.staticTexts["Pen tilt"].firstMatch
        let press = title.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        press.press(forDuration: 0.05, thenDragTo: press.withOffset(CGVector(dx: -60, dy: 30)), withVelocity: .slow, thenHoldForDuration: 0.05)
        XCTAssertLessThan(panel.frame.minX, original.minX - 30)
        attachEditor(in: app, name: "pen-tilt-\(theme)")
        workspaceActivate(app.buttons["pen-tilt-cancel"])
        XCTAssertTrue(panel.waitForNonExistence(timeout: 5))
        settings(); open(); pointCount(2)
        XCTAssertEqual(values(), initial, "Cancel must discard the unbound axis draft")
        workspaceActivate(app.buttons["pen-tilt-close"])
        XCTAssertTrue(panel.waitForNonExistence(timeout: 5))
        settings()
        reveal("pen_pressure", label: "Pen pressure")
        let pressure = app.descendants(matching: .any)["pen-pressure-dialog"].firstMatch
        XCTAssertTrue(pressure.waitForExistence(timeout: 10)); XCTAssertFalse(panel.exists)
        workspaceActivate(app.buttons["pen-pressure-close"])
        settings(); open(); pointCount(2)
        XCTAssertFalse(pressure.exists); XCTAssertEqual(values(), initial)
        workspaceActivate(app.buttons["pen-tilt-close"])
    }

    @MainActor func checkPressureCalibration(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"reveal","id":"pen_pressure"}}]"#
        app.launch()
        let panel = app.descendants(matching: .any)["pen-pressure-dialog"].firstMatch
        func open() {
            let adjust = app.buttons["preference-pen_pressure"]
            XCTAssertTrue(adjust.waitForExistence(timeout: 20)); workspaceActivate(adjust)
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 10))
            XCTAssertTrue(panel.waitForExistence(timeout: 10))
        }
        func reopen() {
            workspaceActivate(app.buttons["settings-button"])
            let input = app.staticTexts["settings-page-input"]
            XCTAssertTrue(input.waitForExistence(timeout: 10)); workspaceActivate(input)
            let adjust = app.buttons["preference-pen_pressure"]
            if !adjust.exists || !adjust.isHittable {
                let search = app.textFields["settings-search"]
                workspaceActivate(search)
                search.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: (search.value as? String ?? "").count))
                search.typeText("Pen pressure")
                workspaceActivate(app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Pen pressure")).firstMatch)
            }
            open()
        }
        open()
        let graph = panel.descendants(matching: .any)["effect-curve"].firstMatch
        func count(_ n: Int) {
            let expected = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", String(n)), object: graph)
            let result = XCTWaiter.wait(for: [expected], timeout: 5)
            let value = graph.exists ? String(describing: graph.value) : "missing"
            XCTAssertEqual(result, .completed, "Expected \(n) pressure points, value=\(value); \(panel.debugDescription)")
        }
        count(3)
        XCTAssertFalse(panel.textFields.firstMatch.exists)
        let original = panel.frame
        let title = panel.staticTexts["Pen pressure"].firstMatch
        let press = title.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        press.press(forDuration: 0.05, thenDragTo: press.withOffset(CGVector(dx: -100, dy: 30)), withVelocity: .slow, thenHoldForDuration: 0.05)
        XCTAssertLessThan(panel.frame.minX, original.minX - 50)
        let point = graph.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5))
        point.tap(); count(4)
        point.press(forDuration: 0.05, thenDragTo: graph.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: -0.3)), withVelocity: .slow, thenHoldForDuration: 0.05)
        count(3)
        workspaceActivate(app.buttons["pen-pressure-reset"]); count(3)
        workspaceActivate(app.buttons["pen-pressure-firmer"])
        workspaceActivate(app.buttons["pen-pressure-lighter"])
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas); waitForExpectations(timeout: 30)
        #if os(macOS)
        let before = editorPixels(in: app)
        let stroke = app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.6))
        stroke.press(forDuration: 0.05, thenDragTo: stroke.withOffset(CGVector(dx: 80, dy: 0)), withVelocity: .slow, thenHoldForDuration: 0.05)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) != before }, evaluatedWith: canvas)
        waitForExpectations(timeout: 15)
        #endif
        attachEditor(in: app, name: "pen-pressure-\(theme)")
        for close in ["cancel", "close", "apply"] {
            workspaceActivate(app.buttons["pen-pressure-" + close]); XCTAssertTrue(panel.waitForNonExistence(timeout: 5))
            reopen(); count(3)
        }
        workspaceActivate(app.buttons["pen-pressure-close"])
        XCTAssertTrue(panel.waitForNonExistence(timeout: 5))
    }
}
