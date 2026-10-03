import XCTest

extension XCTestCase {
    @MainActor func checkLiveInterfaceLanguage(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"settings"},{"type":"preferences","action":{"type":"page","page":"appearance"}}]"#
        app.launch()
        let done = app.buttons["settings-done"], layers = app.buttons["panel-tab-layers"]
        #if os(macOS)
        let language = app.popUpButtons["preference-language"]
        #else
        let language = app.buttons["preference-language"]
        #endif
        func expectLabel(_ element: XCUIElement, _ label: String) {
            expectation(for: NSPredicate(format: "label == %@", label), evaluatedWith: element); waitForExpectations(timeout: 15)
        }
        func choose(_ label: String) {
            XCTAssertTrue(language.waitForExistence(timeout: 20))
            workspaceActivate(language)
            #if os(macOS)
            let option = language.menuItems[label].firstMatch
            XCTAssertTrue(option.waitForExistence(timeout: 5)); option.click()
            #else
            let option = app.buttons[label].firstMatch
            XCTAssertTrue(option.waitForExistence(timeout: 5)); workspaceActivate(option)
            #endif
        }
        choose("日本語")
        expectLabel(done, "完了")
        attachEditor(in: app, name: "language-settings-ja-" + theme)
        workspaceActivate(done)
        XCTAssertTrue(done.waitForNonExistence(timeout: 10))
        expectLabel(layers, "レイヤー")
        attachEditor(in: app, name: "language-editor-ja-" + theme)
        workspaceActivate(app.buttons["settings-button"])
        choose("English")
        expectLabel(done, "Done")
        workspaceActivate(done)
        expectLabel(layers, "Layers")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
