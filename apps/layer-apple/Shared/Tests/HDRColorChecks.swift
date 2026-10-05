import XCTest

extension XCTestCase {
    @MainActor func checkNativeHDRColor(in app: XCUIApplication) throws {
        var actions: [[String: Any]] = [
            ["type": "set_theme", "theme": "dark"],
            ["type": "customize", "action": ["type": "insert_tools", "panel": "commands", "before": NSNull()]]
        ]
        for command in ["change_bit_depth", "sdr_rendition", "histogram", "export_document"] {
            actions.append(["type": "customize", "action": ["type": "picker_select", "control": ["kind": "command", "command": command], "selected": true]])
        }
        actions.append(["type": "customize", "action": ["type": "confirm_tools"]])
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch(); capturePaintEditor(in: app)
        func command(_ text: String) {
            let item = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-commands-", text)).firstMatch
            XCTAssertTrue(item.waitForExistence(timeout: 15))
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: item); waitForExpectations(timeout: 30)
            workspaceActivate(item)
        }
        func choose(_ id: String, _ text: String) {
            #if os(macOS)
            workspaceActivate(app.descendants(matching: .any)[id].firstMatch)
            workspaceActivate(app.menuItems[text].firstMatch)
            #else
            // FormPicker also exposes its visible UIKit label with this ID.
            // Open the menu button, not the preceding static text.
            workspaceActivate(app.buttons[id].firstMatch)
            workspaceActivate(app.buttons[text].firstMatch)
            #endif
        }
        command("Change Bit Depth…"); choose("document-color-depth", "32-bit float HDR")
        workspaceActivate(app.buttons["document-color-preview"])
        let apply = app.buttons["document-color-apply"]
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: apply); waitForExpectations(timeout: 60)
        workspaceActivate(apply); XCTAssertTrue(apply.waitForNonExistence(timeout: 60))
        let screen = app.buttons["screen-status"]
        XCTAssertTrue(screen.waitForExistence(timeout: 15), "An HDR drawing shows how the screen presents it")
        let presented = screen.label
        XCTAssertTrue(["HDR", "Showing SDR"].contains(presented), presented)
        workspaceActivate(screen)
        let headline = app.staticTexts["screen-details-headline"]
        XCTAssertTrue(headline.waitForExistence(timeout: 5))
        let shown = (headline.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? headline.label
        XCTAssertEqual(shown, presented == "HDR" ? "Showing HDR" : "Showing the SDR version")
        attachEditor(in: app, name: "hdr-screen-details")
        workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.12)).clickOrTap()
        XCTAssertTrue(headline.waitForNonExistence(timeout: 5))
        let edit = app.buttons["paint-edit-color"].firstMatch
        XCTAssertTrue(edit.waitForExistence(timeout: 15)); workspaceActivate(edit)
        let use = app.buttons["color-use"]
        XCTAssertTrue(use.waitForExistence(timeout: 15))
        editColorValue("ev", "2", in: app)
        XCTAssertTrue(app.descendants(matching: .any)["color-current"].firstMatch.exists && app.descendants(matching: .any)["color-new"].firstMatch.exists)
        attachEditor(in: app, name: "hdr-edit-color")
        workspaceActivate(use)
        XCTAssertTrue(use.waitForNonExistence(timeout: 15))
        workspaceActivate(edit)
        XCTAssertTrue(use.waitForExistence(timeout: 15))
        XCTAssertTrue(colorValueText("ev", in: app).contains("2.00"), "Use Color must commit the final EV character")
        workspaceActivate(app.buttons["color-cancel"])
        XCTAssertTrue(use.waitForNonExistence(timeout: 15))
        XCTAssertFalse(app.buttons["paint-palettes"].exists)
        command("Proof SDR")
        let dial = app.descendants(matching: .any)["proof-dial"].firstMatch
        XCTAssertTrue(dial.waitForExistence(timeout: 15))
        let start = dial.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let end = dial.coordinate(withNormalizedOffset: CGVector(dx: 0.65, dy: 0.35))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        #else
        start.press(forDuration: 0.05, thenDragTo: end)
        #endif
        attachEditor(in: app, name: "hdr-proof-dial")
        editorHistory("Undo", in: app); editorHistory("Redo", in: app)
        workspaceActivate(app.buttons["proof-reset"])
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        // Exercise the native print-mode control while the proof surface is retained.
        workspaceActivate(app.buttons["proof-mode-print"])
        XCTAssertTrue(app.descendants(matching: .any)["proof-profile"].firstMatch.waitForExistence(timeout: 15))
        attachEditor(in: app, name: "hdr-print-panel")
        workspaceActivate(app.buttons["proof-mode-off"])
        command("Export…")
        let range = app.descendants(matching: .any)["export-range"].firstMatch
        XCTAssertTrue(range.waitForExistence(timeout: 30))
        // Exercise delivery sizing too. Debug AV1 encoding of the full canvas
        // is deliberately outside this UI-control test's latency budget.
        workspaceActivate(app.descendants(matching: .any)["export-fit"].firstMatch)
        for (id, value) in [("export-width", "192"), ("export-height", "128")] {
            let field = app.textFields[id]
            XCTAssertTrue(field.waitForExistence(timeout: 15)); workspaceActivate(field)
            #if os(macOS)
            field.typeKey("a", modifierFlags: .command); field.typeText(value)
            #else
            let current = field.value as? String ?? "2048"
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: current.count) + value)
            #endif
        }
        choose("export-range", "HDR PNG · BT.2020 PQ")
        XCTAssertTrue(app.staticTexts["BT.2020 PQ · 16-bit · Transparency preserved"].waitForExistence(timeout: 15))
        workspaceActivate(app.buttons["export-preview"])
        XCTAssertTrue(app.staticTexts["HDR output · SDR preview"].waitForExistence(timeout: 60))
        attachEditor(in: app, name: "hdr-pq-output")
        for format in ["HDR JPEG · gain map", "HDR AVIF · gain map with transparency"] {
            choose("export-range", format)
            workspaceActivate(app.buttons["export-preview"])
            XCTAssertTrue(app.staticTexts["HDR reconstruction · SDR preview"].firstMatch.waitForExistence(timeout: 90))
            choose("export-preview-rendition", "Encoded SDR base")
            XCTAssertTrue(app.staticTexts["Encoded SDR base"].firstMatch.waitForExistence(timeout: 15))
            attachEditor(in: app, name: format.hasPrefix("HDR JPEG") ? "hdr-jpeg-sdr-base" : "hdr-avif-sdr-base")
            choose("export-preview-rendition", "HDR reconstruction · SDR preview")
        }
        choose("export-range", "OpenEXR · 32-bit float")
        workspaceActivate(app.buttons["export-preview"])
        XCTAssertTrue(app.staticTexts["SDR display preview. OpenEXR preserves document-linear 32-bit float RGB and alpha."].waitForExistence(timeout: 90))
        attachEditor(in: app, name: "hdr-exr-output")
        workspaceActivate(app.buttons["export-cancel"])
    }
}
