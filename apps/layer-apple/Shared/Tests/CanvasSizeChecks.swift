import XCTest

private func displayed(_ element: XCUIElement) -> String {
    (element.value as? String).flatMap { $0.isEmpty ? nil : $0 } ?? element.label
}

private func pixelDimensions(_ element: XCUIElement) -> (width: Int, height: Int)? {
    let values = displayed(element).components(separatedBy: CharacterSet.decimalDigits.inverted).compactMap(Int.init)
    guard values.count == 2, values[0] > 1, values[1] > 0 else { return nil }
    return (values[0], values[1])
}

extension XCTestCase {
    @MainActor func checkCanvasSize(in app: XCUIApplication, theme: String = "dark") {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Image", id: "canvas_size", label: "Canvas Size…")
        let apply = app.buttons["canvas-size-apply"], message = app.staticTexts["canvas-size-message"]
        let title = editorDocumentTitle(in: app)
        func expect(_ element: XCUIElement, _ format: String, _ argument: String) {
            expectation(for: NSPredicate(format: format, argument), evaluatedWith: element); waitForExpectations(timeout: 10)
        }
        XCTAssertTrue(apply.waitForExistence(timeout: 15), "Canvas Size opens its dialog")
        XCTAssertFalse(apply.isEnabled, "The current size has nothing to apply")
        guard let initial = pixelDimensions(message) else {
            return XCTFail("Canvas Size exposes its initial pixel dimensions")
        }
        let initialWidth = initial.width, initialHeight = initial.height
        let newWidth = initialWidth - 1
        XCTAssertEqual(displayed(message), "Current size: \(initialWidth) × \(initialHeight) px")
        let topLeft = app.buttons["canvas-size-anchor-top_left"]
        workspaceActivate(topLeft)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: topLeft); waitForExpectations(timeout: 5)
        let width = app.textFields["number-entry-canvas-size-width"]
        #if os(iOS)
        checkDialogKeyboardHeight(app.descendants(matching: .any)["canvas-size-dialog"].firstMatch, in: app) { workspaceActivate(width) }
        #endif
        replaceEditorText(width, with: "\(newWidth)", in: app)
        #if os(macOS)
        width.typeKey(XCUIKeyboardKey.return.rawValue, modifierFlags: [])
        #else
        width.typeText("\n")
        dismissEditorKeyboard(in: app)
        #endif
        expectation(for: NSPredicate { _, _ in displayed(message) == "New size: \(newWidth) × \(initialHeight) px" }, evaluatedWith: message)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "canvas-size")
        workspaceActivate(apply)
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10), "Apply closes the dialog")
        expect(title, "label CONTAINS %@", "\(newWidth) × \(initialHeight)")
        editorHistory("Undo", in: app)
        expect(title, "label CONTAINS %@", "\(initialWidth) × \(initialHeight)")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}

extension XCTestCase {
    @MainActor func checkImageSize(in app: XCUIApplication, theme: String = "dark") {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Image", id: "image_size", label: "Image Size…")
        let apply = app.buttons["image-size-apply"], message = app.staticTexts["image-size-message"]
        let title = editorDocumentTitle(in: app)
        XCTAssertTrue(apply.waitForExistence(timeout: 15), "Image Size opens its dialog")
        XCTAssertFalse(apply.isEnabled, "The current size has nothing to apply")
        XCTAssertTrue(app.descendants(matching: .any)["image-size-resample"].exists)
        guard let initial = pixelDimensions(message) else {
            return XCTFail("Image Size exposes its initial pixel dimensions")
        }
        let initialWidth = initial.width, initialHeight = initial.height
        let newWidth = initialWidth - 1
        let newHeight = Int((Double(initialHeight) * Double(newWidth) / Double(initialWidth)).rounded())
        XCTAssertEqual(displayed(message), "Current size: \(initialWidth) × \(initialHeight) px")
        let width = app.textFields["number-entry-image-size-width"]
        #if os(iOS)
        checkDialogKeyboardHeight(app.descendants(matching: .any)["image-size-dialog"].firstMatch, in: app) { workspaceActivate(width) }
        #endif
        replaceEditorText(width, with: "\(newWidth)", in: app)
        #if os(macOS)
        width.typeKey(XCUIKeyboardKey.return.rawValue, modifierFlags: [])
        #else
        width.typeText("\n")
        dismissEditorKeyboard(in: app)
        #endif
        expectation(for: NSPredicate { _, _ in displayed(message) == "New size: \(newWidth) × \(newHeight) px" }, evaluatedWith: message)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(apply.isEnabled)
        attachEditor(in: app, name: "image-size")
        workspaceActivate(apply)
        XCTAssertTrue(apply.waitForNonExistence(timeout: 10), "Apply closes the dialog")
        expectation(for: NSPredicate(format: "label CONTAINS %@", "\(newWidth) × \(newHeight)"), evaluatedWith: title); waitForExpectations(timeout: 20)
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate(format: "label CONTAINS %@", "\(initialWidth) × \(initialHeight)"), evaluatedWith: title); waitForExpectations(timeout: 20)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkFrequencySeparation(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        app.launch(); capturePaintEditor(in: app)
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let before = rows.count
        editorMenu(in: app, menu: "Filter", id: "frequency_separation", label: "Frequency Separation…")
        let panel = app.descendants(matching: .any)["frequency-separation-panel"].firstMatch
        XCTAssertTrue(panel.waitForExistence(timeout: 15), "Frequency Separation opens its panel over the canvas")
        workspaceActivate(app.buttons["number-value-frequency-separation-value"])
        app.textFields["number-entry-frequency-separation-value"].typeText("8.5\n")
        attachEditor(in: app, name: "frequency-separation")
        workspaceActivate(app.buttons["frequency-separation-apply"])
        XCTAssertTrue(panel.waitForNonExistence(timeout: 20), "Apply closes the panel")
        expectation(for: NSPredicate { _, _ in rows.count > before }, evaluatedWith: app); waitForExpectations(timeout: 20)
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in rows.count == before }, evaluatedWith: app); waitForExpectations(timeout: 20)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
