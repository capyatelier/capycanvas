import XCTest
#if os(macOS)
import AppKit
#endif

extension XCTestCase {
    @MainActor func editColorValue(_ name: String, _ text: String, in app: XCUIApplication) {
        let value = app.descendants(matching: .any)["color-value-" + name].firstMatch
        workspaceActivate(value)
        let field = app.textFields["color-entry-" + name]
        XCTAssertTrue(field.waitForExistence(timeout: 5), "Tapping a value opens it for typing")
        #if os(macOS)
        field.typeKey("a", modifierFlags: .command); field.typeText(text + "\r")
        #else
        let current = field.value as? String ?? ""
        field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: current.count))
        typeConfirmed(field, text)
        field.typeText("\n")
        if !field.waitForNonExistence(timeout: 2) && field.exists { field.typeText("\n") }
        #endif
        XCTAssertTrue(field.waitForNonExistence(timeout: 5), "Enter commits the value")
    }
    @MainActor func typeConfirmed(_ field: XCUIElement, _ text: String) {
        field.typeText(text)
        #if os(iOS)
        let typed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", text), object: field)
        if XCTWaiter.wait(for: [typed], timeout: 2) != .completed, let current = field.value as? String, text.hasPrefix(current) {
            field.typeText(String(text.dropFirst(current.count)))
        }
        #endif
    }
    @MainActor func elementPixel(_ element: XCUIElement, in app: XCUIApplication) -> (Int, Int, Int) {
        #if os(macOS)
        let reference = app.windows.firstMatch
        #else
        let reference = app.descendants(matching: .any)["canvas"].firstMatch
        #endif
        let f = element.frame, r = reference.frame
        let data = editorPixelSamples(in: app, at: [CGPoint(x: (f.midX - r.minX) / r.width, y: (f.midY - r.minY) / r.height)], size: 4)[0]
        return (Int(data[0]), Int(data[1]), Int(data[2]))
    }
    @MainActor func colorValueText(_ name: String, in app: XCUIApplication) -> String {
        let value = app.descendants(matching: .any)["color-value-" + name].firstMatch
        XCTAssertTrue(value.waitForExistence(timeout: 10))
        return value.value as? String ?? ""
    }
    @MainActor func checkEditColor(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]},{"type":"invoke","command":"select_all"},{"type":"invoke","command":"fill_selection"},{"type":"invoke","command":"deselect"},{"type":"set_color","rgba":[0.9,0.1,0.1,1]}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        let originalPixels = editorPixels(in: app)
        func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        func expectHex(_ hex: String, _ message: String) {
            let match = NSPredicate { _, _ in self.colorValueText("hex", in: app).uppercased() == hex }
            if XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: match, object: app)], timeout: 10) != .completed {
                attachEditor(in: app, name: "edit-color-unexpected-\(theme)"); XCTFail("\(message): \(colorValueText("hex", in: app))")
            }
        }
        let edit = app.buttons["paint-edit-color"].firstMatch, use = app.buttons["color-use"]
        workspaceActivate(edit)
        XCTAssertTrue(use.waitForExistence(timeout: 15), "Edit Color opens")
        for id in ["color-editor-wheel", "color-current", "color-new", "color-pick", "color-copy-hex", "color-format-0", "color-format-1",
                   "color-format-2", "color-copy-0", "color-swatches", "color-editor-shape-circle", "color-editor-shape-square", "color-editor-shape-triangle"] {
            XCTAssertTrue(element(id).waitForExistence(timeout: 5), "Edit Color shows \(id)")
        }
        expectHex("#E61A1A", "The editor starts from the paint")
        #if os(macOS)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("native clipboard sentinel", forType: .string)
        workspaceActivate(element("color-current"))
        app.typeKey("c", modifierFlags: .command)
        expectation(for: NSPredicate { _, _ in NSPasteboard.general.string(forType: .string) == "#E61A1A" }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("native clipboard sentinel", forType: .string)
        app.typeKey("x", modifierFlags: .command)
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "native clipboard sentinel", "Cut without a color text field cannot reach artwork")
        app.typeKey("c", modifierFlags: [.command, .shift])
        app.typeKey("v", modifierFlags: [.command, .shift])
        app.typeKey("v", modifierFlags: .option)
        app.typeKey("n", modifierFlags: [.command, .option])
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), "native clipboard sentinel", "Artwork clipboard variants stay inactive in the color sheet")
        XCTAssertTrue(use.exists, "Artwork paste variants cannot replace the color sheet")
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("#CA4B35", forType: .string)
        app.typeKey("v", modifierFlags: .command)
        expectHex("#CA4B35", "Paste outside a text field edits the color draft")
        workspaceActivate(element("color-current"))
        workspaceActivate(element("color-value-hex"))
        let clipboardField = app.textFields["color-entry-hex"]
        XCTAssertTrue(clipboardField.waitForExistence(timeout: 5))
        let fieldText = clipboardField.value as? String ?? ""
        clipboardField.typeKey("a", modifierFlags: .command)
        clipboardField.typeKey(XCUIKeyboardKey.rightArrow.rawValue, modifierFlags: [])
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString("native clipboard sentinel", forType: .string)
        for key in ["c", "x"] {
            clipboardField.typeKey(key, modifierFlags: .command)
            XCTAssertEqual(NSPasteboard.general.string(forType: .string), "native clipboard sentinel", "Native Copy/Cut without selected text stays native")
            XCTAssertEqual(clipboardField.value as? String, fieldText)
        }
        clipboardField.typeKey("a", modifierFlags: .command)
        clipboardField.typeKey("x", modifierFlags: .command)
        XCTAssertEqual(NSPasteboard.general.string(forType: .string), fieldText)
        XCTAssertEqual(clipboardField.value as? String, "", "Cut removes the native text selection")
        clipboardField.typeKey("v", modifierFlags: .command)
        XCTAssertEqual(clipboardField.value as? String, fieldText, "Paste restores native text instead of changing the artwork")
        clipboardField.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(clipboardField.waitForNonExistence(timeout: 5))
        expectHex("#E61A1A", "Native text clipboard keys preserve the color draft after cancellation")
        #endif
        let current = elementPixel(element("color-current"), in: app)
        XCTAssertTrue(current.0 > 200 && current.1 < 60 && current.2 < 60, "Current shows the paint without tone mapping: \(current)")
        attachEditor(in: app, name: "edit-color-\(theme)")

        workspaceActivate(element("color-editor-shape-square"))
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: element("color-editor-shape-square"))
        waitForExpectations(timeout: 10)
        element("color-editor-wheel").coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).clickOrTap()
        let picked = NSPredicate { _, _ in self.colorValueText("hex", in: app).uppercased() != "#E61A1A" }
        expectation(for: picked, evaluatedWith: app); waitForExpectations(timeout: 10)
        workspaceActivate(element("color-current"))
        expectHex("#E61A1A", "Current reverts the draft")

        workspaceActivate(app.buttons["color-format-0"])
        let unit = app.buttons["color-format-0-rgb_unit"]
        XCTAssertTrue(unit.waitForExistence(timeout: 10), "Row names are format menus")
        workspaceActivate(unit)
        expectation(for: NSPredicate { _, _ in self.colorValueText("0-0", in: app).contains(".") }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        editColorValue("hex", "#00FF00", in: app)
        expectHex("#00FF00", "The hex accepts a whole color")
        editColorValue("0-0", "1", in: app)
        expectHex("#FFFF00", "A value commits through its row")
        let red = element("color-value-0-0")
        #if os(macOS)
        red.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            .click(forDuration: 0.05, thenDragTo: red.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 4)))
        #else
        red.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            .press(forDuration: 0.05, thenDragTo: red.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 4)))
        #endif
        expectation(for: NSPredicate { _, _ in self.colorValueText("hex", in: app).uppercased() != "#FFFF00" }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        #if os(macOS)
        workspaceActivate(app.buttons["color-copy-hex"])
        let copied = colorValueText("hex", in: app)
        expectation(for: NSPredicate { _, _ in NSPasteboard.general.string(forType: .string) == copied }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        #endif

        workspaceActivate(app.buttons["color-swatches"])
        let search = app.textFields["color-sheet-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 10), "The chevron opens every swatch")
        XCTAssertTrue(element("color-tile-sheet").waitForExistence(timeout: 10))
        attachEditor(in: app, name: "edit-color-sheet-\(theme)")
        workspaceActivate(app.buttons["color-sheet-close"])
        XCTAssertTrue(search.waitForNonExistence(timeout: 10))

        workspaceActivate(app.buttons["color-pick"])
        let strip = element("color-strip")
        XCTAssertTrue(strip.waitForExistence(timeout: 10), "The eyedropper shows its strip over the canvas")
        XCTAssertFalse(use.exists, "Picking hides the editor")
        attachEditor(in: app, name: "edit-color-picking-\(theme)")
        canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.6)).clickOrTap()
        XCTAssertTrue(use.waitForExistence(timeout: 15), "A canvas pick returns to Edit Color")
        expectHex("#3373CC", "The editor takes the canvas color")
        workspaceActivate(use)
        XCTAssertTrue(use.waitForNonExistence(timeout: 10))
        workspaceActivate(edit)
        XCTAssertTrue(use.waitForExistence(timeout: 15))
        expectHex("#3373CC", "Use Color sets the paint")
        XCTAssertTrue(colorValueText("0-0", in: app).contains("."), "Row formats are remembered")
        XCTAssertTrue(element("color-tile-recent").waitForExistence(timeout: 10), "Recent colors fill the footer")
        workspaceActivate(app.buttons["color-cancel"])
        XCTAssertTrue(use.waitForNonExistence(timeout: 10))
        expectPixels(originalPixels, in: app)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
    @MainActor func checkFillThumbnailColor(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        func pixels(_ accept: @escaping (Int, Int, Int) -> Bool, _ message: String) {
            let match = NSPredicate { _, _ in
                let data = self.editorPixels(in: app)
                return stride(from: 0, to: data.count, by: 4).allSatisfy { accept(Int(data[$0]), Int(data[$0 + 1]), Int(data[$0 + 2])) }
            }
            if XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: match, object: app)], timeout: 15) != .completed {
                attachEditor(in: app, name: "fill-thumbnail-unexpected-\(theme)"); XCTFail(message)
            }
        }
        pixels({ r, g, b in min(r, g, b) >= 250 }, "A new drawing shows white Paper")
        let paper = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Paper layer row")).firstMatch
        XCTAssertTrue(paper.waitForExistence(timeout: 10))
        workspaceActivate(paper.buttons.matching(NSPredicate(format: "identifier ENDSWITH %@", "-content")).firstMatch)
        let use = app.buttons["color-use"]
        XCTAssertTrue(use.waitForExistence(timeout: 10), "Paper's thumbnail opens Edit Color on its color")
        XCTAssertEqual(colorValueText("hex", in: app).uppercased(), "#FFFFFF")
        let current = elementPixel(app.descendants(matching: .any)["color-current"].firstMatch, in: app)
        XCTAssertTrue(min(current.0, current.1, current.2) >= 250, "Current shows Paper's white: \(current)")
        editColorValue("hex", "#FF0000", in: app)
        attachEditor(in: app, name: "fill-thumbnail-edit-\(theme)")
        workspaceActivate(use)
        XCTAssertTrue(use.waitForNonExistence(timeout: 10))
        pixels({ r, g, b in r >= 250 && g <= 5 && b <= 5 }, "Use Color sets the fill")
        editorHistory("Undo", in: app)
        pixels({ r, g, b in min(r, g, b) >= 250 }, "Undo restores the fill")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
