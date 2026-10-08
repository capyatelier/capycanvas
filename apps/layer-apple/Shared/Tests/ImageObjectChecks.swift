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
        let objects = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "image-object-"))
            .matching(NSPredicate(format: "NOT identifier BEGINSWITH %@", "image-object-grip-"))
        func count(_ expected: Int) {
            expectation(for: NSPredicate(format: "count == %d", expected), evaluatedWith: objects)
            waitForExpectations(timeout: 20)
        }
        func objectMenu(_ row: XCUIElement, _ label: String) {
            #if os(macOS)
            row.rightClick()
            #else
            row.press(forDuration: 0.6)
            #endif
            let menu = app.descendants(matching: .any)["layer-context-menu"].firstMatch
            XCTAssertTrue(menu.waitForExistence(timeout: 5))
            let item = menu.buttons["menu-action-" + label]
            revealEditorControl(item, in: menu)
            XCTAssertTrue(item.isEnabled, "The selected image offers \(label)")
            workspaceActivate(item)
            XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
        }
        let blue = editorPixels(in: app)
        command("Convert to Image Layer")
        let expand = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-images-")).firstMatch
        XCTAssertTrue(expand.waitForExistence(timeout: 20))
        if expand.label == "Expand image list" { workspaceActivate(expand) }
        count(1); expectPixels(blue, in: app)
        let originalID = objects.firstMatch.identifier
        workspaceActivate(objects.firstMatch)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: objects.firstMatch)
        waitForExpectations(timeout: 5)
        objectMenu(objects.firstMatch, "Duplicate Images")
        count(2); expectPixels(blue, in: app)
        editorHistory("Undo", in: app); count(1)
        editorHistory("Redo", in: app); count(2)
        let original = app.descendants(matching: .any)[originalID].firstMatch
        workspaceActivate(original)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: original)
        waitForExpectations(timeout: 5)
        objectMenu(original, "Bring to Front")
        expectation(for: NSPredicate { _, _ in objects.firstMatch.identifier == originalID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in objects.firstMatch.identifier != originalID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        objectMenu(original, "Hide Image")
        XCTAssertTrue(original.buttons["layer-Show Image"].waitForExistence(timeout: 5))
        editorHistory("Undo", in: app)
        XCTAssertTrue(original.buttons["layer-Hide Image"].waitForExistence(timeout: 5))
        workspaceActivate(expand); count(0)
        workspaceActivate(expand); count(2)
        attachEditor(in: app, name: "image-rows-\(theme)")

        command("Clear Entire Layer")
        let notice = app.descendants(matching: .any)["canvas-notice"].firstMatch
        XCTAssertTrue(notice.waitForExistence(timeout: 5))
        for token in ["add_mask", "new_paint_layer", "rasterize_layer"] {
            let action = app.buttons["canvas-notice-action-" + token]
            XCTAssertTrue(action.exists && action.isEnabled && action.isHittable)
        }
        command("Clear Entire Layer")
        app.buttons["canvas-notice-action-rasterize_layer"].clickOrTap()
        XCTAssertTrue(notice.waitForNonExistence(timeout: 5)); count(0)
        expectPixels(blue, in: app)
        editorHistory("Undo", in: app); count(2); expectPixels(blue, in: app)
        editorHistory("Redo", in: app); count(0); expectPixels(blue, in: app)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "image-rasterize-\(theme)")
    }
}
