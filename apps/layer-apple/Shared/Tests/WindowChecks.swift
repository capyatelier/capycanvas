import XCTest

extension XCTestCase {
    #if os(macOS)
    @MainActor func checkNewEditorAfterLastWindowClose(in app: XCUIApplication) {
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let scenes = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "editor-scene-"))
        for useShortcut in [false, true] {
            XCTAssertEqual(scenes.count, 1)
            let previousID = scenes.firstMatch.identifier
            editorMenu(in: app, menu: "File", id: "close_document", label: "Close")
            XCTAssertTrue(scenes.firstMatch.waitForNonExistence(timeout: 20))
            XCTAssertNotEqual(app.state, .notRunning, "Closing the last window must retain the Mac app")
            workspaceActivate(app.menuBars.menuBarItems["File"])
            let newWindow = app.menuBars.menuItems["New Window"].firstMatch
            XCTAssertTrue(newWindow.waitForExistence(timeout: 10)); XCTAssertTrue(newWindow.isEnabled)
            if useShortcut {
                app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
                app.typeKey("n", modifierFlags: .command)
            } else { workspaceActivate(newWindow) }
            XCTAssertTrue(scenes.firstMatch.waitForExistence(timeout: 30))
            XCTAssertNotEqual(scenes.firstMatch.identifier, previousID)
            capturePaintEditor(in: app)
            XCTAssertEqual(scenes.count, 1)
            XCTAssertEqual(app.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-")).count, 2)
            XCTAssertFalse(app.alerts.firstMatch.exists)
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        }
        attachEditor(in: app, name: "new-editor-after-last-window-close")
    }

    @MainActor func checkFullscreenEditor(in app: XCUIApplication) {
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        let window = app.windows.firstMatch, originalFrame = window.frame
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let clock = app.staticTexts["system-clock"]
        let scenes = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "editor-scene-"))
        let sceneID = scenes.firstMatch.identifier
        let points = [CGPoint(x: 0.45, y: 0.5), CGPoint(x: 0.5, y: 0.6), CGPoint(x: 0.6, y: 0.55)]
        func expectPaper(blue: Bool) {
            expectation(for: NSPredicate { _, _ in
                let samples = self.editorPixelSamples(in: app, at: points, size: 8)
                for sample in samples {
                    for i in stride(from: 0, to: sample.count, by: 4) {
                        if blue {
                            if Int(sample[i + 2]) <= Int(sample[i]) + 50 { return false }
                        } else if sample[i] <= 250 || sample[i + 1] <= 250 || sample[i + 2] <= 250 {
                            return false
                        }
                    }
                }
                return true
            }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        func fill() { editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection") }
        func fullscreen(_ enter: Bool) {
            app.menuBars.menuBarItems["View"].click()
            let item = app.menuItems[enter ? "Enter Full Screen" : "Exit Full Screen"].firstMatch
            XCTAssertTrue(item.waitForExistence(timeout: 5)); XCTAssertTrue(item.isEnabled)
            item.click()
            // Clock visibility follows the actual native transition callback.
            XCTAssertTrue(enter ? clock.waitForExistence(timeout: 15) : clock.waitForNonExistence(timeout: 15))
            app.staticTexts["document-title"].hover()
        }
        func expectLayout() {
            XCTAssertEqual(canvas.frame, window.frame, "Metal must extend behind the editor header")
            XCTAssertEqual(scenes.count, 1); XCTAssertEqual(scenes.firstMatch.identifier, sceneID)
            for id in ["panel-tab-brushes", "panel-tab-color", "panel-tab-layers", "workspace-switch-builtin:workspace:illustrator"] {
                XCTAssertTrue(window.frame.contains(app.buttons[id].frame), "The visible editor control must fit: \(id)")
            }
            XCTAssertEqual(app.descendants(matching: .any)["navigator-overview"].firstMatch.value as? String, "Live preview")
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        }
        XCTAssertFalse(clock.exists); expectLayout(); expectPaper(blue: false)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        fill(); expectPaper(blue: true)
        fullscreen(true)
        XCTAssertGreaterThan(window.frame.width, originalFrame.width)
        expectLayout(); expectPaper(blue: true)
        attachEditor(in: app, name: "fullscreen-artwork")
        editorHistory("Undo", in: app); expectPaper(blue: false)
        editorHistory("Redo", in: app); expectPaper(blue: true)
        editorHistory("Undo", in: app); expectPaper(blue: false)

        let pen = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-toolbar-", "Pen")).firstMatch
        if !pen.isSelected { workspaceActivate(pen) }
        let samplePoint = CGPoint(x: 0.53, y: 0.55)
        let blank = editorPixels(in: app, at: samplePoint)
        window.coordinate(withNormalizedOffset: CGVector(dx: 0.44, dy: 0.55)).click(forDuration: 0.05,
            thenDragTo: window.coordinate(withNormalizedOffset: CGVector(dx: 0.62, dy: 0.55)))
        app.staticTexts["document-title"].hover()
        expectation(for: NSPredicate { _, _ in
            let sample = self.editorPixels(in: app, at: samplePoint)
            return stride(from: 0, to: sample.count, by: 4).contains { Int(sample[$0 + 2]) > Int(sample[$0]) + 50 }
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let painted = editorPixels(in: app, at: samplePoint)
        attachEditor(in: app, name: "fullscreen-mouse-stroke")
        for (command, expected) in [("Undo", blank), ("Redo", painted), ("Undo", blank)] {
            editorHistory(command, in: app)
            expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: samplePoint) == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        fill(); expectPaper(blue: true)
        fullscreen(false)
        expectation(for: NSPredicate { _, _ in window.frame == originalFrame }, evaluatedWith: window)
        waitForExpectations(timeout: 10)
        expectLayout(); expectPaper(blue: true)
        editorHistory("Undo", in: app); expectPaper(blue: false)
        editorHistory("Redo", in: app); expectPaper(blue: true)
        attachEditor(in: app, name: "fullscreen-returned-window")
    }
    #endif

    @MainActor func checkIndependentEditorWindows(in app: XCUIApplication) {
        // Install ordinary toolbar commands so the test exercises editor
        // effects without navigating the macOS system menu bar.
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"customize","action":{"type":"insert_tools","panel":"commands","before":null}},{"type":"customize","action":{"type":"picker_select","control":{"kind":"command","command":"new_window"},"selected":true}},{"type":"customize","action":{"type":"picker_select","control":{"kind":"command","command":"close_document"},"selected":true}},{"type":"customize","action":{"type":"confirm_tools"}}]"#
        app.launch()
        let scenes = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "editor-scene-"))
        XCTAssertTrue(scenes.firstMatch.waitForExistence(timeout: 20))
        let firstID = scenes.firstMatch.identifier
        let first = app.descendants(matching: .any)[firstID].firstMatch
        func activate(_ button: XCUIElement) {
            XCTAssertTrue(button.waitForExistence(timeout: 10))
            expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: button)
            waitForExpectations(timeout: 20)
            #if os(macOS)
            button.click()
            #else
            button.tap()
            #endif
        }
        func command(_ scene: XCUIElement, _ label: String) -> XCUIElement {
            scene.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", label)).firstMatch
        }
        func rows(_ scene: XCUIElement) -> XCUIElementQuery {
            #if os(macOS)
            return scene.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            #else
            return scene.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            #endif
        }
        func expectRows(_ scene: XCUIElement, _ count: Int) {
            expectation(for: NSPredicate { _, _ in rows(scene).count == count }, evaluatedWith: scene)
            waitForExpectations(timeout: 20)
        }
        expectRows(first, 2)
        activate(first.buttons["layer-New layer"])
        activate(first.buttons["layer-New layer"])
        expectRows(first, 4)
        activate(command(first, "New Window"))
        let second = scenes.matching(NSPredicate(format: "identifier != %@", firstID)).firstMatch
        XCTAssertTrue(second.waitForExistence(timeout: 30), "New Window must create another editor scene")
        XCTAssertNotEqual(second.identifier, firstID)
        expectRows(second, 2)
        activate(second.buttons["layer-New layer"])
        expectRows(second, 3)
        activate(command(second, "Undo"))
        expectRows(second, 2)
        #if os(macOS)
        expectRows(first, 4)
        #endif
        let opened = XCTAttachment(screenshot: app.screenshot())
        opened.name = "independent-editor-windows"; opened.lifetime = .keepAlways; add(opened)
        activate(command(second, "Close"))
        XCTAssertTrue(second.waitForNonExistence(timeout: 20), "Close must destroy only the second editor scene")
        // Closing the foreground iPad scene can leave the surviving scene in
        // the app switcher. Bring the application forward before its next edit.
        app.activate()
        XCTAssertTrue(first.waitForExistence(timeout: 20))
        expectRows(first, 4)
        activate(command(first, "Undo"))
        expectRows(first, 3)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        // Leave this disposable test document clean before app termination.
        activate(command(first, "Undo"))
        expectRows(first, 2)
        app.terminate()
    }
}
