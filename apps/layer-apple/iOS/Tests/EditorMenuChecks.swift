import XCTest

final class EditorMenuChecks: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }
    @MainActor func testPanelGripTapBounds() {
        continueAfterFailure = true
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            let app = editorCaptureApplication()
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
            app.launch()
            let canvas = app.descendants(matching: .any)["canvas"].firstMatch
            expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
            waitForExpectations(timeout: 60)
            waitForEditorInteraction(in: app)
            let group = app.descendants(matching: .any)
                .matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-group-"))
                .containing(.button, identifier: "panel-tab-color").firstMatch
            let grip = group.descendants(matching: .any)
                .matching(NSPredicate(format: "identifier BEGINSWITH %@", "group-options-")).firstMatch
            let menu = app.descendants(matching: .any)["workspace-context-menu"].firstMatch
            let edit = app.buttons["paint-edit-color"].firstMatch, use = app.buttons["color-use"]
            XCTAssertTrue(grip.waitForExistence(timeout: 10))
            XCTAssertFalse(grip.frame.contains(CGPoint(x: edit.frame.midX, y: edit.frame.midY)))
            for x in [0.5, 0.15, 0.85] {
                edit.coordinate(withNormalizedOffset: CGVector(dx: x, dy: 0.5)).tap()
                let opened = use.waitForExistence(timeout: 10)
                if x == 0.5 || !opened { attachEditor(in: app, name: "panel-grip-adjacent-color-\(theme)-\(x)") }
                XCTAssertTrue(opened, "The adjacent color control must open Edit Color at \(x)")
                XCTAssertFalse(menu.exists, "A color tap must not open panel group options")
                if opened {
                    workspaceActivate(app.buttons["color-cancel"])
                    XCTAssertTrue(use.waitForNonExistence(timeout: 10))
                }
            }
            workspaceActivate(grip)
            let options = menu.waitForExistence(timeout: 10)
            attachEditor(in: app, name: "panel-grip-options-\(theme)")
            XCTAssertTrue(options, "The actual grip must still open group options")
            guard options else { return }
            app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.45)).tap()
            XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
            grip.press(forDuration: 0.8)
            XCTAssertTrue(menu.waitForExistence(timeout: 5), "Holding the grip must retain its context menu")
            app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.45)).tap()
            XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
            grip.doubleTap()
            let unexpected = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == YES"), object: menu)
            unexpected.isInverted = true
            XCTAssertEqual(XCTWaiter.wait(for: [unexpected], timeout: 1), .completed,
                "A double tap must suppress the ordinary grip menu")
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            app.terminate()
        }
    }
    @MainActor func testHeaderMenuTapAndHold() {
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            let app = editorTestApplication()
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
            app.launch()
            let canvas = app.descendants(matching: .any)["canvas"].firstMatch
            expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
            waitForExpectations(timeout: 60)
            waitForEditorInteraction(in: app)
            let viewMenu = app.buttons["menu-View"]
            let trigger = viewMenu.exists ? viewMenu : app.buttons["application-menus"]
            workspaceActivate(trigger)
            expectation(for: NSPredicate(format: "value == %@", "Expanded"), evaluatedWith: trigger)
            waitForExpectations(timeout: 5)
            let menu = app.descendants(matching: .any)["application-menu-content"].firstMatch
            XCTAssertTrue(menu.waitForExistence(timeout: 5))
            attachEditor(in: app, name: "header-menu-tap-\(theme)")
            let outside = app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.8))
            outside.tap()
            XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
            trigger.press(forDuration: 0.8)
            let context = app.descendants(matching: .any)["editor-action-menu"].firstMatch
            XCTAssertTrue(context.waitForExistence(timeout: 5))
            XCTAssertEqual(trigger.value as? String, "Collapsed", "Holding must open the context menu and suppress the ordinary tap")
            attachEditor(in: app, name: "header-menu-hold-\(theme)")
            outside.tap()
            XCTAssertTrue(context.waitForNonExistence(timeout: 5))
            XCTAssertFalse(app.staticTexts["Canvas error"].exists)
            app.terminate()
        }
    }
    @MainActor func testMainMenuThemesAndAction() {
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            let app = editorTestApplication()
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = "[{\"type\":\"set_theme\",\"theme\":\"\(theme)\"}]"
            app.launch()
            let newLayer = app.buttons["layer-New layer"]
            XCTAssertTrue(newLayer.waitForExistence(timeout: 20)); newLayer.tap()
            let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
            expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
            waitForExpectations(timeout: 5)
            app.buttons["menu-Edit"].tap()
            let menu = app.descendants(matching: .any)["application-menu-content"].firstMatch
            XCTAssertTrue(menu.waitForExistence(timeout: 5), "Main menu button state: \(String(describing: app.buttons["menu-Edit"].value))")
            XCTAssertLessThan(menu.frame.width, 380)
            let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            capture.name = "flat-main-menu-" + theme; capture.lifetime = .keepAlways; add(capture)
            app.buttons["command-undo"].tap()
            XCTAssertTrue(menu.waitForNonExistence(timeout: 5))
            expectation(for: NSPredicate { _, _ in rows.count == 2 }, evaluatedWith: app)
            waitForExpectations(timeout: 5)
            app.buttons["layer-blend"].tap()
            let normal = app.buttons["menu-action-Normal"]
            XCTAssertTrue(normal.waitForExistence(timeout: 5)); normal.tap()
            XCTAssertTrue(normal.waitForNonExistence(timeout: 5))
            app.terminate()
        }
    }
    @MainActor func testSubmenusAndShortcuts() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launch()
        let edit = app.buttons["menu-Edit"]
        XCTAssertTrue(edit.waitForExistence(timeout: 20))
        // The padded label is part of the button's target, including its edges.
        edit.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.1)).tap()
        let menu = app.descendants(matching: .any)["application-menu-content"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["command-undo"].isEnabled)
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.7)).tap()
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5))

        app.buttons["menu-Filter"].tap()
        let blur = app.buttons["menu-action-Blur"]
        XCTAssertTrue(blur.waitForExistence(timeout: 5)); blur.tap()
        let back = app.buttons["editor-menu-back"]
        XCTAssertTrue(back.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["menu-action-Gaussian Blur"].exists)
        back.tap()
        XCTAssertTrue(blur.waitForExistence(timeout: 5))
        app.typeKey(XCUIKeyboardKey.rightArrow, modifierFlags: [])
        XCTAssertTrue(back.waitForExistence(timeout: 5), "Right enters the focused submenu")
        app.typeKey(XCUIKeyboardKey.leftArrow, modifierFlags: [])
        XCTAssertTrue(back.waitForNonExistence(timeout: 5))
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.7)).tap()
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5))

        app.buttons["layer-New layer"].tap()
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        edit.tap()
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5), "The menu shortcut executes Undo and dismisses")
        expectation(for: NSPredicate { _, _ in rows.count == 2 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        app.typeKey("z", modifierFlags: [.command, .shift])
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
    }
    @MainActor func testCompactMenuShortcutAcrossPages() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"customize","action":{"type":"header","action":{"type":"remove","id":2}}},{"type":"customize","action":{"type":"header","action":{"type":"add","zone":"left","before":null,"item":{"kind":"menu"}}}},{"type":"invoke","command":"add_layer"}]"#
        app.launch()
        let trigger = app.buttons["application-menus"]
        XCTAssertTrue(trigger.waitForExistence(timeout: 20))
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        trigger.tap()
        let menu = app.descendants(matching: .any)["application-menu-content"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        let file = app.buttons["menu-action-File"]
        XCTAssertTrue(file.waitForExistence(timeout: 5)); file.tap()
        XCTAssertTrue(app.buttons["editor-menu-back"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["command-undo"].exists)
        app.typeKey("z", modifierFlags: .command)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5), "Undo must execute from the other submenu and dismiss")
        expectation(for: NSPredicate { _, _ in rows.count == 2 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        trigger.tap()
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["command-redo"].exists)
        app.typeKey("z", modifierFlags: [.command, .shift])
        XCTAssertTrue(menu.waitForNonExistence(timeout: 5), "Redo must execute before its submenu is opened")
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
    @MainActor func testWorkspaceMenuDragBothDirections() {
        continueAfterFailure = true
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"workspace_manager","command":{"type":"manage"}}]"#
        app.launch()
        let painter = app.descendants(matching: .any)["workspace-select-builtin:workspace:painter"].firstMatch
        let illustrator = app.descendants(matching: .any)["workspace-select-builtin:workspace:illustrator"].firstMatch
        let photographer = app.descendants(matching: .any)["workspace-select-builtin:workspace:photographer"].firstMatch
        XCTAssertTrue(photographer.waitForExistence(timeout: 30))
        XCTAssertTrue(illustrator.isSelected)
        photographer.press(forDuration: 0.8)
        let menu = app.descendants(matching: .any)["workspace-row-menu"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 5), "Stationary release retains the same vertical menu helper")
        app.buttons["menu-action-Move Up"].tap()
        expectation(for: NSPredicate { _, _ in photographer.frame.minY < illustrator.frame.minY }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        photographer.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.8,
            thenDragTo: painter.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1)), withVelocity: .slow, thenHoldForDuration: 0.1)
        expectation(for: NSPredicate { _, _ in photographer.frame.minY < painter.frame.minY }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(menu.exists)
        photographer.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.8,
            thenDragTo: illustrator.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.9)), withVelocity: .slow, thenHoldForDuration: 0.1)
        expectation(for: NSPredicate { _, _ in photographer.frame.minY > illustrator.frame.minY }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(illustrator.isSelected, "Menu actions and held dragging must not select another workspace")
    }
}

