import XCTest

extension XCTestCase {
    @MainActor func checkZenPreferences(in app: XCUIApplication) {
        let tab = app.buttons["panel-tab-sizes"]
        let capy = app.descendants(matching: .any)["zen-button"].firstMatch
        var current = true
        func isOn(_ element: XCUIElement) -> Bool {
            (element.value as? NSNumber)?.boolValue ?? (element.value as? String == "1")
        }
        func toggle(_ element: XCUIElement) {
            #if os(macOS)
            workspaceActivate(element)
            #else
            element.coordinate(withNormalizedOffset: CGVector(dx: 0.93, dy: 0.5)).tap()
            #endif
        }
        func point(_ x: CGFloat, _ y: CGFloat) -> XCUICoordinate {
            workspaceViewport(in: app).coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: x, dy: y))
        }
        func chrome(visible: Bool, _ message: String) {
            if !visible { _ = tab.waitForExistence(timeout: 1) }
            expectation(for: NSPredicate(format: "exists == %@", NSNumber(value: visible)), evaluatedWith: tab)
            waitForExpectations(timeout: 10)
            XCTAssertEqual(tab.exists, visible, message)
        }
        func configure(show: Bool, theme: String) {
            workspaceActivate(app.buttons["settings-button"])
            XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 10))
            for (id, title, value, saved) in [
                ("zen_show_capy", "Show Capy in Zen mode", show, current),
            ] {
                let search = app.textFields["settings-search"]
                workspaceActivate(search)
                search.typeKey("a", modifierFlags: .command)
                search.typeText(title)
                workspaceActivate(app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch)
                let control = app.switches["preference-" + id]
                XCTAssertTrue(control.waitForExistence(timeout: 10), "Search reveals the \(title) switch")
                XCTAssertEqual(isOn(control), saved, "\(title) keeps its saved value")
                if saved != value {
                    toggle(control)
                    expectation(for: NSPredicate { _, _ in isOn(control) == value }, evaluatedWith: control)
                    waitForExpectations(timeout: 5)
                }
            }
            current = show
            attachEditor(in: app, name: "zen-settings-\(theme)-\(show)")
            workspaceActivate(app.buttons["settings-done"])
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 5))
        }
        for theme in ["light", "dark"] {
            for headerSize in ["small", "medium", "large"] {
                app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"restore_settings","settings":{}},{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"hand"},{"type":"customize","action":{"type":"header","action":{"type":"set_size","size":"\#(headerSize)"}}}]"#
                app.launch()
                current = true
                XCTAssertTrue(tab.waitForExistence(timeout: 30))
                for show in [true, false] {
                    configure(show: show, theme: theme)
                    let baseline = tab.frame, normalCapySize = capy.frame.size
                    workspaceActivate(capy)
                    chrome(visible: false, "Zen hides the workspace chrome")
                    XCTAssertEqual(capy.waitForExistence(timeout: 5), show, "Show Capy decides the standalone button")
                    if show { XCTAssertEqual(capy.frame.size, normalCapySize, "Zen keeps the \(headerSize) Capy button size") }
                    attachEditor(in: app, name: "zen-\(theme)-\(headerSize)-\(show)")
                    #if os(macOS)
                    if show {
                        let controls = app.windows.firstMatch.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "_XCUI:"))
                        XCTAssertGreaterThanOrEqual(controls.count, 3)
                        for control in controls.allElementsBoundByIndex {
                            XCTAssertFalse(control.frame.intersects(capy.frame), "Native window controls must clear the standalone Capy")
                        }
                    }
                    #endif
                    let size = workspaceViewport(in: app).frame.size
                    let edge = point(size.width / 2, 6)
                    #if os(macOS)
                    edge.hover(); edge.click()
                    #else
                    edge.tap()
                    #endif
                    chrome(visible: false, "Edge contact keeps panels hidden")
                    if show { workspaceActivate(capy) }
                    else { app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: []) }
                    chrome(visible: true, "The Capy or Tab exits Zen")
                    XCTAssertEqual(tab.frame, baseline, "Zen leaves the workspace layout unchanged")
                }
            }
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
