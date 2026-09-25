import XCTest

extension XCTestCase {
    @MainActor func checkZenPreferences(in app: XCUIApplication) {
        let tab = app.buttons["panel-tab-sizes"]
        let capy = app.descendants(matching: .any)["zen-button"].firstMatch
        var current = (show: true, edges: false)
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
        func configure(show: Bool, edges: Bool, theme: String) {
            workspaceActivate(app.buttons["settings-button"])
            XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 10))
            for (id, title, value, saved) in [
                ("zen_show_capy", "Show Capy in Zen mode", show, current.show),
                ("zen_reveal_at_edges", "Reveal panels near screen edges", edges, current.edges),
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
            current = (show, edges)
            attachEditor(in: app, name: "zen-settings-\(theme)-\(show)-\(edges)")
            workspaceActivate(app.buttons["settings-done"])
            XCTAssertTrue(app.buttons["settings-done"].waitForNonExistence(timeout: 5))
        }
        for theme in ["light", "dark"] {
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"hand"}]"#
            app.launch()
            XCTAssertTrue(tab.waitForExistence(timeout: 30))
            for show in [true, false] {
                for edges in [false, true] {
                    configure(show: show, edges: edges, theme: theme)
                    let baseline = tab.frame
                    workspaceActivate(capy)
                    chrome(visible: false, "Zen hides the workspace chrome")
                    XCTAssertEqual(capy.waitForExistence(timeout: 5), show, "Show Capy decides the standalone button")
                    attachEditor(in: app, name: "zen-\(theme)-\(show)-\(edges)")
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
                    let edge = point(size.width / 2, 60), center = point(size.width / 2, size.height / 2)
                    #if os(macOS)
                    edge.hover()
                    if !edges { edge.click() }
                    #else
                    edge.tap()
                    #endif
                    chrome(visible: edges, "Reveal at edges decides whether the top edge shows panels")
                    if edges {
                        #if os(macOS)
                        center.hover()
                        #else
                        center.tap()
                        #endif
                        chrome(visible: false, "Leaving the edge hides the revealed panels")
                    }
                    if show {
                        workspaceActivate(capy)
                    } else {
                        app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [])
                    }
                    chrome(visible: true, "The Capy or Tab exits Zen")
                    XCTAssertEqual(tab.frame, baseline, "Zen leaves the workspace layout unchanged")
                }
            }
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
