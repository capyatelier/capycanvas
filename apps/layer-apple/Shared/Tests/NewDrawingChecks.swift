import XCTest

extension XCTestCase {
    @MainActor func newDrawingNumber(_ name: String, in app: XCUIApplication) -> XCUIElement {
        app.textFields["number-entry-new-drawing-" + name]
    }
    @MainActor func newDrawingElement(_ id: String, in app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any)[id].firstMatch
    }
    @MainActor func chooseNewDrawingOption(_ id: String, _ index: Int, in app: XCUIApplication) {
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        #endif
        workspaceActivate(app.buttons[id].firstMatch)
        workspaceActivate(app.buttons["\(id)-option-\(index)"].firstMatch)
    }
    @MainActor func enterNewDrawingNumber(_ name: String, _ value: String, in app: XCUIApplication) {
        let field = newDrawingNumber(name, in: app)
        replaceEditorText(field, with: value, in: app)
        #if os(macOS)
        field.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [])
        #else
        field.typeText("\n")
        dismissEditorKeyboard(in: app)
        #endif
    }
    @MainActor func expectNewDrawingText(_ id: String, in app: XCUIApplication, _ shows: @escaping (String) -> Bool) {
        let element = newDrawingElement(id, in: app)
        expectation(for: NSPredicate { _, _ in shows(self.editorText(element)) }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
    }
    @MainActor func expectNewDrawingValue(_ id: String, _ value: String, in app: XCUIApplication) {
        expectation(for: NSPredicate(format: "value == %@", value), evaluatedWith: app.buttons[id].firstMatch)
        waitForExpectations(timeout: 15)
    }
    @MainActor func createNewDrawing(width: String, height: String, depth: Int? = nil, in app: XCUIApplication) {
        let create = app.buttons["new-drawing-create"]
        XCTAssertTrue(create.waitForExistence(timeout: 60), "The New drawing dialog did not open")
        chooseNewDrawingOption("new-drawing-unit", 3, in: app)
        enterNewDrawingNumber("width", width, in: app)
        enterNewDrawingNumber("height", height, in: app)
        if let depth { chooseNewDrawingOption("new-drawing-depth", depth, in: app) }
        workspaceActivate(create)
        XCTAssertTrue(create.waitForNonExistence(timeout: 60))
    }

    @MainActor func createWhitePaperDrawing(width: String = "2400", height: String = "1800", in app: XCUIApplication) {
        editorMenu(in: app, menu: "File", id: "new_document", label: "New…")
        XCTAssertTrue(app.buttons["new-drawing-create"].waitForExistence(timeout: 60))
        workspaceActivate(app.buttons["new-drawing-texture"])
        enterNewDrawingNumber("paper-relief", "0", in: app)
        let usePaper = app.buttons["new-drawing-paper-use"]
        workspaceActivate(usePaper)
        XCTAssertTrue(usePaper.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["new-drawing-paper-color"])
        let white = app.buttons["new-drawing-paper-color-0"]
        XCTAssertEqual(white.label, "White")
        workspaceActivate(white)
        let useColor = app.buttons["new-drawing-color-use"]
        workspaceActivate(useColor)
        XCTAssertTrue(useColor.waitForNonExistence(timeout: 10))
        createNewDrawing(width: width, height: height, depth: 0, in: app)
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        expectation(for: NSPredicate { _, _ in
            self.editorPixels(in: app).enumerated().allSatisfy { $0.offset % 4 == 3 || $0.element >= 253 }
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
    }

    #if os(iOS)
    @MainActor func selectedNewDrawingDocument(in app: XCUIApplication, expectedTitle: String? = nil) -> (id: String, label: String, value: String?) {
        editorMenu(in: app, menu: "Window", id: "drawings", label: "Drawings…")
        let selector = app.descendants(matching: .any)["drawing-selector"].firstMatch
        XCTAssertTrue(selector.waitForExistence(timeout: 15))
        let selected = selector.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND selected == YES", "drawing-tab-"))
        expectation(for: NSPredicate { _, _ in selected.count == 1 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        if let expectedTitle {
            expectation(for: NSPredicate { _, _ in
                guard selected.count == 1 else { return false }
                let document = selected.firstMatch
                return document.label.contains(expectedTitle) || (document.value as? String)?.contains(expectedTitle) == true
            }, evaluatedWith: app)
            waitForExpectations(timeout: 30)
        }
        guard let snapshot = try? selected.firstMatch.snapshot() else {
            XCTFail("The Drawings selector must expose its selected drawing")
            return ("", "", nil)
        }
        XCTAssertTrue(snapshot.isSelected)
        XCTAssertTrue(snapshot.identifier.hasPrefix("drawing-tab-"))
        let result = (id: snapshot.identifier, label: snapshot.label, value: snapshot.value as? String)
        let proof = XCTAttachment(string: "Selected drawing ID: \(result.id)\nLabel: \(result.label)\nValue: \(result.value ?? "nil")\n\(selector.debugDescription)")
        proof.name = "new-drawing-selected-document"; proof.lifetime = .keepAlways; add(proof)
        attachEditor(in: app, name: "new-drawing-selected-document")
        workspaceActivate(app.buttons["Done"].firstMatch)
        XCTAssertTrue(selector.waitForNonExistence(timeout: 10))
        return result
    }
    #endif

    @MainActor func checkNewDrawingDialog(in app: XCUIApplication, theme: String) {
        let create = app.buttons["new-drawing-create"]
        XCTAssertTrue(create.waitForExistence(timeout: 60), "The New drawing dialog did not open after canvas startup")
        expectNewDrawingText("new-drawing-caption", in: app) { $0.hasPrefix("Sketchbook A4") }
        let resolution = newDrawingNumber("resolution", in: app)
        XCTAssertEqual(resolution.value as? String, "300")
        XCTAssertTrue(create.isEnabled)
        attachEditor(in: app, name: "new-drawing-main-" + theme)

        workspaceActivate(app.buttons["new-drawing-resolution-choices"])
        let line = app.buttons["new-drawing-resolution-4"]
        XCTAssertTrue(line.waitForExistence(timeout: 10))
        attachEditor(in: app, name: "new-drawing-resolutions-" + theme)
        workspaceActivate(line)
        expectation(for: NSPredicate(format: "value == %@", "600"), evaluatedWith: resolution)
        waitForExpectations(timeout: 15)

        workspaceActivate(app.buttons["new-drawing-presets-button"])
        let search = app.textFields["new-drawing-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        #if os(iOS)
        expectation(for: NSPredicate { _, _ in (!create.exists || !create.isEnabled) && search.isHittable }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        #else
        XCTAssertTrue(create.waitForNonExistence(timeout: 5), "The presets cover the page")
        #endif
        attachEditor(in: app, name: "new-drawing-presets-" + theme)
        app.typeText("chalk")
        expectation(for: NSPredicate(format: "value == %@", "chalk"), evaluatedWith: search)
        waitForExpectations(timeout: 10)
        #if os(macOS)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        expectation(for: NSPredicate(format: "value == %@", ""), evaluatedWith: search)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(search.exists, "Escape clears search before closing Presets")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(search.waitForNonExistence(timeout: 10))
        expectation(for: NSPredicate(format: "hasKeyboardFocus == YES"), evaluatedWith: app.buttons["new-drawing-presets-button"])
        waitForExpectations(timeout: 10)
        workspaceActivate(app.buttons["new-drawing-presets-button"])
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        app.typeText("chalk")
        #endif
        let chalkboard = app.buttons["new-drawing-preset-0"]
        expectation(for: NSPredicate(format: "label == %@", "Chalkboard"), evaluatedWith: chalkboard)
        waitForExpectations(timeout: 15)
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        #endif
        workspaceActivate(chalkboard)
        expectNewDrawingText("new-drawing-summary-name", in: app) { $0 == "Chalkboard" }
        expectNewDrawingText("new-drawing-summary-details", in: app) { $0.hasSuffix("Chalkboard 50 %, Chalkboard") }
        workspaceActivate(app.buttons["new-drawing-save-as"])
        let saveName = app.textFields["new-drawing-save-name"]
        XCTAssertTrue(saveName.waitForExistence(timeout: 10))
        #if os(macOS)
        app.typeText("Draft")
        expectation(for: NSPredicate(format: "value CONTAINS %@", "Draft"), evaluatedWith: saveName)
        waitForExpectations(timeout: 10)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(saveName.waitForNonExistence(timeout: 10))
        XCTAssertEqual(search.value as? String, "chalk", "Leaving naming keeps search")
        expectation(for: NSPredicate(format: "hasKeyboardFocus == YES"), evaluatedWith: search)
        waitForExpectations(timeout: 10)
        workspaceActivate(app.buttons["new-drawing-save-as"])
        XCTAssertTrue(saveName.waitForExistence(timeout: 10))
        #endif
        replaceEditorText(saveName, with: "Studio wall", in: app)
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        #endif
        attachEditor(in: app, name: "new-drawing-save-as-" + theme)
        workspaceActivate(app.buttons["new-drawing-save-confirm"])
        XCTAssertTrue(saveName.waitForNonExistence(timeout: 10))
        expectNewDrawingText("new-drawing-summary-name", in: app) { $0 == "Studio wall" }
        workspaceActivate(app.buttons["new-drawing-select"])
        XCTAssertTrue(search.waitForNonExistence(timeout: 10))
        #if os(macOS)
        expectation(for: NSPredicate(format: "hasKeyboardFocus == YES"), evaluatedWith: app.buttons["new-drawing-presets-button"])
        waitForExpectations(timeout: 10)
        #endif
        expectNewDrawingText("new-drawing-caption", in: app) { $0.hasPrefix("Studio wall") }
        XCTAssertEqual(resolution.value as? String, "150", "Selecting a preset restores its resolution")

        enterNewDrawingNumber("resolution", "10000", in: app)
        expectation(for: NSPredicate(format: "enabled == NO"), evaluatedWith: create)
        waitForExpectations(timeout: 15)
        expectNewDrawingText("new-drawing-size", in: app) { $0.hasPrefix("Too large for this device") }
        enterNewDrawingNumber("resolution", "150", in: app)
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: create)
        waitForExpectations(timeout: 15)

        checkNewDrawingPaper(in: app, theme: theme)

        let paperColor = app.buttons["new-drawing-paper-color"]
        workspaceActivate(paperColor)
        let useColor = app.buttons["new-drawing-color-use"]
        XCTAssertTrue(useColor.waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["color-pick"].exists, "The paper color has no eyedropper")
        let kraft = app.buttons["new-drawing-paper-color-5"]
        XCTAssertEqual(kraft.label, "Kraft")
        workspaceActivate(kraft)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: kraft)
        waitForExpectations(timeout: 10)
        checkNewDrawingPaperColorDraft(in: app)
        attachEditor(in: app, name: "new-drawing-paper-color-" + theme)
        workspaceActivate(useColor)
        XCTAssertTrue(useColor.waitForNonExistence(timeout: 10))
        expectNewDrawingValue("new-drawing-paper-color", "Kraft", in: app)
        workspaceActivate(paperColor)
        XCTAssertTrue(useColor.waitForExistence(timeout: 10))
        workspaceActivate(app.buttons["new-drawing-color-mode-transparent"])
        XCTAssertTrue(app.staticTexts["color-transparent"].waitForExistence(timeout: 10))
        attachEditor(in: app, name: "new-drawing-transparent-" + theme)
        workspaceActivate(useColor)
        XCTAssertTrue(useColor.waitForNonExistence(timeout: 10))
        expectNewDrawingValue("new-drawing-paper-color", "Transparent", in: app)

        chooseNewDrawingOption("new-drawing-unit", 3, in: app)
        enterNewDrawingNumber("width", "640", in: app)
        enterNewDrawingNumber("height", "480", in: app)
        let name = app.textFields["new-drawing-name"]
        replaceEditorText(name, with: "Harbor study", in: app)
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        #endif
        workspaceActivate(create)
        XCTAssertTrue(create.waitForNonExistence(timeout: 60))
        #if os(macOS)
        let title = editorDocumentTitle(in: app)
        expectation(for: NSPredicate(format: "label BEGINSWITH %@ OR value BEGINSWITH %@", "Harbor study", "Harbor study"), evaluatedWith: title)
        waitForExpectations(timeout: 30)
        let created = editorText(title)
        #else
        let created = selectedNewDrawingDocument(in: app)
        XCTAssertTrue(created.label.hasPrefix("Harbor study") || created.value?.hasPrefix("Harbor study") == true)
        #endif
        attachEditor(in: app, name: "new-drawing-created-" + theme)

        #if os(macOS)
        app.typeKey("n", modifierFlags: .command)
        #else
        editorMenu(in: app, menu: "File", id: "new_document", label: "New…")
        #endif
        XCTAssertTrue(create.waitForExistence(timeout: 15))
        XCTAssertEqual(newDrawingNumber("width", in: app).value as? String, "640", "New reopens with the last drawing")
        checkNewDrawingDraftTransitions(in: app)
        workspaceActivate(app.buttons["new-drawing-cancel"])
        XCTAssertTrue(create.waitForNonExistence(timeout: 15))
        #if os(macOS)
        XCTAssertEqual(editorText(title), created, "Cancel keeps the current drawing")
        #else
        let retained = selectedNewDrawingDocument(in: app)
        XCTAssertEqual(retained.id, created.id, "Cancel keeps the current drawing")
        XCTAssertEqual(retained.label, created.label)
        XCTAssertEqual(retained.value, created.value)
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkNewDrawingDraftTransitions(in app: XCUIApplication) {
        let width = newDrawingNumber("width", in: app), height = newDrawingNumber("height", in: app)
        let unit = app.buttons["new-drawing-unit"], presets = app.buttons["new-drawing-presets-button"]
        let search = app.textFields["new-drawing-search"]
        replaceEditorText(width, with: "800 + 40", in: app)
        chooseNewDrawingOption("new-drawing-unit", 0, in: app)
        expectNewDrawingValue("new-drawing-unit", "mm", in: app)
        chooseNewDrawingOption("new-drawing-unit", 3, in: app)
        expectation(for: NSPredicate(format: "value == %@", "840"), evaluatedWith: width)
        waitForExpectations(timeout: 10)

        replaceEditorText(width, with: "1 / 0", in: app)
        chooseNewDrawingOption("new-drawing-unit", 0, in: app)
        XCTAssertEqual(unit.value as? String, "px", "An invalid draft cannot switch its unit and replace the field")
        XCTAssertEqual(width.value as? String, "1 / 0", "A refused unit change preserves the text for correction")
        workspaceActivate(presets)
        XCTAssertFalse(search.waitForExistence(timeout: 2), "An invalid size keeps Presets from covering its error")
        XCTAssertTrue(app.buttons["new-drawing-create"].exists)
        workspaceActivate(app.buttons["new-drawing-portrait"])
        XCTAssertEqual(width.value as? String, "1 / 0", "Turning the sheet cannot discard an invalid size")
        XCTAssertEqual(height.value as? String, "480", "A refused orientation change preserves the accepted sheet")

        replaceEditorText(width, with: "960", in: app)
        workspaceActivate(app.buttons["new-drawing-portrait"])
        expectation(for: NSPredicate(format: "value == %@", "480"), evaluatedWith: width)
        expectation(for: NSPredicate(format: "value == %@", "960"), evaluatedWith: height)
        waitForExpectations(timeout: 10)
        replaceEditorText(width, with: "500 + 12", in: app)
        workspaceActivate(presets)
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        #endif
        workspaceActivate(app.buttons["new-drawing-browser-cancel"])
        XCTAssertTrue(search.waitForNonExistence(timeout: 10))
        XCTAssertEqual(width.value as? String, "512", "Opening Presets admits the valid size before covering the page")
        XCTAssertEqual(height.value as? String, "960")
    }

    @MainActor func checkNewDrawingPaperColorDraft(in app: XCUIApplication) {
        let scroll = newDrawingElement("new-drawing-color-drawer", in: app).scrollViews.firstMatch
        func reveal(_ id: String) -> XCUIElement {
            let element = newDrawingElement(id, in: app)
            #if os(iOS)
            revealEditorControl(element, identifier: id, type: .any, in: scroll)
            #endif
            return element
        }
        guard let before = Int(editorText(reveal("new-drawing-paper-color-count"))) else {
            return XCTFail("The paper palette must expose its shared color count")
        }
        let entry = app.textFields["color-entry-hex"]
        func revealHex() { _ = reveal(entry.exists ? "color-entry-hex" : "color-value-hex") }
        revealHex()
        editColorValue("hex", "#E07080", in: app)
        let add = app.buttons["new-drawing-add-paper-color"]
        _ = reveal("new-drawing-add-paper-color")
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: add)
        waitForExpectations(timeout: 10)
        revealHex()
        if !entry.exists { workspaceActivate(reveal("color-value-hex")) }
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        replaceEditorText(entry, with: "#4A7CBD", in: app)
        workspaceActivate(reveal("new-drawing-add-paper-color"))
        XCTAssertEqual(editorText(reveal("new-drawing-paper-color-count")), String(before + 1))
        let stored = reveal("new-drawing-paper-color-\(before)")
        XCTAssertTrue(stored.waitForExistence(timeout: 10))
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: stored)
        waitForExpectations(timeout: 10)
        workspaceActivate(reveal("new-drawing-paper-color-5"))
        workspaceActivate(reveal("new-drawing-paper-color-\(before)"))
        revealHex()
        expectation(for: NSPredicate { _, _ in self.colorValueText("hex", in: app).uppercased() == "#4A7CBD" }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        workspaceActivate(reveal("new-drawing-paper-color-5"))
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: app.buttons["new-drawing-paper-color-5"])
        waitForExpectations(timeout: 10)
    }

    @MainActor func checkNewDrawingPaper(in app: XCUIApplication, theme: String) {
        let textureTile = app.buttons["new-drawing-texture"], useTexture = app.buttons["new-drawing-paper-use"]
        let relief = newDrawingNumber("paper-relief", in: app)
        expectNewDrawingValue("new-drawing-texture", "Chalkboard, 50 %", in: app)
        workspaceActivate(textureTile)
        XCTAssertTrue(useTexture.waitForExistence(timeout: 10))
        XCTAssertEqual(relief.value as? String, "50", "Texture visibility opens with the sheet's value")
        XCTAssertTrue(relief.isEnabled)
        let noPaper = app.buttons["new-drawing-paper-tile-0"]
        XCTAssertTrue(noPaper.waitForExistence(timeout: 10))
        let none = noPaper.label
        workspaceActivate(noPaper)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: noPaper)
        expectation(for: NSPredicate(format: "enabled == NO"), evaluatedWith: relief)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "new-drawing-no-paper-" + theme)
        workspaceActivate(useTexture)
        XCTAssertTrue(useTexture.waitForNonExistence(timeout: 10))
        expectNewDrawingValue("new-drawing-texture", none, in: app)

        let coldPress = app.buttons["new-drawing-paper-tile-2"]
        func pickColdPress() {
            workspaceActivate(textureTile)
            XCTAssertTrue(useTexture.waitForExistence(timeout: 10))
            XCTAssertFalse(relief.isEnabled, "No paper has no texture to show")
            workspaceActivate(coldPress)
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: coldPress)
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: relief)
            waitForExpectations(timeout: 10)
            XCTAssertEqual(relief.value as? String, "50")
        }
        pickColdPress()
        let texture = coldPress.label
        let importTile = app.buttons["new-drawing-paper-import"]
        XCTAssertTrue(importTile.waitForExistence(timeout: 10))
        XCTAssertEqual(importTile.label, "Import…")
        XCTAssertEqual(importTile.frame.size, noPaper.frame.size, "Import… is a tile like No paper")
        revealEditorControl(importTile, identifier: "new-drawing-paper-import", type: .button,
            in: newDrawingElement("new-drawing-paper-drawer", in: app).scrollViews.firstMatch)
        XCTAssertEqual(newDrawingElement("new-drawing-papers", in: app).buttons.allElementsBoundByIndex.last?.identifier,
            importTile.identifier, "Import… follows the imported papers")
        attachEditor(in: app, name: "new-drawing-paper-import-" + theme)
        workspaceActivate(importTile)
        #if os(macOS)
        let chooser = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(chooser.waitForExistence(timeout: 20), "Import… opens the file chooser")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        #else
        let chooser = app.buttons.matching(NSPredicate(format: "label == %@ AND NOT (identifier BEGINSWITH %@)", "Cancel", "new-drawing-")).firstMatch
        XCTAssertTrue(chooser.waitForExistence(timeout: 20), "Import… opens the file chooser")
        workspaceActivate(chooser)
        #endif
        XCTAssertTrue(chooser.waitForNonExistence(timeout: 15))
        XCTAssertTrue(useTexture.exists, "Cancelling the import keeps the paper drawer")
        XCTAssertTrue(coldPress.isSelected, "Import… keeps the marked paper")
        XCTAssertFalse(importTile.isSelected)

        replaceEditorText(relief, with: "250", in: app)
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        #endif
        workspaceActivate(app.buttons["new-drawing-paper-cancel"])
        XCTAssertTrue(useTexture.waitForNonExistence(timeout: 10))
        expectNewDrawingValue("new-drawing-texture", none, in: app)

        pickColdPress()
        replaceEditorText(relief, with: "150", in: app)
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        #endif
        attachEditor(in: app, name: "new-drawing-paper-" + theme)
        workspaceActivate(useTexture)
        XCTAssertTrue(useTexture.waitForNonExistence(timeout: 10))
        expectNewDrawingValue("new-drawing-texture", texture + ", 150 %", in: app)
        attachEditor(in: app, name: "new-drawing-paper-relief-" + theme)
    }
}
