import XCTest

extension EditorLaunchTests {
    @MainActor func testPhoneDialogsLight() throws { try checkPhoneDialogs(theme: "light") }
    @MainActor func testPhoneDialogsDark() throws { try checkPhoneDialogs(theme: "dark") }

    @MainActor func testPhoneSettingsEditorsLight() throws { try checkPhoneSettingsEditors(theme: "light") }
    @MainActor func testPhoneSettingsEditorsDark() throws { try checkPhoneSettingsEditors(theme: "dark") }
    @MainActor func testPhoneCanvasDialogsLight() throws { try checkPhoneCanvasDialogs(theme: "light") }
    @MainActor func testPhoneCanvasDialogsDark() throws { try checkPhoneCanvasDialogs(theme: "dark") }

    @MainActor private func phonePageActivate(_ id: String, in app: XCUIApplication) {
        let item = app.buttons[id].firstMatch, list = app.collectionViews.firstMatch
        if !item.waitForExistence(timeout: 2) && list.exists { revealEditorControl(item, identifier: id, type: .button, in: list) }
        XCTAssertTrue(item.waitForExistence(timeout: 15), id)
        let scrolls = app.scrollViews.containing(.button, identifier: id).allElementsBoundByIndex
            + app.collectionViews.containing(.button, identifier: id).allElementsBoundByIndex
        if let scroll = scrolls.last { revealEditorControl(item, in: scroll) }
        workspaceActivate(item)
    }
    @MainActor private func phonePageCapture(_ state: String, in app: XCUIApplication, theme: String) throws {
        let data = try XCTUnwrap(app.launchEnvironment["CAPY_INITIAL_ACTIONS"]?.data(using: .utf8))
        let fixture = try XCTUnwrap(try JSONSerialization.jsonObject(with: data) as? [[String: Any]])
        phoneCapture(in: app, id: phoneCaptureID("pages", state), theme: theme, fixture: fixture)
    }
    @MainActor private func checkPhoneSettingsEditors(theme: String) throws {
        let app = try phoneCaptureApplication(theme: theme)
        defer { app.terminate() }
        func tap(_ id: String) { phonePageActivate(id, in: app) }
        func shot(_ state: String) throws { try phonePageCapture(state, in: app, theme: theme) }
        func recordingFooter() {
            let text = app.staticTexts["shortcut-captured"], cancel = app.buttons["shortcut-cancel"], confirm = app.buttons["shortcut-confirm"]
            let window = app.windows.firstMatch.frame
            XCTAssertTrue(cancel.isHittable); XCTAssertTrue(confirm.exists)
            XCTAssertGreaterThan(cancel.frame.minY, text.frame.maxY)
            XCTAssertEqual(cancel.frame.midY, confirm.frame.midY, accuracy: 1)
            let keyboard = app.keyboards.firstMatch
            XCTAssertLessThanOrEqual(confirm.frame.maxY, keyboard.exists ? min(window.maxY, keyboard.frame.minY) : window.maxY)
            let scroll = app.collectionViews.containing(.staticText, identifier: "shortcut-captured").firstMatch
            if scroll.exists {
                XCTAssertFalse(scroll.buttons["shortcut-cancel"].exists)
                XCTAssertFalse(scroll.buttons["shortcut-confirm"].exists)
                let before = [cancel.frame, confirm.frame]
                scroll.swipeUp()
                XCTAssertEqual(cancel.frame, before[0]); XCTAssertEqual(confirm.frame, before[1])
                scroll.swipeDown()
                XCTAssertEqual(cancel.frame, before[0]); XCTAssertEqual(confirm.frame, before[1])
            }
        }
        func settings(_ page: String) {
            _ = phoneMenuRoot(in: app); tap("phone-preferences"); tap("settings-page-" + page)
        }
        settings("shortcuts")
        let search = app.textFields["shortcut-search"]
        replaceEditorText(search, with: "Redo", in: app); search.typeText(XCUIKeyboardKey.return.rawValue)
        tap("shortcut-command.Redo"); try shot("shortcut-editor"); tap("shortcut-editor-done")
        replaceEditorText(search, with: "Hand", in: app); search.typeText(XCUIKeyboardKey.return.rawValue)
        tap("shortcut-command.Hand"); tap("shortcut-add")
        XCTAssertTrue(app.staticTexts["shortcut-captured"].waitForExistence(timeout: 10)); recordingFooter(); try shot("shortcut-recording")
        app.typeKey("e", modifierFlags: [])
        let confirm = app.buttons["shortcut-confirm"]
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Reassign", "Reassign"), evaluatedWith: confirm)
        waitForExpectations(timeout: 10); recordingFooter(); try shot("shortcut-conflict")
        tap("shortcut-cancel"); tap("shortcut-editor-done")
        replaceEditorText(search, with: "", in: app); search.typeText(XCUIKeyboardKey.return.rawValue)
        tap("shortcut-category-Modifier keys"); tap("add-modifier-key")
        XCTAssertTrue(app.staticTexts["shortcut-captured"].waitForExistence(timeout: 10)); recordingFooter(); try shot("modifier-key"); tap("shortcut-cancel")
        tap("modifier-Space")
        let same = app.switches["modifier-same"]
        if same.value as? String != "0" { workspaceActivate(same) }
        expectation(for: NSPredicate(format: "value == %@", "0"), evaluatedWith: same)
        waitForExpectations(timeout: 10)
        try shot("modifier-per-tool"); tap("settings-back"); tap("settings-back")
        tap("keymap-menu"); workspaceActivate(app.buttons["Differences…"].firstMatch)
        XCTAssertTrue(app.buttons["keymap-details-done"].waitForExistence(timeout: 10)); try shot("keymap-details"); tap("keymap-details-done")
        let token = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_FILE_TEST_TOKEN"])
        XCTAssertNotNil(UUID(uuidString: token))
        let provider = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_UI_PACKAGE_PROVIDER"])
        XCTAssertEqual(provider, "Capy Test Files " + token)
        let keymap = app.descendants(matching: .any)["keymap-preset"].firstMatch
        let preset = editorText(keymap)
        let headerTop = app.buttons["settings-done"].frame.minY
        tap("keymap-menu"); tap("keymap-import-button")
        phoneChooseProviderFile("Phone keymap.capykeys", token: token, provider: provider, submitSelection: false, in: app)
        XCTAssertTrue(app.buttons["confirm-keymap-import"].waitForExistence(timeout: 15))
        for identifier in ["keymap-menu", "settings-done"] {
            let background = app.buttons[identifier]
            XCTAssertTrue(!background.exists || !background.isEnabled, "The import confirmation must disable the Settings form and header")
        }
        let heading = app.staticTexts["editor-alert-title"]
        XCTAssertTrue(heading.exists)
        XCTAssertGreaterThanOrEqual(heading.frame.minY, headerTop, "The alert heading stays below the native status area")
        _ = workspacePoint(app.buttons["confirm-keymap-import"])
        _ = workspacePoint(app.buttons["cancel-keymap-import"])
        try shot("keymap-import")
        tap("cancel-keymap-import")
        XCTAssertTrue(app.buttons["confirm-keymap-import"].waitForNonExistence(timeout: 10))
        XCTAssertEqual(editorText(keymap), preset, "Cancelling the native keymap preview retains the current preset")
        tap("settings-done"); settings("touch")
        XCTAssertFalse(app.buttons["trigger-pen.button.primary"].exists)
        tap("trigger-touch.tap.2")
        XCTAssertTrue(app.textFields["action-picker-search"].waitForExistence(timeout: 10)); try shot("choose-action")
        tap("action-picker-close"); tap("settings-done")
    }
    @MainActor private func checkPhoneCanvasDialogs(theme: String) throws {
        let app = try phoneCaptureApplication(theme: theme)
        defer { app.terminate() }
        func tap(_ id: String) { phonePageActivate(id, in: app) }
        func shot(_ state: String) throws { try phonePageCapture(state, in: app, theme: theme) }
        for (kind, state) in [("pressure", "pressure-calibration"), ("tilt", "tilt-calibration")] {
            _ = phoneMenuRoot(in: app); tap("phone-preferences"); tap("settings-page-input"); tap("preference-pen_" + kind)
            XCTAssertTrue(app.descendants(matching: .any)["pen-" + kind + "-dialog"].firstMatch.waitForExistence(timeout: 15))
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 10))
            let plot = app.descendants(matching: .any)["effect-curve"].firstMatch
            XCTAssertTrue(plot.waitForExistence(timeout: 10))
            XCTAssertGreaterThanOrEqual(plot.frame.width, 200)
            XCTAssertGreaterThanOrEqual(plot.frame.height, 200)
            try shot(state); tap("pen-" + kind + "-cancel")
        }
        phoneAlertCommand("Select", label: "Select all pixels", in: app)
        phoneAlertCommand("Select", label: "Feather Selection…", in: app)
        XCTAssertTrue(app.buttons["selection-refine-cancel"].waitForExistence(timeout: 15))
        workspaceActivate(app.buttons["number-value-selection-refine-value"])
        let refine = app.textFields["number-entry-selection-refine-value"]
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        expectation(for: NSPredicate { _, _ in
            refine.isHittable && refine.frame.maxY <= app.keyboards.firstMatch.frame.minY + 1
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.buttons["selection-refine-cancel"].isHittable)
        XCTAssertTrue(app.buttons["selection-refine-apply"].isHittable)
        try shot("selection-refine"); tap("selection-refine-cancel")
        phoneAlertCommand("Select", label: "Deselect pixels", in: app)
        phoneAlertCommand("Filter", label: "Frequency Separation…", in: app)
        XCTAssertTrue(app.buttons["frequency-separation-cancel"].waitForExistence(timeout: 15))
        workspaceActivate(app.buttons["number-value-frequency-separation-value"])
        let frequency = app.textFields["number-entry-frequency-separation-value"]
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        expectation(for: NSPredicate { _, _ in
            frequency.isHittable && frequency.frame.maxY <= app.keyboards.firstMatch.frame.minY + 1
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(app.buttons["frequency-separation-cancel"].isHittable)
        XCTAssertTrue(app.buttons["frequency-separation-apply"].isHittable)
        try shot("frequency-separation"); tap("frequency-separation-cancel")
        func drawer(panel: String?, kind: String? = nil) throws {
            let tool = try XCTUnwrap(phoneObservedTools(in: app)?.first { row in
                guard let control = row["control"] as? [String: Any] else { return false }
                return panel.map { control["panel"] as? String == $0 } ?? (control["kind"] as? String == kind)
            })
            tap("phone-tool-\(try XCTUnwrap(tool["id"] as? Int))")
            XCTAssertTrue(app.descendants(matching: .any)["tool-drawer"].firstMatch.waitForExistence(timeout: 10))
        }
        try drawer(panel: nil, kind: "color"); tap("paint-edit-color")
        XCTAssertTrue(app.buttons["color-cancel"].waitForExistence(timeout: 15)); try shot("color")
        tap("color-swatches"); XCTAssertTrue(app.descendants(matching: .any)["color-sheet"].firstMatch.waitForExistence(timeout: 10))
        let swatchSearch = app.textFields["color-sheet-search"]
        XCTAssertTrue(swatchSearch.waitForExistence(timeout: 10))
        XCTAssertFalse(app.keyboards.firstMatch.exists)
        XCTAssertFalse(app.textFields.matching(NSPredicate(format: "identifier == %@ AND hasKeyboardFocus == YES", swatchSearch.identifier)).firstMatch.exists)
        replaceEditorText(swatchSearch, with: "blue", in: app); swatchSearch.typeText("\n")
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 10))
        XCTAssertEqual(swatchSearch.value as? String, "blue")
        XCTAssertTrue(app.buttons["color-sheet-close"].isHittable)
        let swatches = app.descendants(matching: .any)["color-sheet"].firstMatch.frame
        let footerSwatch = app.descendants(matching: .any)["color-footer-current"].firstMatch
        XCTAssertTrue(footerSwatch.exists)
        XCTAssertEqual(swatches.maxY, footerSwatch.frame.minY - 8 - 1, accuracy: 1)
        XCTAssertGreaterThanOrEqual(swatches.minY, app.buttons["color-back"].frame.maxY)
        try shot("all-swatches"); tap("color-sheet-close"); tap("color-cancel")
        try drawer(panel: "layers")
        let paper = artworkRows(in: app).matching(NSPredicate(format: "label CONTAINS %@", "Paper")).firstMatch
        XCTAssertTrue(paper.waitForExistence(timeout: 10)); workspaceActivate(paper)
        tap("layer-properties-disclosure"); tap("property-texture")
        XCTAssertTrue(app.descendants(matching: .any)["texture-dialog"].firstMatch.waitForExistence(timeout: 10)); try shot("paper"); tap("texture-cancel")
        phoneAlertCommand("Window", label: "Proof", in: app); tap("proof-mode-print")
        XCTAssertTrue(app.descendants(matching: .any)["proof-profile"].firstMatch.waitForExistence(timeout: 15)); try shot("soft-proof")
        let region = try XCTUnwrap(phoneDrawerCanvasRegion(in: app)), window = app.windows.firstMatch
        window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: region.midX - window.frame.minX, dy: region.midY - window.frame.minY)).tap()
        XCTAssertTrue(app.descendants(matching: .any)["tool-drawer"].firstMatch.waitForNonExistence(timeout: 10))
        phoneAlertCommand("File", label: "New…", in: app)
        revealNewDrawingControl("new-drawing-depth", in: app); tap("new-drawing-depth")
        let float = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "new-drawing-depth-option-", "32-bit float HDR")).firstMatch
        XCTAssertTrue(float.waitForExistence(timeout: 10)); workspaceActivate(float)
        expectNewDrawingValue("new-drawing-depth", "32-bit float HDR", in: app)
        createNewDrawing(width: "1200", height: "1600", in: app)
        let drawings = phoneMenuRoot(in: app); revealEditorControl(app.buttons["phone-drawing"], in: drawings); tap("phone-drawing")
        phoneWaitForMenuTitle("Drawings", in: app)
        let selected = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND selected == YES", "phone-menu-row-")).firstMatch
        let closes = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "phone-menu-close-"))
        XCTAssertEqual(closes.count, 2); XCTAssertTrue(selected.exists)
        let selectedID = selected.identifier, selectedLabel = selected.label
        tap("phone-capy")
        phoneAlertCommand("View", label: "Proof SDR", in: app)
        let dial = app.descendants(matching: .any)["proof-dial"].firstMatch
        XCTAssertTrue(dial.waitForExistence(timeout: 15)); XCTAssertTrue(app.buttons["proof-mode-sdr"].isSelected)
        let storage = try XCTUnwrap(app.launchEnvironment["CAPY_STORAGE_DIR"])
        var proof = try XCTUnwrap(phoneDrawingFixtures[storage]); proof["depth"] = "F32"; proof["document_count"] = 2
        proof["selected_drawing"] = selectedLabel; proof.removeValue(forKey: "name")
        phoneDrawingFixtures[storage] = proof; try shot("sdr-rendition")
        let ending = phoneMenuRoot(in: app); revealEditorControl(app.buttons["phone-drawing"], in: ending); tap("phone-drawing")
        phoneWaitForMenuTitle("Drawings", in: app)
        tap(selectedID.replacingOccurrences(of: "phone-menu-row-", with: "phone-menu-close-"))
        XCTAssertFalse(app.buttons["document-close-discard"].exists, "The untouched owned F32 drawing closes without discarding authored changes")
    }

    @MainActor private func checkPhoneDialogs(theme: String) throws {
        let app = try phoneCaptureApplication(theme: theme)
        defer { app.terminate() }
        func any(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }
        func activate(_ id: String) { phonePageActivate(id, in: app) }
        func capture(_ state: String) {
            XCTAssertNoThrow(try phonePageCapture(state, in: app, theme: theme))
        }
        func editsNameOnOpen(_ id: String) {
            let field = app.textFields[id]
            expectation(for: NSPredicate(format: "hasKeyboardFocus == YES"), evaluatedWith: field)
            waitForExpectations(timeout: 10)
            XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
            let before = editorTextMatches(field, "") ? "" : field.value as? String ?? ""
            field.typeText("X")
            XCTAssertEqual(field.value as? String, before + "X", "A name page opened by touch keeps its caret at the end")
        }
        for (application, label, state, closedBy) in [
            ("Image", "Assign Profile…", "assign-profile", "document-color-cancel"),
            ("Image", "Convert Color Space…", "convert-color-space", "document-color-cancel"),
            ("Image", "Change Bit Depth…", "change-bit-depth", "document-color-cancel"),
            ("File", "Document Properties…", "properties", "Done")
        ] {
            phoneAlertCommand(application, label: label, in: app)
            XCTAssertTrue(any(state == "properties" ? "document-properties-appearance" : "document-color-title").waitForExistence(timeout: 30))
            XCTAssertTrue(any(closedBy).isHittable)
            if state == "properties" { capture(state) }
            activate(closedBy)
            XCTAssertTrue(any(closedBy).waitForNonExistence(timeout: 15))
        }
        phoneAlertCommand("File", label: "New…", in: app)
        activate("new-drawing-texture")
        XCTAssertTrue(any("new-drawing-paper-use").waitForExistence(timeout: 15))
        capture("new-paper")
        activate("new-drawing-paper-back")
        activate("new-drawing-paper-color")
        XCTAssertTrue(any("new-drawing-color-use").waitForExistence(timeout: 15))
        capture("new-drawing-footer-paper-color")
        activate("new-drawing-color-back")
        activate("new-drawing-presets-button")
        capture("new-drawing-footer-presets")
        activate("new-drawing-save-as")
        XCTAssertTrue(any("new-drawing-save-confirm").waitForExistence(timeout: 15))
        capture("new-drawing-footer-save-preset")
        activate("new-drawing-browser-back")
        activate("new-drawing-browser-back")
        activate("new-drawing-cancel")

        activate("phone-capy"); activate("phone-preferences")
        XCTAssertTrue(any("settings-done").waitForExistence(timeout: 15))
        assertEditorSettingsRoot(in: app, phone: true)
        capture("preferences-root")
        let pages = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "settings-page-")).allElementsBoundByIndex.map(\.identifier)
        XCTAssertFalse(pages.isEmpty)
        for page in pages {
            activate(page)
            XCTAssertTrue(any("settings-back").waitForExistence(timeout: 10))
            capture(page == "settings-page-shortcuts" ? "preferences-shortcuts-direct" : page.replacingOccurrences(of: "settings-page-", with: "preferences-"))
            if page == "settings-page-color" {
                revealEditorControl(app.buttons["settings-color-profiles"], identifier: "settings-color-profiles", type: .button,
                    in: app.collectionViews.firstMatch)
                activate("settings-color-profiles")
                XCTAssertTrue(any("profile-library-done").waitForExistence(timeout: 15))
                capture("profile-library")
                activate("profile-library-done")
            }
            if page == "settings-page-shortcuts" {
                let categories = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "shortcut-category-")).allElementsBoundByIndex.map(\.identifier)
                for category in categories {
                    activate(category)
                    if category == "shortcut-category-Modifier keys" { capture("preferences-shortcuts-category") }
                    activate("settings-back")
                }
            }
            activate("settings-back")
        }
        activate("settings-done")
        phoneAlertCommand("File", label: "Export…", in: app)
        XCTAssertTrue(any("export-choose-file").waitForExistence(timeout: 60))
        for label in ["Master", "Output"] {
            XCTAssertTrue(app.staticTexts[label].waitForExistence(timeout: 60), "The automatic Export comparison must finish before capture")
        }
        capture("export")
        for page in ["size", "color", "presets"] {
            activate("export-page-" + page)
            XCTAssertTrue(any("export-back").waitForExistence(timeout: 10))
            if page == "presets" {
                XCTAssertFalse(app.buttons["export-preset-update"].exists)
                XCTAssertFalse(app.buttons["export-preset-remove"].exists)
            }
            capture("export-" + page)
            if page == "presets" {
                activate("export-preset-save")
                XCTAssertTrue(any("export-preset-name").waitForExistence(timeout: 10))
                capture("save-export-preset")
                editsNameOnOpen("export-preset-name")
                activate("export-preset-cancel")
            }
            activate("export-back")
        }
        activate("export-cancel")
        phoneAlertCommand("Window", label: "Manage Workspaces…", in: app)
        XCTAssertTrue(any("workspace-library-manager").waitForExistence(timeout: 15))
        XCTAssertTrue(any("workspace-manager-title").waitForExistence(timeout: 10))
        XCTAssertLessThan(any("workspace-manager-title").frame.midY, workspaceViewport(in: app).frame.minY + workspaceViewport(in: app).frame.height / 3)
        capture("workspaces")
        let entries = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-select-")).allElementsBoundByIndex.map(\.identifier)
        XCTAssertFalse(entries.isEmpty)
        for identifier in entries {
            activate(identifier)
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: any(identifier))
            waitForExpectations(timeout: 10)
        }
        activate("workspace-action-new")
        XCTAssertTrue(any("workspace-form-name").waitForExistence(timeout: 10))
        XCTAssertTrue(any("workspace-form-title").waitForExistence(timeout: 10))
        XCTAssertLessThan(any("workspace-form-title").frame.midY, workspaceViewport(in: app).frame.minY + workspaceViewport(in: app).frame.height / 3)
        capture("new-workspace")
        editsNameOnOpen("workspace-form-name")
        activate("workspace-form-cancel")
        activate("workspace-manager-close")
    }
}
