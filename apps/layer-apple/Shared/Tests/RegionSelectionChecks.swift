import XCTest
import CoreGraphics

extension XCTestCase {
    @MainActor func checkRegionSelectionAndFill(in app: XCUIApplication) {
        // Only colors/theme are seeded. Artwork, references and selections use native UI.
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        #if os(macOS)
        let viewport = workspaceViewport(in: app)
        let center = CGPoint(x: 0.48, y: 0.55)
        let points = [center, CGPoint(x: 0.58, y: 0.55), CGPoint(x: 0.66, y: 0.55)]
        func coordinate(_ point: CGPoint) -> XCUICoordinate {
            viewport.coordinate(withNormalizedOffset: CGVector(dx: point.x, dy: point.y))
        }
        func samples() -> [Data] { editorPixelSamples(in: app, at: points, size: 4) }
        func waitPixels(_ expected: [Data]) {
            expectation(for: NSPredicate { _, _ in samples() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func red(_ data: Data) -> Bool { Int(data[0]) > Int(data[2]) + 50 }
        // A separate outline layer makes the three region sources distinguishable.
        editorTool("Figure", in: app)
        editorChoice("Rectangle", group: true, in: app); editorChoice("Outline", in: app)
        coordinate(CGPoint(x: 0.44, y: 0.42)).click(forDuration: 0.05,
            thenDragTo: coordinate(CGPoint(x: 0.62, y: 0.68)))
        app.staticTexts["document-title"].hover()
        workspaceActivate(app.buttons["layer-Use selected layers as references"])
        XCTAssertTrue(app.buttons["layer-Stop using this layer as a reference"].isSelected)
        workspaceActivate(app.buttons["layer-New layer"])
        // Visible artwork includes this unmarked divider; reference-only sampling ignores it.
        editorChoice("Line", group: true, in: app)
        coordinate(CGPoint(x: 0.53, y: 0.42)).click(forDuration: 0.05,
            thenDragTo: coordinate(CGPoint(x: 0.53, y: 0.68)))
        app.staticTexts["document-title"].hover()
        workspaceActivate(app.buttons["layer-New layer"])
        workspaceActivate(app.buttons["color-swap"])
        let blank = samples()
        func fillResult(_ source: String, inverted: Bool = false, name: String) {
            expectation(for: NSPredicate { _, _ in red(samples()[inverted ? 1 : 0]) }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
            let painted = samples()
            let coverage = source == "Editing layer" ? [true, true, true]
                : source == "Reference layers" ? [true, true, false] : [true, false, false]
            for index in points.indices {
                if coverage[index] != inverted { XCTAssertTrue(red(painted[index]), "\(name) must paint sample \(index)") }
                else { XCTAssertEqual(painted[index], blank[index], "\(name) must preserve sample \(index)") }
            }
            attachEditor(in: app, name: name)
            for (command, pixels) in [("Undo", blank), ("Redo", painted), ("Undo", blank)] {
                editorHistory(command, in: app); waitPixels(pixels)
            }
        }
        #endif
        for tool in ["Fill", "Auto select"] {
            editorTool(tool, in: app)
            for source in ["Visible artwork", "Editing layer", "Reference layers"] {
                editorChoice(source, in: app)
                for setting in ["tolerance", "smoothing"] {
                    XCTAssertTrue(app.buttons["number-value-tool-" + setting].exists)
                }
                for setting in ["gap_closing", "expansion"] {
                    XCTAssertTrue(app.textFields["number-entry-tool-" + setting].exists)
                }
                XCTAssertEqual(app.buttons["number-value-tool-opacity"].exists, tool == "Fill")
                #if os(macOS)
                coordinate(center).click(); app.staticTexts["document-title"].hover()
                if tool == "Auto select" {
                    let select = app.menuBars.menuBarItems["Select"]
                    workspaceActivate(select)
                    expectation(for: NSPredicate(format: "enabled == YES"),
                        evaluatedWith: select.menuItems["Invert selection"])
                    waitForExpectations(timeout: 10)
                    app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
                    editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
                }
                fillResult(source, name: "region-\(tool)-\(source)")
                if tool == "Auto select" {
                    if source == "Visible artwork" {
                        editorMenu(in: app, menu: "Select", id: "invert_selection", label: "Invert selection")
                        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
                        fillResult(source, inverted: true, name: "region-inverted")
                    }
                    editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
                }
                #endif
            }
            // Exercise the real numeric editor, including restoration of the original value.
            for percent in [20, 10] {
                workspaceActivate(app.buttons["number-value-tool-tolerance"])
                let entry = app.textFields["number-entry-tool-tolerance"]
                XCTAssertTrue(entry.waitForExistence(timeout: 5))
                entry.typeText("\(percent)\n")
                expectation(for: NSPredicate(format: "value == %@", "\(percent).0 %"),
                    evaluatedWith: app.buttons["number-value-tool-tolerance"])
                waitForExpectations(timeout: 5)
            }
            attachEditor(in: app, name: "region-\(tool)-controls")
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkSelectionInversion(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        func pixels() -> Data { editorPixels(in: app) }
        func expectPixels(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func invert() { editorMenu(in: app, menu: "Select", id: "invert_selection", label: "Invert selection") }
        func fill() { editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection") }
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        fill()
        expectation(for: NSPredicate { _, _ in let p = pixels(); return Int(p[2]) > Int(p[0]) + 50 }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let blue = pixels()
        workspaceActivate(app.buttons["color-swap"])
        invert(); fill(); expectPixels(blue)
        attachEditor(in: app, name: "selection-inverted-empty")
        invert(); fill()
        expectation(for: NSPredicate { _, _ in let p = pixels(); return Int(p[0]) > Int(p[2]) + 50 }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let red = pixels()
        attachEditor(in: app, name: "selection-inverted-full")
        editorHistory("Undo", in: app); expectPixels(blue)
        editorHistory("Redo", in: app); expectPixels(red)
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
