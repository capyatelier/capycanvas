import XCTest
import CoreGraphics

extension XCTestCase {
    @MainActor func checkLassoControls(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        editorTool("Lasso selection", in: app)
        editorChoice("Lasso", group: true, in: app)
        workspaceActivate(app.buttons["layer-Layer actions"])
        workspaceActivate(app.buttons["menu-action-Selection"])
        attachEditor(in: app, name: "layer-selection-menu")
        workspaceActivate(app.buttons["menu-action-Lasso Fill"])
        editorChoice("Lasso fill", group: true, in: app)
        XCTAssertFalse(app.buttons["number-value-tool-opacity"].exists)
        attachEditor(in: app, name: "lasso-fill-controls")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkHandAndEyedropper(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        #if os(macOS)
        app.launchEnvironment["CAPY_COLOR_PROBE"] = "1"
        #endif
        app.launch(); capturePaintEditor(in: app)
        editorTool("Hand", in: app)
        let hand = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "tool-group-", "Hand")).firstMatch
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: hand)
        waitForExpectations(timeout: 5)
        let viewport = workspaceViewport(in: app)
        let originalFrame = viewport.frame
        let point = CGPoint(x: 0.32, y: 0.55)
        func pixels() -> Data { editorPixels(in: app, at: point) }
        let paper = pixels()
        XCTAssertTrue(paper.prefix(3).allSatisfy { $0 == 255 }, "The initial sample must lie on white paper")
        let start = viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.44, dy: 0.55))
        let end = viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.60, dy: 0.55))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        app.staticTexts["document-title"].hover()
        #else
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.2)
        #endif
        attachEditor(in: app, name: "hand-after-drag")
        XCTAssertEqual(viewport.frame, originalFrame, "Hand must move the canvas within the same OS window")
        expectation(for: NSPredicate { _, _ in pixels() != paper }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "hand-panned")
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        expectation(for: NSPredicate { _, _ in pixels() == paper }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let undo = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-commands-", "Undo")).firstMatch
        XCTAssertFalse(undo.isEnabled, "Panning and Fit must not create artwork history")
        let flip = app.buttons["navigator-flip_horizontal"]
        for selected in [true, false] {
            workspaceActivate(flip)
            expectation(for: NSPredicate(format: "selected == %@", NSNumber(value: selected)), evaluatedWith: flip)
            waitForExpectations(timeout: 5)
        }
        #if os(macOS)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        workspaceActivate(app.buttons["layer-New layer"])
        workspaceActivate(app.buttons["color-swap"])
        editorTool("Figure", in: app); editorChoice("Rectangle", group: true, in: app); editorChoice("Fill", in: app)
        viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.44, dy: 0.42)).click(forDuration: 0.05,
            thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.62, dy: 0.68)))
        workspaceActivate(app.buttons["number-value-layer-opacity"])
        app.textFields["number-entry-layer-opacity"].typeText("50\n")
        expectation(for: NSPredicate(format: "value == %@", "50"), evaluatedWith: app.buttons["number-value-layer-opacity"])
        waitForExpectations(timeout: 5)
        func color() -> [Double] {
            let wheel = app.descendants(matching: .any)["color-wheel"].firstMatch
            guard let text = wheel.value as? String,
                let state = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { return [] }
            return state["rgba"] as? [Double] ?? []
        }
        func pick(_ x: Double, expected: [Double]) {
            viewport.coordinate(withNormalizedOffset: CGVector(dx: x, dy: 0.55)).click()
            app.staticTexts["document-title"].hover()
            expectation(for: NSPredicate { _, _ in
                let actual = color()
                return actual.count == 4 && zip(actual, expected).allSatisfy { abs($0.0 - $0.1) < 0.01 }
            }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        #endif
        editorTool("Eyedropper", in: app)
        editorChoice("Visible color", in: app)
        #if os(macOS)
        // Independently calculated sRGB result of 50% red over blue in linear light.
        pick(0.53, expected: [0.672824, 0.366774, 0.599931, 1])
        attachEditor(in: app, name: "eyedropper-visible")
        #endif
        editorChoice("Layer color", in: app)
        #if os(macOS)
        pick(0.53, expected: [0.9, 0.25, 0.2, 1])
        pick(0.66, expected: [0.9, 0.25, 0.2, 1]) // Transparent layer pixels preserve the current color.
        attachEditor(in: app, name: "eyedropper-layer")
        editorChoice("Visible color", in: app)
        pick(0.66, expected: [0.2, 0.45, 0.8, 1])
        #endif
        attachEditor(in: app, name: "eyedropper-controls")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
