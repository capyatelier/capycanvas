import XCTest

extension XCTestCase {
    @MainActor func checkTitleBarToolDrawers(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        workspaceActivate(app.buttons["workspace-switch-builtin:workspace:painter"])
        let toolbar = app.descendants(matching: .any)["toolbar-options-toolbar"].firstMatch
        XCTAssertTrue(toolbar.waitForNonExistence(timeout: 10), "Sketch tools belong in the shared title bar")
        let drawer = app.descendants(matching: .any)["tool-drawer"].firstMatch
        let wheel = app.descendants(matching: .any)["color-wheel"].firstMatch
        #if os(macOS)
        workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.65)).hover()
        let canvasCapture = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        canvasCapture.name = "sketch-canvas-brush-cursor"; canvasCapture.lifetime = .keepAlways; add(canvasCapture)
        #endif
        workspaceActivate(app.buttons["Brush color"])
        XCTAssertTrue(wheel.waitForExistence(timeout: 10))
        XCTAssertTrue(workspaceViewport(in: app).frame.contains(drawer.frame),
            "Drawer \(drawer.frame) must fit the editor \(workspaceViewport(in: app).frame)")
        workspaceActivate(app.buttons["Brush"])
        XCTAssertTrue(wheel.waitForNonExistence(timeout: 10), "Another header tool switches the drawer in one click")
        XCTAssertTrue(drawer.exists)
        workspaceActivate(app.buttons["Brush"])
        XCTAssertTrue(drawer.waitForNonExistence(timeout: 10), "The current opener toggles its drawer closed")
        workspaceActivate(app.buttons["Layers panel"])
        XCTAssertTrue(app.buttons["layer-New layer"].waitForExistence(timeout: 10))
        #if os(macOS)
        let capture = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        #else
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        #endif
        capture.name = "sketch-header-layer-drawer"; capture.lifetime = .keepAlways; add(capture)
        workspaceActivate(app.buttons["Brush color"])
        XCTAssertTrue(wheel.waitForExistence(timeout: 10))
        workspaceActivate(app.buttons["Brush color"])
        XCTAssertTrue(drawer.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkTitleBarSystemStatus(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"customize_workspace_ui"}]"#
        app.launch()
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 15))
            #if os(macOS)
            element.click()
            #else
            element.tap()
            #endif
        }
        let clockItem = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "header-item-", "Clock")).firstMatch
        XCTAssertTrue(clockItem.waitForExistence(timeout: 20), "Clock remains editable even while windowed")
        #if os(macOS)
        clockItem.rightClick()
        #else
        clockItem.press(forDuration: 0.8)
        #endif
        activate(app.buttons["Remove from Title Bar"].firstMatch)
        XCTAssertTrue(clockItem.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.descendants(matching: .any)["header-component-clock"].firstMatch.exists)
        activate(app.buttons["header-cancel"])

        let clock = app.staticTexts["system-clock"]
        #if os(macOS)
        XCTAssertTrue(clock.waitForNonExistence(timeout: 10), "Windowed native chrome owns system status")
        activate(app.menuBars.menuBarItems["View"])
        activate(app.menuItems["Enter Full Screen"])
        #endif
        XCTAssertTrue(clock.waitForExistence(timeout: 15), "Cancel restores the fullscreen Clock item")
        let title = editorDocumentTitle(in: app)
        XCTAssertTrue(title.exists)
        XCTAssertLessThanOrEqual(abs(clock.frame.midY - title.frame.midY), 1)
        #if os(macOS)
        let screenshot = app.windows.firstMatch.screenshot()
        #else
        let screenshot = XCUIScreen.main.screenshot()
        #endif
        let capture = XCTAttachment(screenshot: screenshot)
        capture.name = "header-system-status"; capture.lifetime = .keepAlways; add(capture)
        #if os(macOS)
        activate(app.menuBars.menuBarItems["View"])
        activate(app.menuItems["Exit Full Screen"])
        XCTAssertTrue(clock.waitForNonExistence(timeout: 15))
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}


