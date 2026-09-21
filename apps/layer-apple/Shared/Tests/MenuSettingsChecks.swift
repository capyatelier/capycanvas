import XCTest
#if os(macOS)
import AppKit
#endif

extension XCTestCase {
    @MainActor func checkSettingsControls(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"page","page":"appearance"}}]"#
        app.launch()
        func page(_ id: String) {
            let item = app.staticTexts["settings-page-" + id]
            XCTAssertTrue(item.waitForExistence(timeout: 10)); workspaceActivate(item)
        }
        func reopen(_ id: String) {
            workspaceActivate(app.buttons["settings-done"])
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 5))
            workspaceActivate(app.buttons["settings-button"])
            XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 10)); page(id)
        }
        func choice(_ id: String) -> XCUIElement {
            #if os(macOS)
            return app.popUpButtons["preference-" + id]
            #else
            return app.buttons["preference-" + id]
            #endif
        }
        func selected(_ id: String, _ label: String) {
            expectation(for: NSPredicate(format: "value == %@ OR label ENDSWITH %@", label, label), evaluatedWith: choice(id))
            waitForExpectations(timeout: 5)
        }
        func choose(_ id: String, _ label: String) {
            let control = choice(id)
            XCTAssertTrue(control.waitForExistence(timeout: 20)); XCTAssertTrue(control.isEnabled)
            workspaceActivate(control)
            #if os(macOS)
            let option = app.menuItems[label].firstMatch
            #else
            let option = app.buttons[label].firstMatch
            #endif
            XCTAssertTrue(option.waitForExistence(timeout: 5)); workspaceActivate(option)
            selected(id, label)
        }
        choose("theme", "Dark")
        attachEditor(in: app, name: "settings-dropdown-dark")
        choose("theme", "System")
        choose("theme", "Light")
        reopen("appearance"); selected("theme", "Light")

        page("input")
        for label in ["Outline and crosshair", "Crosshair", "No cursor", "Brush outline", "Dot"] {
            choose("cursor", label)
        }
        reopen("input"); selected("cursor", "Dot")
        attachEditor(in: app, name: "settings-dropdown-cursor")

        #if os(macOS)
        func enabled(_ element: XCUIElement, _ value: Bool) {
            expectation(for: NSPredicate(format: "enabled == %@", NSNumber(value: value)), evaluatedWith: element)
            waitForExpectations(timeout: 5)
        }
        func active(_ element: XCUIElement, _ value: Bool) {
            expectation(for: NSPredicate(format: "value == %@", NSNumber(value: value)), evaluatedWith: element)
            waitForExpectations(timeout: 5)
        }
        page("input")
        // Grouped macOS forms expose switch labels as separate static text.
        let master = app.switches.element(boundBy: 0)
        let native = app.switches.element(boundBy: 1)
        let amount = app.buttons["number-value-Prediction amount"]
        XCTAssertTrue(master.waitForExistence(timeout: 10)); XCTAssertTrue(native.exists)
        XCTAssertEqual(app.switches.count, 2)
        active(master, true); active(native, false); enabled(native, false)
        XCTAssertTrue(amount.exists); enabled(amount, true)
        workspaceActivate(master); active(master, false); enabled(native, false); enabled(amount, false)
        workspaceActivate(master); active(master, true); enabled(amount, true)
        let originalAmount = amount.value as? String ?? ""
        XCTAssertFalse(originalAmount.isEmpty)
        workspaceActivate(app.buttons["number-increase-Prediction amount"])
        expectation(for: NSPredicate(format: "value != %@", originalAmount), evaluatedWith: amount)
        waitForExpectations(timeout: 5)
        let editedAmount = amount.value as? String
        reopen("input"); active(master, true); active(native, false); enabled(native, false)
        XCTAssertEqual(amount.value as? String, editedAmount, "Done/reopen must retain the manual amount")
        attachEditor(in: app, name: "settings-prediction-dependencies")
        #endif
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkSettingsTextState(in app: XCUIApplication, keyboardSelection: Bool = true) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"page","page":"appearance"}}]"#
        app.launch()
        func reopen() {
            workspaceActivate(app.buttons["settings-done"])
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 5))
            workspaceActivate(app.buttons["settings-button"])
            XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 10))
        }
        for label in ["Dark theme base color", "Light theme base color"] {
            let field = app.textFields[label]
            XCTAssertTrue(field.waitForExistence(timeout: 20))
            let defaultValue = field.value as? String ?? ""
            XCTAssertTrue(defaultValue.hasPrefix("#")); XCTAssertEqual(defaultValue.count, 7)
            func replace(_ value: String) {
                workspaceActivate(field)
                #if os(macOS)
                field.typeKey("a", modifierFlags: .command)
                #else
                if keyboardSelection { field.typeKey("a", modifierFlags: .command) }
                else { field.tap(withNumberOfTaps: 3, numberOfTouches: 1) }
                #endif
                field.typeText(value)
            }
            for draft in ["#abcdef", "invalid"] {
                replace("#12ab3489")
                expectation(for: NSPredicate(format: "value == %@", "#12ab34"), evaluatedWith: field)
                waitForExpectations(timeout: 5)
                reopen()
                XCTAssertEqual(field.value as? String, "#12ab34", "Done must commit the limited native draft")
                replace(draft)
                XCTAssertEqual(field.value as? String, draft)
                let title = app.staticTexts[label].firstMatch
                #if os(macOS)
                title.rightClick()
                let reset = app.menuItems["Reset to Default"]
                #else
                title.press(forDuration: 0.7)
                let reset = app.buttons["Reset to Default"]
                #endif
                XCTAssertTrue(reset.waitForExistence(timeout: 5)); XCTAssertTrue(reset.isEnabled)
                workspaceActivate(reset)
                expectation(for: NSPredicate(format: "value == %@", defaultValue), evaluatedWith: field)
                waitForExpectations(timeout: 5)
                reopen()
                XCTAssertEqual(field.value as? String, defaultValue,
                    "Done/reopen must retain Reset instead of restoring the focused draft")
            }
        }
        attachEditor(in: app, name: "settings-text-state")
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkSettingsNumericReset(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"reveal","id":"pressure"}}]"#
        app.launch()
        let value = app.buttons["number-value-Pressure response"]
        let entry = app.textFields["number-entry-Pressure response"]
        XCTAssertTrue(value.waitForExistence(timeout: 20))
        let defaultDisplay = value.value as? String ?? ""
        XCTAssertFalse(defaultDisplay.isEmpty)
        for draft in ["", "2 + 0.25", "2 + ("] {
            workspaceActivate(app.buttons["number-increase-Pressure response"])
            expectation(for: NSPredicate(format: "value != %@", defaultDisplay), evaluatedWith: value)
            waitForExpectations(timeout: 5)
            if !draft.isEmpty {
                workspaceActivate(value)
                XCTAssertTrue(entry.waitForExistence(timeout: 5))
                entry.typeText(draft)
                XCTAssertEqual(entry.value as? String, draft)
            }
            let label = app.staticTexts["Pressure response"].firstMatch
            #if os(macOS)
            label.rightClick()
            let reset = app.menuItems["Reset to Default"]
            #else
            label.press(forDuration: 0.7)
            let reset = app.buttons["Reset to Default"]
            #endif
            XCTAssertTrue(reset.waitForExistence(timeout: 5)); XCTAssertTrue(reset.isEnabled)
            workspaceActivate(reset)
            XCTAssertFalse(app.staticTexts["number-error-Pressure response"].exists,
                "Reset must discard an invalid draft's error")
            if !draft.isEmpty {
                XCTAssertNotEqual(entry.exists ? entry.value as? String : nil, draft,
                    "Reset must replace the unfinished expression")
            }
            workspaceActivate(app.buttons["settings-done"])
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 5))
            workspaceActivate(app.buttons["settings-button"])
            workspaceActivate(app.staticTexts["settings-page-input"])
            XCTAssertTrue(value.waitForExistence(timeout: 10))
            XCTAssertEqual(value.value as? String, defaultDisplay,
                "Done/reopen must retain Reset instead of restoring the discarded draft")
        }
        attachEditor(in: app, name: "settings-numeric-reset")
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkSettingsChoicePresentation(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"reveal","id":"zen_icon"}}]"#
        app.launch()
        let first = app.buttons["preference-zen_icon-0"]
        XCTAssertTrue(first.waitForExistence(timeout: 20))
        XCTAssertTrue(first.isSelected)
        for index in 1...3 {
            let choice = app.buttons["preference-zen_icon-\(index)"]
            workspaceActivate(choice)
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: choice)
            waitForExpectations(timeout: 5)
            XCTAssertFalse(first.isSelected)
        }
        attachEditor(in: app, name: "settings-image-choices")
        let selected = app.buttons["preference-zen_icon-3"]
        #if os(macOS)
        selected.rightClick()
        let reset = app.menuItems["Reset to Default"]
        #else
        selected.press(forDuration: 0.7)
        let reset = app.buttons["Reset to Default"]
        #endif
        XCTAssertTrue(reset.waitForExistence(timeout: 5)); workspaceActivate(reset)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: first)
        waitForExpectations(timeout: 5)
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertTrue(first.waitForNonExistence(timeout: 5))
        workspaceActivate(app.buttons["settings-button"])
        XCTAssertTrue(first.waitForExistence(timeout: 10)); XCTAssertTrue(first.isSelected)

        workspaceActivate(app.staticTexts["settings-page-shortcuts"])
        let search = app.textFields["shortcut-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 5)); workspaceActivate(search); search.typeText("Zen")
        let shortcut = app.buttons["shortcut-command.ZenMode"]
        XCTAssertTrue(shortcut.waitForExistence(timeout: 5)); workspaceActivate(shortcut)
        let add = app.buttons["shortcut-add"]
        XCTAssertTrue(add.waitForExistence(timeout: 5)); workspaceActivate(add)
        let cancel = app.buttons["shortcut-cancel"]
        XCTAssertTrue(cancel.waitForExistence(timeout: 5))
        attachEditor(in: app, name: "settings-shortcut-recording")
        workspaceActivate(cancel)
        XCTAssertTrue(cancel.waitForNonExistence(timeout: 5))
        workspaceActivate(app.buttons["shortcut-editor-done"])
        XCTAssertTrue(add.waitForNonExistence(timeout: 5))
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertTrue(search.waitForNonExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkNumericSettingsDone(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"page","page":"input"}}]"#
        app.launch()
        let value = app.buttons["number-value-Pressure response"]
        let entry = app.textFields["number-entry-Pressure response"]
        XCTAssertTrue(value.waitForExistence(timeout: 20)); workspaceActivate(value)
        XCTAssertTrue(entry.waitForExistence(timeout: 5)); entry.typeText("1 + 0.25")
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertTrue(entry.waitForNonExistence(timeout: 5))
        workspaceActivate(app.buttons["settings-button"])
        let search = app.textFields["settings-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 10)); workspaceActivate(search); search.typeText("Pressure response")
        #if os(iOS)
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        #endif
        let result = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Pressure response")).firstMatch
        XCTAssertTrue(result.waitForExistence(timeout: 5)); workspaceActivate(result)
        #if os(iOS)
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 5), "Opening a search result must dismiss the keyboard")
        #endif
        XCTAssertTrue(value.waitForExistence(timeout: 5), "Opening a search result must reveal its setting")
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", "1.25"), evaluatedWith: value)
        waitForExpectations(timeout: 5)
        attachEditor(in: app, name: "settings-numeric-done")
        workspaceActivate(search); search.typeText("theme")
        #if os(iOS)
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        #endif
        let canvasPage = app.staticTexts["settings-page-canvas"]
        XCTAssertTrue(canvasPage.waitForExistence(timeout: 5)); workspaceActivate(canvasPage)
        XCTAssertTrue(app.buttons["number-value-Scroll pan speed"].waitForExistence(timeout: 5))
        #if os(iOS)
        XCTAssertTrue(app.keyboards.firstMatch.waitForNonExistence(timeout: 5), "Opening a sidebar page must dismiss the keyboard")
        #endif
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkEditorKeyboardFocus(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        func selected(_ label: String) {
            let tool = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-toolbar-", label)).firstMatch
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: tool)
            waitForExpectations(timeout: 5)
        }
        func hand() { app.typeKey("h", modifierFlags: []); selected("Hand") }
        editorTool("Eraser", in: app); selected("Eraser"); hand()
        editorTool("Pen", in: app); selected("Pen")
        let value = app.buttons["number-value-tool-size"], entry = app.textFields["number-entry-tool-size"]
        revealEditorControl(value, in: app.scrollViews.containing(.button, identifier: value.identifier).firstMatch)
        workspaceActivate(value); XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeText("96\n")
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", "96"), evaluatedWith: value)
        waitForExpectations(timeout: 5); hand()

        #if os(macOS)
        editorTool("Pen", in: app); selected("Pen")
        workspaceActivate(value); XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeText("123e")
        selected("Pen")
        XCTAssertTrue((entry.value as? String ?? "").contains("e"), "Typing a tool shortcut in a field must edit text")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(entry.waitForNonExistence(timeout: 5))
        expectation(for: NSPredicate(format: "value BEGINSWITH %@", "96"), evaluatedWith: value)
        waitForExpectations(timeout: 5); hand()
        #endif

        workspaceActivate(app.buttons["settings-button"])
        let search = app.textFields["settings-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 5)); workspaceActivate(search); search.typeText("brush")
        expectation(for: NSPredicate(format: "value == %@", "brush"), evaluatedWith: search)
        waitForExpectations(timeout: 5)
        workspaceActivate(app.buttons["settings-done"])
        XCTAssertTrue(search.waitForNonExistence(timeout: 5))
        app.typeKey("e", modifierFlags: []); selected("Eraser")
        hand()
        XCTAssertEqual(viewport.frame, originalFrame)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "keyboard-after-controls-and-settings")
    }

    @MainActor func checkNumericTextHistory(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        let paper = editorPixels(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        expectation(for: NSPredicate { _, _ in
            let sample = self.editorPixels(in: app); return Int(sample[2]) > Int(sample[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let painted = editorPixels(in: app)
        let value = app.buttons["number-value-tool-size"], entry = app.textFields["number-entry-tool-size"]
        revealEditorControl(value, in: app.scrollViews.containing(.button, identifier: value.identifier).firstMatch)
        workspaceActivate(value); XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeText("123")
        app.typeKey("a", modifierFlags: .command)
        attachEditor(in: app, name: "keyboard-select-all-text")
        #if os(iOS)
        app.typeText("96")
        #else
        entry.typeText("96")
        #endif
        XCTAssertEqual(entry.value as? String, "96", "Command-A must select the native field's text")
        app.typeKey("z", modifierFlags: .command)
        attachEditor(in: app, name: "keyboard-focused-text-undo")
        expectation(for: NSPredicate(format: "value != %@", "96"), evaluatedWith: entry)
        waitForExpectations(timeout: 5)
        XCTAssertEqual(editorPixels(in: app), painted, "Text Undo must preserve artwork")
        app.typeKey("z", modifierFlags: [.command, .shift])
        expectation(for: NSPredicate(format: "value == %@", "96"), evaluatedWith: entry)
        waitForExpectations(timeout: 5)
        XCTAssertEqual(editorPixels(in: app), painted, "Text Redo must preserve artwork")
        #if os(macOS)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        #else
        app.typeText("\n")
        #endif
        XCTAssertTrue(entry.waitForNonExistence(timeout: 5))
        // The same shortcuts return to artwork when text editing has ended.
        let history: [(XCUIElement.KeyModifierFlags, Data)] = [(.command, paper), ([.command, .shift], painted)]
        for (flags, expected) in history {
            app.typeKey("z", modifierFlags: flags)
            expectation(for: NSPredicate { _, _ in self.editorPixels(in: app) == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        attachEditor(in: app, name: "keyboard-text-and-artwork-history")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkApplicationLinkHandoff(in app: XCUIApplication) {
        #if os(macOS)
        guard let application = NSWorkspace.shared.urlForApplication(toOpen: URL(string: "https://capycanvas.art/")!),
            let identifier = Bundle(url: application)?.bundleIdentifier else {
            XCTFail("A browser is required for the public-link workflow"); return
        }
        let browser = XCUIApplication(bundleIdentifier: identifier)
        #else
        let browser = XCUIApplication(bundleIdentifier: "com.apple.mobilesafari")
        #endif
        app.launch()
        XCTAssertTrue(app.buttons["settings-button"].waitForExistence(timeout: 30))
        func open(_ link: XCUIElement, url: String) {
            workspaceActivate(link)
            expectation(for: NSPredicate { _, _ in browser.state == .runningForeground }, evaluatedWith: browser)
            waitForExpectations(timeout: 30)
            // Focus the native address field to read the full destination,
            // including the source repository path hidden by compact URL bars.
            #if os(macOS)
            browser.typeKey("l", modifierFlags: .command)
            #else
            let location = browser.textFields.matching(NSPredicate(format: "label == %@", "Address")).firstMatch
            XCTAssertTrue(location.waitForExistence(timeout: 10))
            location.tap()
            #endif
            // Safari omits the scheme and prefixes the displayed address with
            // a direction mark even while editing it.
            let address = browser.textFields.matching(NSPredicate(format: "value CONTAINS %@",
                url.replacingOccurrences(of: "https://", with: ""))).firstMatch
            XCTAssertTrue(address.waitForExistence(timeout: 10), "The browser must receive \(url)")
            app.activate()
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        }
        let links = [("website", "Website", "https://capycanvas.art"),
            ("source_code", "Source code", "https://github.com/capyatelier/capycanvas")]
        for (id, label, url) in links {
            #if os(macOS)
            workspaceActivate(app.menuBars.menuBarItems["Help"])
            open(app.menuItems[label], url: url)
            #else
            workspaceActivate(app.buttons["menu-Help"])
            open(app.buttons["command-" + id], url: url)
            #endif
        }
        #if os(macOS)
        workspaceActivate(app.menuBars.menuBarItems["Capy Canvas"])
        workspaceActivate(app.menuItems["About Capy Canvas"])
        #else
        workspaceActivate(app.buttons["menu-Help"])
        workspaceActivate(app.buttons["command-about"])
        #endif
        for (id, _, url) in links {
            open(app.descendants(matching: .any)["preference-" + id].firstMatch, url: url)
            XCTAssertTrue(app.buttons["settings-done"].exists, "Returning from a link must retain About")
        }
        workspaceActivate(app.buttons["settings-done"])
    }

    @MainActor func checkAboutAndApplicationMenus(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        #if os(macOS)
        let applicationMenu = app.menuBars.menuBarItems["Capy Canvas"]
        XCTAssertTrue(applicationMenu.exists, "The OS menu must use the public application name")
        workspaceActivate(applicationMenu)
        workspaceActivate(app.menuItems["About Capy Canvas"])
        #else
        workspaceActivate(app.buttons["menu-Help"])
        workspaceActivate(app.buttons["command-about"])
        #endif

        let done = app.buttons["settings-done"]
        XCTAssertTrue(done.waitForExistence(timeout: 10))
        let hierarchy = XCTAttachment(string: app.debugDescription)
        hierarchy.name = "about-accessibility"; hierarchy.lifetime = .keepAlways; add(hierarchy)
        for text in ["Capy Canvas", "Website", "Source code"] {
            XCTAssertTrue(app.descendants(matching: .any).matching(NSPredicate(format: "label == %@ OR value == %@", text, text))
                .firstMatch.exists, "About must show \(text)")
        }
        for (label, value) in [("Version", "0.1.0"), ("Application license", "MIT OR Apache-2.0"),
            ("Canvas rendering", "Native GPU")] {
            XCTAssertTrue(app.staticTexts[label].firstMatch.exists)
            let content = app.descendants(matching: .any).matching(NSPredicate(format:
                "label == %@ OR label == %@ OR value == %@", label + ", " + value, value, value)).firstMatch
            XCTAssertTrue(content.exists, "About \(label) must show \(value)")
        }
        for (id, label) in [("website", "capycanvas.art"), ("source_code", "github.com/capyatelier/capycanvas")] {
            let link = app.descendants(matching: .any)["preference-" + id].firstMatch
            XCTAssertTrue(link.exists && link.isHittable, "About must expose the \(id) link")
            XCTAssertTrue(link.label.hasSuffix(label), "The native link label must retain its destination")
        }
        #if os(macOS)
        let screenshot = app.windows.firstMatch.screenshot()
        #else
        let screenshot = XCUIScreen.main.screenshot()
        #endif
        let capture = XCTAttachment(screenshot: screenshot)
        capture.name = "about-application-information"; capture.lifetime = .keepAlways; add(capture)

        // The ordinary settings search must still locate the source-code row.
        let search = app.textFields["settings-search"]
        workspaceActivate(search); search.typeText("github")
        let result = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Source code")).firstMatch
        XCTAssertTrue(result.waitForExistence(timeout: 5)); workspaceActivate(result)
        XCTAssertTrue(app.descendants(matching: .any)["preference-source_code"].firstMatch.waitForExistence(timeout: 5))
        workspaceActivate(done)
        XCTAssertTrue(done.waitForNonExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkShortcutConflictAndEditorEffect(in app: XCUIApplication) {
        func activate(_ item: XCUIElement) {
            XCTAssertTrue(item.waitForExistence(timeout: 15))
            #if os(macOS)
            item.click()
            #else
            item.tap()
            #endif
        }
        let search = app.textFields["shortcut-search"]
        activate(search); search.typeText("Zen")
        expectation(for: NSPredicate(format: "value == %@", "Zen"), evaluatedWith: search)
        waitForExpectations(timeout: 10)
        activate(app.buttons["shortcut-command.ZenMode"])
        activate(app.buttons["shortcut-add"])
        XCTAssertTrue(app.staticTexts["shortcut-captured"].waitForExistence(timeout: 10))
        // Capture must intercept an existing accelerator before it invokes Undo.
        app.typeKey("z", modifierFlags: .command)
        let confirm = app.buttons["shortcut-confirm"]
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: confirm)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(confirm.label == "Replace" || confirm.value as? String == "Replace")
        activate(confirm)
        XCTAssertTrue(confirm.waitForNonExistence(timeout: 10))
        activate(app.buttons["shortcut-editor-done"])
        activate(app.buttons["settings-done"])
        let title = editorDocumentTitle(in: app)
        XCTAssertTrue(title.waitForExistence(timeout: 10))
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(title.waitForNonExistence(timeout: 10), "The new shortcut must execute Zen after the editor closes")
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(title.waitForExistence(timeout: 10))
    }
}
