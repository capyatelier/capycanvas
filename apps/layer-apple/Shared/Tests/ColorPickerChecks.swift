import XCTest

extension XCTestCase {
    @MainActor func checkColorPicker(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launchEnvironment["CAPY_COLOR_PROBE"] = "1"
        app.launch(); capturePaintEditor(in: app)
        func expectSelected(_ element: XCUIElement, _ selected: Bool) {
            expectation(for: NSPredicate(format: "selected == %@", NSNumber(value: selected)), evaluatedWith: element)
            waitForExpectations(timeout: 10)
        }
        let paint = app.buttons["workspace-switch-builtin:workspace:illustrator"]
        let eyedropper = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-toolbar-", "Eyedropper")).firstMatch
        func color() -> [Double] {
            let wheel = app.descendants(matching: .any)["color-wheel"].firstMatch
            guard let text = wheel.value as? String,
                let state = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { return [] }
            return state["rgba"] as? [Double] ?? []
        }
        func expectColor(_ expected: [Double]) {
            expectation(for: NSPredicate { _, _ in
                let actual = color()
                return actual.count == 4 && zip(actual, expected).allSatisfy { abs($0.0 - $0.1) < 0.01 }
            }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        let canvas = workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        XCTAssertTrue(eyedropper.waitForExistence(timeout: 10))
        expectColor([0.2, 0.45, 0.8, 1])
        workspaceActivate(eyedropper)
        expectSelected(eyedropper, true)
        #if os(macOS)
        canvas.hover()
        attachEditor(in: app, name: "picker-loupe")
        expectColor([0.2, 0.45, 0.8, 1])
        app.descendants(matching: .any)["color-panel-controls"].firstMatch.hover()
        XCTAssertTrue(eyedropper.isSelected, "Leaving the canvas keeps picking")
        canvas.click()
        expectSelected(eyedropper, false)
        expectColor([1, 1, 1, 1])
        #else
        canvas.tap()
        expectSelected(eyedropper, false)
        expectColor([0.2, 0.45, 0.8, 1])
        canvas.press(forDuration: 1)
        expectSelected(eyedropper, false)
        expectColor([1, 1, 1, 1])
        #endif
        workspaceActivate(app.buttons["color-quick-black"])
        expectColor([0, 0, 0, 1])
        attachEditor(in: app, name: "picker-neutral-shortcuts")

        let sketch = app.buttons["workspace-switch-builtin:workspace:painter"]
        workspaceActivate(sketch)
        expectSelected(sketch, true)
        let picker = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", "Color Picker")).firstMatch
        XCTAssertTrue(picker.waitForExistence(timeout: 10), "Sketch's brush toolbar carries the picker")
        let toolbar = String(picker.identifier.prefix(upTo: picker.identifier.lastIndex(of: "-")!)) + "-"
        func tile(_ label: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", toolbar, label)).firstMatch
        }
        let sliders = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "component-slider-"))
        let order = [sliders.element(boundBy: 0), picker, sliders.element(boundBy: 1), tile("Undo"), tile("Redo")].map(\.frame.midY)
        XCTAssertEqual(order, order.sorted(), "Size, picker, opacity, Undo, Redo")
        #if os(macOS)
        picker.doubleClick()
        let size = app.popUpButtons["picker-setting-size"]
        #else
        picker.doubleTap()
        let size = app.buttons["picker-setting-size"]
        #endif
        XCTAssertTrue(app.descendants(matching: .any)["tool-drawer"].firstMatch.waitForExistence(timeout: 10), "A double press opens picker settings")
        expectSelected(picker, true)
        XCTAssertTrue(size.waitForExistence(timeout: 10))
        workspaceActivate(size)
        let circles = ["Single pixel", "5 px circle", "15 px circle", "51 px circle", "101 px circle"]
        #if os(macOS)
        for label in circles { XCTAssertTrue(app.menuItems[label].firstMatch.waitForExistence(timeout: 5), label) }
        workspaceActivate(app.menuItems["15 px circle"].firstMatch)
        #else
        for label in circles { XCTAssertTrue(app.buttons[label].firstMatch.waitForExistence(timeout: 5), label) }
        workspaceActivate(app.buttons["15 px circle"].firstMatch)
        #endif
        attachEditor(in: app, name: "picker-settings")
        workspaceActivate(picker)
        expectSelected(picker, false)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
