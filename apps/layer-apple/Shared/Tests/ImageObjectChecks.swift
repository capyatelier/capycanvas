import XCTest

extension XCTestCase {
    @MainActor func checkImageObjects(in app: XCUIApplication, theme: String) throws {
        var actions: [[String: Any]] = [
            ["type": "set_theme", "theme": theme],
            ["type": "set_color", "rgba": [0.2, 0.45, 0.8, 1]],
            ["type": "invoke", "command": "select_all"],
            ["type": "invoke", "command": "fill_selection"],
            ["type": "invoke", "command": "deselect"],
            ["type": "customize", "action": ["type": "insert_tools", "panel": "commands", "before": NSNull()]]
        ]
        for command in ["convert_to_object", "rasterize_layer", "clear_layer", "fit_canvas"] {
            actions.append(["type": "customize", "action": ["type": "picker_select",
                "control": ["kind": "command", "command": command], "selected": true]])
        }
        actions.append(["type": "customize", "action": ["type": "confirm_tools"]])
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        func command(_ label: String) {
            let button = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-commands-", label)).firstMatch
            XCTAssertTrue(button.waitForExistence(timeout: 10))
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: button)
            waitForExpectations(timeout: 15)
            workspaceActivate(button)
        }
        command("Fit canvas")
        let layers = app.buttons["panel-tab-layers"]
        XCTAssertTrue(layers.waitForExistence(timeout: 10))
        if !layers.isSelected { workspaceActivate(layers) }
        #if os(macOS)
        let candidates = app.groups
        #else
        let candidates = app.otherElements
        #endif
        let rows = candidates.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let initialCount = rows.count
        func count(_ expected: Int) {
            expectation(for: NSPredicate(format: "count == %d", expected), evaluatedWith: rows)
            waitForExpectations(timeout: 20)
        }
        func layerMenu(_ row: XCUIElement, _ label: String) {
            #if os(macOS)
            row.rightClick()
            #else
            row.press(forDuration: 0.6)
            #endif
            let menu = app.descendants(matching: .any)["layer-context-menu"].firstMatch
            XCTAssertTrue(menu.waitForExistence(timeout: 5))
            if label == "Duplicate" { workspaceActivate(menu.buttons["menu-action-Organize"]) }
            let item = menu.buttons["menu-action-" + label]
            revealEditorControl(item, in: menu)
            XCTAssertTrue(item.isEnabled, "The selected image offers \(label)")
            workspaceActivate(item)
            XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
        }
        let blue = editorPixels(in: app)
        command("Convert to Object Layer")
        count(initialCount); expectPixels(blue, in: app)
        let originalID = rows.firstMatch.identifier
        let original = candidates[originalID].firstMatch
        workspaceActivate(original.buttons["Edit layer content"])
        layerMenu(original, "Duplicate")
        count(initialCount + 1); expectPixels(blue, in: app)
        editorHistory("Undo", in: app); count(initialCount)
        editorHistory("Redo", in: app); count(initialCount + 1)
        workspaceActivate(original.buttons["Edit layer content"])
        workspaceActivate(original.buttons["layer-Hide layer"])
        XCTAssertTrue(original.buttons["layer-Show layer"].waitForExistence(timeout: 5))
        editorHistory("Undo", in: app)
        XCTAssertTrue(original.buttons["layer-Hide layer"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-images-")).firstMatch.exists)
        attachEditor(in: app, name: "object-layers-\(theme)")

        command("Clear Entire Layer")
        let notice = app.descendants(matching: .any)["canvas-notice"].firstMatch
        XCTAssertTrue(notice.waitForExistence(timeout: 5))
        for token in ["add_mask", "new_paint_layer", "rasterize_layer"] {
            let action = app.buttons["canvas-notice-action-" + token]
            XCTAssertTrue(action.exists && action.isEnabled && action.isHittable)
        }
        command("Clear Entire Layer")
        app.buttons["canvas-notice-action-rasterize_layer"].clickOrTap()
        XCTAssertTrue(notice.waitForNonExistence(timeout: 5)); count(initialCount + 1)
        expectPixels(blue, in: app)
        editorHistory("Undo", in: app); count(initialCount + 1); expectPixels(blue, in: app)
        editorHistory("Redo", in: app); count(initialCount + 1); expectPixels(blue, in: app)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "image-rasterize-\(theme)")
    }
}
