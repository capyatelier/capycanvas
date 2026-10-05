import XCTest

extension XCTestCase {
    @MainActor func checkLayerColorModesAndFilters(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]},{"type":"invoke","command":"select_all"},{"type":"invoke","command":"fill_selection"},{"type":"invoke","command":"deselect"}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let mode = app.buttons["layer-color-mode"]
        func pixels(_ accept: @escaping (Int, Int, Int) -> Bool, _ message: String) {
            let match = NSPredicate { _, _ in
                let data = self.editorPixels(in: app)
                return stride(from: 0, to: data.count, by: 4).allSatisfy { accept(Int(data[$0]), Int(data[$0 + 1]), Int(data[$0 + 2])) }
            }
            if XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: match, object: app)], timeout: 15) != .completed {
                attachEditor(in: app, name: "layer-color-mode-unexpected-\(theme)"); XCTFail(message)
            }
        }
        func thumbnail(_ accept: @escaping (Int, Int, Int) -> Bool, _ message: String) {
            #if os(macOS)
            let reference = app.windows.firstMatch
            #else
            let reference = canvas
            #endif
            let content = rows.element(boundBy: 0).buttons.matching(NSPredicate(format: "identifier ENDSWITH %@", "-content")).firstMatch
            let match = NSPredicate { _, _ in
                let f = content.frame, r = reference.frame
                let data = self.editorPixelSamples(in: app, at: [CGPoint(x: (f.midX - r.minX) / r.width, y: (f.midY - r.minY) / r.height)], size: 4)[0]
                return accept(Int(data[0]), Int(data[1]), Int(data[2]))
            }
            if XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: match, object: app)], timeout: 15) != .completed {
                attachEditor(in: app, name: "layer-color-mode-thumbnail-\(theme)"); XCTFail(message)
            }
        }
        func choose(_ label: String) {
            workspaceActivate(mode)
            let item = app.buttons["menu-action-" + label]
            XCTAssertTrue(item.waitForExistence(timeout: 10), "Color mode offers \(label)")
            workspaceActivate(item)
            expectation(for: NSPredicate(format: "value == %@", label), evaluatedWith: mode)
            waitForExpectations(timeout: 10)
        }
        XCTAssertTrue(mode.waitForExistence(timeout: 30), "Layers offers the layer's color mode")
        pixels({ r, _, b in b > r + 50 }, "The filled layer starts blue")
        choose("Grayscale")
        pixels({ r, g, b in abs(r - g) <= 2 && abs(g - b) <= 2 && r > 20 && r < 235 }, "Grayscale keeps gray")
        thumbnail({ r, g, b in abs(r - g) <= 3 && abs(g - b) <= 3 }, "The layer thumbnail follows Grayscale")
        attachEditor(in: app, name: "layer-color-mode-grayscale-\(theme)")
        choose("Two-tone (black & white)")
        pixels({ r, g, b in [r, g, b].allSatisfy { $0 <= 2 || $0 >= 253 } && r == g && g == b }, "Two-tone keeps black or white")
        choose("Full color")
        for label in ["Two-tone (black & white)", "Grayscale", "Full color"] {
            editorHistory("Undo", in: app)
            expectation(for: NSPredicate(format: "value == %@", label), evaluatedWith: mode)
            waitForExpectations(timeout: 10)
        }
        pixels({ r, _, b in b > r + 50 }, "Undo restores the layer's colors")

        let count = rows.count
        workspaceActivate(app.buttons["layer-add-filter"])
        let menu = app.descendants(matching: .any)["layer-filter-menu"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 10), "The Layers footer opens the filter menu")
        workspaceActivate(menu.buttons["menu-action-Tone"])
        let exposure = menu.buttons["menu-action-Exposure"]
        revealEditorControl(exposure, in: menu); workspaceActivate(exposure)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        expectation(for: NSPredicate { _, _ in rows.count == count + 1 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.buttons["number-value-property-exposure"].waitForExistence(timeout: 10), "Inserting a filter reveals its Properties")
        attachEditor(in: app, name: "layer-add-filter-\(theme)")
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in rows.count == count }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
