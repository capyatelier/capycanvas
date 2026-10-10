import XCTest

extension XCTestCase {
    @MainActor func checkZenPreferences(in app: XCUIApplication) {
        var tab = app.descendants(matching: .any)["workspace-switcher"].firstMatch
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
            workspaceActivate(workspaceViewport(in: app))
            app.typeKey("k", modifierFlags: .command)
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
        for workspace in ["painter", "illustrator", "photographer"] {
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:\#(workspace)"}}]"#
            app.launch()
            let choice = app.buttons["workspace-switch-builtin:workspace:" + workspace]
            let compact = app.descendants(matching: .any)["workspace-switcher-menu"].firstMatch
            let title = ["painter": "Sketch", "illustrator": "Paint", "photographer": "Photo"][workspace]!
            expectation(for: NSPredicate { _, _ in
                choice.exists && choice.isSelected || compact.exists && compact.label.contains(title)
            }, evaluatedWith: app)
            waitForExpectations(timeout: 30)
            app.terminate()
            for theme in ["light", "dark"] {
                for headerSize in ["small", "medium", "large"] {
                    app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"restore_settings","settings":{}},{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"hand"},{"type":"customize","action":{"type":"header","action":{"type":"set_size","size":"\#(headerSize)"}}}]"#
                    app.launch()
                    current = true
                    func headerAnchor() -> XCUIElement? {
                        let candidates = [app.descendants(matching: .any)["settings-button"].firstMatch,
                            app.descendants(matching: .any)["workspace-switcher"].firstMatch] +
                            app.descendants(matching: .any).matching(NSPredicate(format:
                                "identifier BEGINSWITH %@ OR identifier BEGINSWITH %@", "header-tool-", "header-overflow-")).allElementsBoundByIndex
                        return candidates.first { $0.exists && $0.frame.width > 0 && workspaceViewport(in: app).frame.contains($0.frame) }
                    }
                    expectation(for: NSPredicate { _, _ in headerAnchor() != nil }, evaluatedWith: app)
                    waitForExpectations(timeout: 30)
                    tab = headerAnchor()!
                    for show in [true, false] {
                        configure(show: show, theme: theme)
                        let baseline = tab.frame, normalCapyBounds = capy.frame
                        XCTAssertEqual(normalCapyBounds.width, CGFloat(["small": 36.0, "medium": 48.0, "large": 60.0][headerSize]!), accuracy: 0.5, "The requested native header size is active")
                        workspaceActivate(capy)
                        chrome(visible: false, "Zen hides the workspace chrome")
                        XCTAssertEqual(capy.waitForExistence(timeout: 5), show, "Show Capy decides the standalone button")
                        if show { XCTAssertEqual(capy.frame, normalCapyBounds, "Zen keeps the \(workspace)/\(headerSize) Capy bounds") }
                        attachEditor(in: app, name: "zen-\(workspace)-\(theme)-\(headerSize)-\(show)")
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
                        XCTAssertEqual(capy.frame, normalCapyBounds, "Leaving Zen restores the Capy bounds")
                    }
                }
            }
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