extension XCTestCase {
    @MainActor func checkTitleBarCustomization(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"customize_workspace_ui"}]"#
        app.launch()
        func item(_ label: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "header-item-", label)).firstMatch
        }
        func component(_ kind: String) -> XCUIElement { app.descendants(matching: .any)["header-component-" + kind].firstMatch }
        func drag(_ source: XCUIElement, to destination: XCUICoordinate) {
            XCTAssertTrue(source.waitForExistence(timeout: 15))
            source.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5))
                .press(forDuration: 0.01, thenDragTo: destination)
        }
        let title = item("Document Title")
        XCTAssertTrue(title.waitForExistence(timeout: 20))
        workspaceActivate(component("space"))
        XCTAssertFalse(item("Space").exists, "A bank click is inert")
        drag(component("space"), to: title.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)))
        XCTAssertTrue(item("Space").waitForExistence(timeout: 10))
        let editorBeforeDrag = workspaceViewport(in: app).frame
        drag(item("Space"), to: workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.4)))
        XCTAssertEqual(workspaceViewport(in: app).frame, editorBeforeDrag, "Editing the title bar must not move or resize the OS window")
        XCTAssertTrue(item("Space").waitForNonExistence(timeout: 10), "Outside release removes the item")
        let target = title.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5))
        drag(component("tools"), to: target)
        let search = app.textFields["tool-picker-search"]
        XCTAssertTrue(search.waitForExistence(timeout: 15), "The dropped bank chip opens the existing picker")
        workspaceActivate(search); search.typeText("Undo")
        workspaceActivate(app.buttons["tool-choice-Undo"])
        workspaceActivate(search); search.typeKey("a", modifierFlags: .command); search.typeText("Redo")
        workspaceActivate(app.buttons["tool-choice-Redo"])
        workspaceActivate(app.buttons["tool-picker-confirm"])
        XCTAssertTrue(item("Undo").waitForExistence(timeout: 10))
        XCTAssertTrue(item("Redo").exists)
        XCTAssertLessThan(item("Undo").frame.minX, item("Redo").frame.minX, "Filtering retains selection order")
        #if os(iOS)
        drag(component("menu"), to: title.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)))
        XCTAssertTrue(item("Main Menu").waitForExistence(timeout: 10))
        #endif
        workspaceActivate(app.buttons["header-done"])
        XCTAssertTrue(app.buttons["header-done"].waitForNonExistence(timeout: 10))
        let documentTitle = editorDocumentTitle(in: app)
        #if os(macOS)
        documentTitle.rightClick()
        let customize = app.menuItems.matching(identifier: "invokeMenuItem:")
            .matching(identifier: "Customize Title Bar…").firstMatch
        XCTAssertTrue(customize.waitForExistence(timeout: 10))
        customize.click()
        #else
        documentTitle.press(forDuration: 0.8)
        workspaceActivate(app.buttons["Customize Title Bar…"])
        #endif
        XCTAssertTrue(item("Undo").waitForExistence(timeout: 10), "Done retains the accepted layout")
        drag(component("tools"), to: item("Document Title").coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)))
        XCTAssertTrue(search.waitForExistence(timeout: 10))
        #if os(macOS)
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        #else
        // XCTest's Escape injection is not a physical keyboard acceptance gate.
        workspaceActivate(app.descendants(matching: .any)["tool-picker"].firstMatch.buttons["Cancel"])
        #endif
        XCTAssertTrue(search.waitForNonExistence(timeout: 10))
        XCTAssertTrue(app.buttons["header-done"].exists, "Dismissing the picker returns to title-bar editing")
        workspaceActivate(app.buttons["header-cancel"])
        #if os(iOS)
        workspaceActivate(app.buttons.matching(NSPredicate(format: "label == %@", "Main Menu")).firstMatch)
        let menu = app.descendants(matching: .any)["application-menu-content"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        workspaceActivate(menu.buttons["File"])
        XCTAssertTrue(menu.buttons["Recovered Drawings…"].waitForExistence(timeout: 10))
        workspaceActivate(menu.buttons["editor-menu-back"])
        workspaceActivate(menu.buttons["Window"])
        workspaceActivate(menu.buttons["Customize Title Bar…"])
        XCTAssertTrue(app.buttons["header-done"].waitForExistence(timeout: 10), "The shared Main Menu enters title-bar editing")
        workspaceActivate(app.buttons["header-cancel"])
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
