import XCTest

extension XCTestCase {
    @MainActor func checkTitleBarToolDrawers(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:painter"}}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 60)
        let sketch = app.buttons["workspace-switch-builtin:workspace:painter"]
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: sketch)
        waitForExpectations(timeout: 30)
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
        let pencil = app.buttons["brush_sets-Pencil"]
        XCTAssertTrue(pencil.waitForExistence(timeout: 10))
        workspaceActivate(pencil)
        workspaceActivate(app.buttons["brush-2"])
        let size = app.buttons["number-value-tool-size"]
        workspaceActivate(size)
        app.textFields["number-entry-tool-size"].typeText("37\n")
        expectation(for: NSPredicate(format: "value == %@", "37.0 px"), evaluatedWith: size)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "brush-three-column-drawer")
        workspaceActivate(app.buttons["Sculpt"])
        workspaceActivate(app.buttons["sculpt_sets-Liquify"])
        XCTAssertTrue(app.buttons["brush-12"].waitForExistence(timeout: 10))
        XCTAssertFalse(pencil.exists)
        attachEditor(in: app, name: "sculpt-three-column-drawer")
        workspaceActivate(app.buttons["Brush"])
        XCTAssertTrue(pencil.waitForExistence(timeout: 10))
        expectation(for: NSPredicate(format: "value == %@", "37.0 px"), evaluatedWith: size)
        waitForExpectations(timeout: 10)
        workspaceActivate(app.buttons["Brush"])
        XCTAssertTrue(drawer.waitForNonExistence(timeout: 10), "The current opener toggles its drawer closed")
        workspaceActivate(app.buttons["Filters panel"])
        let brightness = app.buttons["adjustment-brightness_contrast"], curves = app.buttons["adjustment-curves"]
        workspaceActivate(brightness)
        XCTAssertTrue(app.buttons["number-value-property-brightness"].waitForExistence(timeout: 10))
        workspaceActivate(curves)
        XCTAssertTrue(app.buttons["property-channel"].waitForExistence(timeout: 10))
        XCTAssertFalse(brightness.isSelected); XCTAssertTrue(curves.isSelected)
        workspaceActivate(app.buttons["Filters panel"])
        workspaceActivate(app.buttons["Filters panel"])
        XCTAssertTrue(curves.isSelected, "Reopening retains the selected filter")
        attachEditor(in: app, name: "filter-three-column-drawer")
        workspaceActivate(app.buttons["cancel-filter"])
        XCTAssertTrue(drawer.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["Layers panel"])
        XCTAssertTrue(app.buttons["layer-New layer"].waitForExistence(timeout: 10))
        XCTAssertEqual(app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-")).count, 2)
        workspaceActivate(app.buttons["layer-thumbnail-2-content"])
        workspaceActivate(app.buttons["Filters panel"])
        let bucket = app.buttons["paper-color-bucket"]
        XCTAssertTrue(bucket.waitForExistence(timeout: 10))
        let paperPoint = CGPoint(x: 0.35, y: 0.7) // Exposed paper below the drawer.
        let beforePaper = editorPixels(in: app, at: paperPoint)
        workspaceActivate(bucket)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: paperPoint) != beforePaper }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        attachEditor(in: app, name: "paper-color-property")
        workspaceActivate(app.buttons["Layers panel"])
        #if os(iOS)
        let paper = app.descendants(matching: .any)["layer-row-2"].firstMatch
        let start = paper.coordinate(withNormalizedOffset: CGVector(dx: 0.65, dy: 0.5))
        start.press(forDuration: 0.01, thenDragTo: start.withOffset(CGVector(dx: -90, dy: 0)))
        workspaceActivate(app.buttons["layer-delete-2"])
        XCTAssertTrue(paper.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["Undo"])
        XCTAssertTrue(paper.waitForExistence(timeout: 10))
        #endif
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
        workspaceActivate(app.buttons["zen-button"])
        XCTAssertTrue(sketch.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["zen-button"])
        XCTAssertTrue(sketch.waitForExistence(timeout: 10))
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