extension EditorMenuChecks {
    @MainActor func testColorEntryKeepsDraftFocus() {
        continueAfterFailure = true
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            let app = editorCaptureApplication()
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
            app.launch()
            defer { app.terminate() }
            waitForEditorInteraction(in: app)
            workspaceActivate(app.buttons["paint-edit-color"].firstMatch)
            guard app.buttons["color-use"].waitForExistence(timeout: 15) else { XCTFail("Edit Color opens"); continue }
            workspaceActivate(app.descendants(matching: .any)["color-value-hex"].firstMatch)
            let field = app.textFields["color-entry-hex"]
            guard field.waitForExistence(timeout: 5) else { XCTFail("The hex value opens for typing"); continue }
            let current = field.value as? String ?? ""
            field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: current.count) + "#123")
            let closed = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == NO"), object: field)
            closed.isInverted = true
            XCTAssertEqual(XCTWaiter.wait(for: [closed], timeout: 4), .completed, "A pause while typing must preserve color focus")
            attachEditor(in: app, name: "color-entry-paused-\(theme)")
            guard field.exists else {
                let hierarchy = XCTAttachment(string: app.debugDescription)
                hierarchy.name = "color-entry-focus-lost-\(theme)"; hierarchy.lifetime = .keepAlways; add(hierarchy)
                continue
            }
            XCTAssertTrue(app.textFields.matching(NSPredicate(format: "identifier == %@ AND value == %@", "color-entry-hex", "#123")).firstMatch.exists,
                "The partial draft must stay available while typing pauses")
            field.typeText("456\n")
            XCTAssertTrue(field.waitForNonExistence(timeout: 5))
            XCTAssertTrue(app.descendants(matching: .any).matching(NSPredicate(format: "identifier == %@ AND value == %@", "color-value-hex", "#123456")).firstMatch.waitForExistence(timeout: 10),
                "Finishing the same draft must commit the exact color")
        }
    }
    @MainActor func testColorPanelResizeTargets() {
        continueAfterFailure = true
        XCUIDevice.shared.orientation = .landscapeLeft
        for theme in ["light", "dark"] {
            let app = editorCaptureApplication()
            app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:illustrator"}}]"#
            app.launch()
            defer { app.terminate() }
            let canvas = app.descendants(matching: .any)["canvas"].firstMatch
            XCTAssertTrue(canvas.waitForExistence(timeout: 30))
            expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
            waitForExpectations(timeout: 60)
            waitForEditorInteraction(in: app)
            let tab = app.buttons["panel-tab-color"].firstMatch
            guard tab.waitForExistence(timeout: 10) else {
                attachEditor(in: app, name: "color-resize-missing-tab-\(theme)")
                XCTFail("The fixture requires a docked Color tab")
                continue
            }
            if !tab.isSelected { workspaceActivate(tab) }
            let group = app.descendants(matching: .any)
                .matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-group-"))
                .containing(.button, identifier: "panel-tab-color").firstMatch
            let controls = app.descendants(matching: .any)["color-panel-controls"].firstMatch
            guard group.waitForExistence(timeout: 10), controls.waitForExistence(timeout: 10) else {
                XCTFail("The fixture requires the visible Color group and its controls")
                continue
            }
            func viewport() -> CGRect {
                let frame = group.frame
                return CGRect(x: frame.minX, y: tab.frame.maxY,
                    width: frame.width, height: max(0, frame.maxY - tab.frame.maxY))
            }
            let original = viewport()
            let dividers = app.descendants(matching: .any)
                .matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-divider-"))
            let candidates = dividers.allElementsBoundByIndex.filter {
                let frame = $0.frame
                return frame.width > frame.height && frame.maxY <= tab.frame.minY + 2
                    && frame.maxX > group.frame.minX && frame.minX < group.frame.maxX
            }.sorted { abs($0.frame.midY - tab.frame.minY) < abs($1.frame.midY - tab.frame.minY) }
            guard let divider = candidates.first,
                  abs(divider.frame.maxY - tab.frame.minY) < tab.frame.height else {
                attachEditor(in: app, name: "color-resize-unavailable-\(theme)")
                XCTFail("The docked Color group must expose its horizontal divider")
                continue
            }
            let start = divider.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            start.press(forDuration: 0.1, thenDragTo: start.withOffset(CGVector(dx: 0, dy: -60)))
            let resized = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                viewport().height > original.height + 25
            }, object: app)
            let achieved = XCTWaiter.wait(for: [resized], timeout: 10) == .completed
            attachEditor(in: app, name: "color-resize-allocation-\(theme)")
            let hierarchy = XCTAttachment(string: "original=\(original) actual=\(viewport())\n\(app.debugDescription)")
            hierarchy.name = "color-resize-geometry-\(theme)"; hierarchy.lifetime = .keepAlways
            add(hierarchy)
            XCTAssertTrue(achieved, "The real divider must increase the Color body height")
            guard achieved else { continue }
            let allocation = viewport().insetBy(dx: -1, dy: -1)
            for identifier in ["color-wheel", "paint-edit-color", "color-shape-square", "color-shape-triangle"] {
                let element = app.descendants(matching: .any)[identifier].firstMatch
                if element.exists {
                    XCTAssertTrue(allocation.contains(element.frame), "\(identifier) exceeds the actual resized viewport: \(element.frame), viewport=\(viewport())")
                }
            }
            let edit = app.buttons["paint-edit-color"].firstMatch
            let use = app.buttons["color-use"]
            for x in [0.15, 0.5, 0.85] {
                waitForEditorInteraction(in: app)
                edit.coordinate(withNormalizedOffset: CGVector(dx: x, dy: 0.5)).tap()
                let opened = use.waitForExistence(timeout: 10)
                XCTAssertTrue(opened, "Resized Color Edit must open at horizontal fraction \(x)")
                if opened {
                    workspaceActivate(app.buttons["color-cancel"])
                    XCTAssertTrue(use.waitForNonExistence(timeout: 10))
                } else {
                    attachEditor(in: app, name: "color-resize-edit-failed-\(theme)-\(x)")
                }
            }
            let restore = divider.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            restore.press(forDuration: 0.1, thenDragTo: restore.withOffset(CGVector(dx: 0, dy: 60)))
            expectation(for: NSPredicate { _, _ in abs(viewport().height - original.height) <= 1 }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
    }
}
