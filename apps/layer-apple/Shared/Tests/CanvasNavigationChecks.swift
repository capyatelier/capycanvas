import XCTest
import CoreGraphics

extension XCTestCase {
    @MainActor func checkLassoControls(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        #if os(macOS)
        func clickWithoutArea() {
            let layers = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            let count = layers.count
            workspaceActivate(app.buttons["layer-New layer"])
            expectation(for: NSPredicate(format: "count == %d", count + 1), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
            editorHistory("Undo", in: app)
            expectation(for: NSPredicate(format: "count == %d", count), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
            workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.55)).click()
            editorDocumentTitle(in: app).hover()
            editorHistory("Redo", in: app)
            expectation(for: NSPredicate(format: "count == %d", count + 1), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
            XCTAssertFalse(app.staticTexts["Canvas error"].exists, "A lasso click must not report a canvas error")
            editorHistory("Undo", in: app)
            expectation(for: NSPredicate(format: "count == %d", count), evaluatedWith: layers)
            waitForExpectations(timeout: 10)
        }
        #endif
        editorTool("Lasso selection", in: app)
        editorChoice("Select", group: true, in: app)
        editorChoice("Lasso selection", in: app)
        #if os(macOS)
        clickWithoutArea()
        #endif
        workspaceActivate(app.buttons["layer-Layer actions"])
        workspaceActivate(app.buttons["menu-action-Pixel Selection"])
        for label in ["Fill Selection", "Invert Selection", "Deselect Pixels"] {
            XCTAssertTrue(app.buttons["menu-action-" + label].waitForExistence(timeout: 5), label)
        }
        attachEditor(in: app, name: "layer-selection-menu")
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
        let sheet = editorPaper(in: app)
        let point = sheet.point(0.06, 0.5)
        func pixels() -> Data { editorPixels(in: app, at: point) }
        let paper = pixels()
        XCTAssertTrue(paper.prefix(3).allSatisfy { $0 == 255 }, "The initial sample must lie on white paper")
        let start = viewport.coordinate(withNormalizedOffset: sheet.offset(0.3, 0.5))
        let end = viewport.coordinate(withNormalizedOffset: sheet.offset(0.7, 0.5))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        editorDocumentTitle(in: app).hover()
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
        viewport.coordinate(withNormalizedOffset: sheet.offset(0.25, 0.2)).click(forDuration: 0.05,
            thenDragTo: viewport.coordinate(withNormalizedOffset: sheet.offset(0.6, 0.8)))
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
        func pick(_ x: CGFloat, expected: [Double]) {
            viewport.coordinate(withNormalizedOffset: sheet.offset(x, 0.5)).click()
            editorDocumentTitle(in: app).hover()
            expectation(for: NSPredicate { _, _ in
                let actual = color()
                return actual.count == 4 && zip(actual, expected).allSatisfy { abs($0.0 - $0.1) < 0.01 }
            }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        func source(_ label: String) {
            editorTool("Eyedropper", in: app)
            let menu = app.popUpButtons["picker-setting-source"]
            XCTAssertTrue(menu.waitForExistence(timeout: 10), "Picking shows its source setting")
            workspaceActivate(menu)
            workspaceActivate(app.menuItems[label].firstMatch)
            expectation(for: NSPredicate(format: "value == %@", label), evaluatedWith: menu)
            waitForExpectations(timeout: 5)
        }
        // Independently calculated sRGB result of 50% red over blue in linear light.
        source("Visible color")
        pick(0.45, expected: [0.672824, 0.366774, 0.599931, 1])
        attachEditor(in: app, name: "eyedropper-visible")
        source("Selected layer")
        pick(0.45, expected: [0.9, 0.25, 0.2, 1])
        editorTool("Eyedropper", in: app)
        pick(0.8, expected: [0.9, 0.25, 0.2, 1]) // Transparent layer pixels preserve the current color.
        attachEditor(in: app, name: "eyedropper-layer")
        source("Visible color")
        pick(0.8, expected: [0.2, 0.45, 0.8, 1])
        #else
        editorTool("Eyedropper", in: app)
        XCTAssertTrue(app.buttons["picker-setting-source"].waitForExistence(timeout: 10), "Picking shows its source setting")
        #endif
        attachEditor(in: app, name: "eyedropper-controls")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
