import XCTest

extension XCTestCase {
    @MainActor func checkLayerColorModesAndFilters(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]},{"type":"invoke","command":"select_all"},{"type":"invoke","command":"fill_selection"},{"type":"invoke","command":"deselect"},{"type":"customize","action":{"type":"set_panel_visible","panel":"layers","visible":true}},{"type":"customize","action":{"type":"set_panel_visible","panel":"properties","visible":true}}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let mode = app.buttons["property-color_mode"]
        func show(_ panel: String) {
            let tab = app.buttons["panel-tab-" + panel]
            XCTAssertTrue(tab.waitForExistence(timeout: 10))
            if !tab.isSelected { workspaceActivate(tab) }
        }
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
            show("layers")
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
        func choose(_ index: Int, _ label: String) {
            show("properties")
            revealEditorControl(mode, in: app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch)
            workspaceActivate(mode)
            let item = app.buttons["property-color_mode-option-\(index)"]
            XCTAssertTrue(item.waitForExistence(timeout: 10), "Color mode offers \(label)")
            workspaceActivate(item)
            expectation(for: NSPredicate(format: "value == %@", label), evaluatedWith: mode)
            waitForExpectations(timeout: 10)
        }
        show("properties")
        XCTAssertFalse(app.buttons["layer-color-mode"].exists)
        XCTAssertTrue(mode.waitForExistence(timeout: 30), "Properties offers the layer's color mode")
        pixels({ r, _, b in b > r + 50 }, "The filled layer starts blue")
        choose(1, "Grayscale")
        pixels({ r, g, b in abs(r - g) <= 2 && abs(g - b) <= 2 && r > 20 && r < 235 }, "Grayscale keeps gray")
        thumbnail({ r, g, b in abs(r - g) <= 3 && abs(g - b) <= 3 }, "The layer thumbnail follows Grayscale")
        attachEditor(in: app, name: "layer-color-mode-grayscale-\(theme)")
        choose(2, "Two-tone (black & white)")
        pixels({ r, g, b in [r, g, b].allSatisfy { $0 <= 2 || $0 >= 253 } && r == g && g == b }, "Two-tone keeps black or white")
        choose(0, "Full color")
        for label in ["Two-tone (black & white)", "Grayscale", "Full color"] {
            editorHistory("Undo", in: app)
            expectation(for: NSPredicate(format: "value == %@", label), evaluatedWith: mode)
            waitForExpectations(timeout: 10)
        }
        pixels({ r, _, b in b > r + 50 }, "Undo restores the layer's colors")

        show("layers")
        let count = rows.count, owner = rows.element(boundBy: 0).identifier
        for (index, filter) in ["Exposure", "Curves", "Levels"].enumerated() {
            show(index == 0 ? "properties" : "layers")
            let menuID: String
            if index < 2 {
                let button = app.buttons[index == 0 ? "properties-add-filter" : "layer-add-filter"]
                if index == 0 { revealEditorControl(button, in: app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch) }
                workspaceActivate(button)
                menuID = index == 0 ? "properties-filter-menu" : "layer-filter-menu"
            } else {
                let row = app.descendants(matching: .any)[owner].firstMatch
                revealEditorControl(row, in: app.scrollViews.containing(.any, identifier: "layer-rows").firstMatch)
                #if os(macOS)
                row.rightClick()
                #else
                row.press(forDuration: 0.6)
                #endif
                menuID = "layer-context-menu"
            }
            let menu = app.descendants(matching: .any)[menuID].firstMatch
            XCTAssertTrue(menu.waitForExistence(timeout: 10))
            if index == 2 { workspaceActivate(menu.buttons["menu-action-Add Filter"]) }
            workspaceActivate(menu.buttons["menu-action-Tone"])
            let item = menu.buttons["menu-action-" + filter]
            revealEditorControl(item, in: menu); workspaceActivate(item)
            XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
            show("layers")
            expectation(for: NSPredicate { _, _ in rows.count == count + index + 1 }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        attachEditor(in: app, name: "layer-add-filter-\(theme)")
        for _ in 0..<3 { editorHistory("Undo", in: app) }
        expectation(for: NSPredicate { _, _ in rows.count == count }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
