import XCTest
import CoreGraphics
import ImageIO

extension XCTestCase {
    @MainActor func checkTitleBarToolDrawers(in app: XCUIApplication, theme: String = "light", bridgesOnly: Bool = false) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:painter"}}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 60)
        let sketch = app.buttons["workspace-switch-builtin:workspace:painter"]
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: sketch)
        waitForExpectations(timeout: 30)
        waitForEditorInteraction(in: app)
        XCTAssertTrue(app.buttons["header-tool-drawing_brush"].waitForExistence(timeout: 10))
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
        expectation(for: NSPredicate { _, _ in
            drawer.exists && self.workspaceViewport(in: app).frame.contains(drawer.frame)
        }, evaluatedWith: drawer)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(workspaceViewport(in: app).frame.contains(drawer.frame),
            "Drawer \(drawer.frame) must fit the editor \(workspaceViewport(in: app).frame)")
        attachEditor(in: app, name: "color-palette-tool-drawer-" + theme)
        workspaceActivate(app.buttons["header-tool-drawing_brush"])
        XCTAssertTrue(wheel.waitForNonExistence(timeout: 10), "Another header tool switches the drawer in one click")
        XCTAssertTrue(drawer.exists)
        if bridgesOnly {
            attachEditor(in: app, name: "brush-tool-bridge-" + theme)
            workspaceActivate(app.buttons["header-tool-sculpt"])
            XCTAssertTrue(drawer.waitForExistence(timeout: 10))
            attachEditor(in: app, name: "sculpt-tool-bridge-" + theme)
            workspaceActivate(app.buttons["Filters panel"])
            XCTAssertTrue(drawer.waitForExistence(timeout: 10))
            attachEditor(in: app, name: "filters-tool-bridge-" + theme)
            return
        }
        let pencil = app.buttons["brush_sets-Pencil"]
        XCTAssertTrue(pencil.waitForExistence(timeout: 10))
        workspaceActivate(pencil)
        workspaceActivate(app.buttons["brush-2"])
        let size = app.buttons["number-value-tool-size"]
        workspaceActivate(size)
        let entry = app.textFields["number-entry-tool-size"]
        replaceEditorText(entry, with: "37", in: app)
        #if os(macOS)
        entry.typeKey(XCUIKeyboardKey.return.rawValue, modifierFlags: [])
        #else
        entry.typeText("\n")
        #endif
        expectation(for: NSPredicate(format: "value == %@", "37 px"), evaluatedWith: size)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "brush-three-column-drawer")
        workspaceActivate(app.buttons["header-tool-sculpt"])
        workspaceActivate(app.buttons["sculpt_sets-Liquify"])
        XCTAssertTrue(app.buttons["brush-12"].waitForExistence(timeout: 10))
        XCTAssertFalse(pencil.exists)
        attachEditor(in: app, name: "sculpt-three-column-drawer")
        workspaceActivate(app.buttons["header-tool-drawing_brush"])
        XCTAssertTrue(pencil.waitForExistence(timeout: 10))
        expectation(for: NSPredicate(format: "value == %@", "37 px"), evaluatedWith: size)
        waitForExpectations(timeout: 10)
        workspaceActivate(app.buttons["header-tool-drawing_brush"])
        XCTAssertTrue(drawer.waitForNonExistence(timeout: 10), "The current opener toggles its drawer closed")
        workspaceActivate(app.buttons["Filters panel"])
        let brightness = app.buttons["adjustment-brightness_contrast"], curves = app.buttons["adjustment-curves"]
        workspaceActivate(brightness)
        XCTAssertTrue(app.buttons["number-value-property-brightness"].waitForExistence(timeout: 10))
        workspaceActivate(curves)
        XCTAssertTrue(app.buttons["properties-page"].waitForExistence(timeout: 10))
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
        let colorCancel = app.buttons["color-cancel"]
        XCTAssertTrue(colorCancel.waitForExistence(timeout: 10))
        workspaceActivate(colorCancel)
        XCTAssertTrue(colorCancel.waitForNonExistence(timeout: 5))
        workspaceActivate(app.buttons["Filters panel"])
        let bucket = app.buttons["property-color-bucket"]
        XCTAssertTrue(bucket.waitForExistence(timeout: 10))
        let paperPoint = CGPoint(x: 0.5, y: 0.65) // Exposed paper below the drawer.
        let beforePaper = editorPixels(in: app, at: paperPoint)
        workspaceActivate(bucket)
        expectation(for: NSPredicate { _, _ in self.editorPixels(in: app, at: paperPoint) != beforePaper }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        attachEditor(in: app, name: "paper-color-property")
        let texture = app.buttons["property-texture"], scale = app.buttons["number-value-property-scale"]
        let propertyScroll = app.scrollViews.containing(.any, identifier: "layer-properties").firstMatch
        func expectPaperValue(_ element: XCUIElement, _ text: String) {
            expectation(for: NSPredicate(format: "value == %@", text), evaluatedWith: element)
            waitForExpectations(timeout: 10)
        }
        func reopenToolDrawer(_ label: String) {
            #if os(iOS)
            XCTAssertTrue(drawer.waitForNonExistence(timeout: 10))
            workspaceActivate(app.buttons[label])
            XCTAssertTrue(drawer.waitForExistence(timeout: 10))
            #endif
        }
        let papers = ["Hot press", "Cold press", "Rough", "Extra rough", "Fine tooth", "Bristol", "Newsprint", "Ingres",
            "Pastel", "Sanded", "Coarse grit", "Washi", "Unryu", "Handmade", "Canvas", "Linen",
            "Canvas paper", "Rough linen", "Burlap", "Gesso panel", "Gesso across", "Gesso down", "Rough gesso", "Rough across",
            "Rough down", "Matte film", "Chalkboard", "Concrete", "Sand", "Asphalt", "Barn wood", "Dry mud",
            "Crumpled", "Papyrus", "Reeds across", "Reeds down", "Leaf", "Toilet"]
        XCTAssertTrue(texture.waitForExistence(timeout: 10)); expectPaperValue(texture, "Cold press")
        let textureDialog = app.descendants(matching: .any)["texture-dialog"].firstMatch
        func chooseTexture(_ index: Int) {
            let id = "texture-tile-\(index)", option = app.buttons["texture-tile-\(index)"]
            revealEditorControl(option, identifier: id, type: .button, in: textureDialog.scrollViews.firstMatch)
            workspaceActivate(option)
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: option)
            waitForExpectations(timeout: 10)
        }
        var previousPaper = "Cold press"
        for (index, name) in papers.enumerated() {
            revealEditorControl(texture, in: propertyScroll); workspaceActivate(texture)
            XCTAssertTrue(textureDialog.waitForExistence(timeout: 10))
            if index == 0 {
                let options = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "texture-tile-"))
                XCTAssertEqual(options.count, papers.count)
                for (option, label) in papers.enumerated() { XCTAssertEqual(app.buttons["texture-tile-\(option)"].label, label) }
                XCTAssertTrue(app.buttons["texture-tile-1"].isSelected, "Choose Texture starts on the current texture")
                chooseTexture(papers.count - 1)
                workspaceActivate(app.buttons["texture-cancel"])
                XCTAssertTrue(textureDialog.waitForNonExistence(timeout: 10))
                expectPaperValue(texture, "Cold press")
                revealEditorControl(texture, in: propertyScroll); workspaceActivate(texture)
                XCTAssertTrue(textureDialog.waitForExistence(timeout: 10))
                XCTAssertTrue(app.buttons["texture-tile-1"].isSelected, "Reopening forgets the cancelled choice")
                attachEditor(in: app, name: "texture-dialog-" + theme)
            }
            chooseTexture(index)
            workspaceActivate(app.buttons["texture-apply"])
            XCTAssertTrue(textureDialog.waitForNonExistence(timeout: 10))
            XCTAssertTrue(drawer.exists, "Choose Texture preserves the tool drawer")
            expectPaperValue(texture, name)
            editorMenu(in: app, menu: "Edit", id: "undo", label: "Undo"); reopenToolDrawer("Filters panel"); expectPaperValue(texture, previousPaper)
            editorMenu(in: app, menu: "Edit", id: "redo", label: "Redo"); reopenToolDrawer("Filters panel"); expectPaperValue(texture, name)
            previousPaper = name
        }
        revealEditorControl(scale, in: propertyScroll); expectPaperValue(scale, "100 %")
        workspaceActivate(scale)
        let scaleEntry = app.textFields["number-entry-property-scale"]
        replaceEditorText(scaleEntry, with: "200", in: app)
        #if os(macOS)
        scaleEntry.typeKey(XCUIKeyboardKey.return.rawValue, modifierFlags: [])
        #else
        scaleEntry.typeText("\n")
        #endif
        expectPaperValue(scale, "200 %")
        editorMenu(in: app, menu: "Edit", id: "undo", label: "Undo"); reopenToolDrawer("Filters panel"); expectPaperValue(scale, "100 %")
        editorMenu(in: app, menu: "Edit", id: "redo", label: "Redo"); reopenToolDrawer("Filters panel"); expectPaperValue(scale, "200 %")
        let scaleTrack = app.descendants(matching: .any)["number-track-property-scale"].firstMatch
        revealEditorControl(scaleTrack, in: propertyScroll)
        scaleTrack.coordinate(withNormalizedOffset: CGVector(dx: 0.35, dy: 0.5)).press(forDuration: 0.05,
            thenDragTo: scaleTrack.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)),
            withVelocity: .slow, thenHoldForDuration: 0.1)
        expectation(for: NSPredicate { _, _ in
            let value = scale.value as? String ?? ""
            return !value.isEmpty && value != "200 %"
        }, evaluatedWith: scale)
        waitForExpectations(timeout: 10)
        let draggedScale = scale.value as? String ?? ""
        XCTAssertFalse(draggedScale.isEmpty); XCTAssertNotEqual(draggedScale, "200 %")
        editorMenu(in: app, menu: "Edit", id: "undo", label: "Undo"); reopenToolDrawer("Filters panel"); expectPaperValue(scale, "200 %")
        editorMenu(in: app, menu: "Edit", id: "redo", label: "Redo"); reopenToolDrawer("Filters panel"); expectPaperValue(scale, draggedScale)
        let scaleLabel = app.staticTexts["Scale"].firstMatch
        XCTAssertTrue(scaleLabel.waitForExistence(timeout: 5))
        revealEditorControl(scaleLabel, in: propertyScroll)
        let window = app.windows.firstMatch, caption = scaleLabel.frame
        guard caption.width > 0, caption.height > 0, propertyScroll.frame.contains(caption), window.frame.contains(caption) else {
            return XCTFail("The Scale caption must be visible inside its editor window")
        }
        let resetScale = window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: caption.midX - window.frame.minX, dy: caption.midY - window.frame.minY))
        #if os(macOS)
        resetScale.doubleClick()
        #else
        resetScale.doubleTap()
        #endif
        expectPaperValue(scale, "100 %")
        attachEditor(in: app, name: "paper-texture-scale-" + theme)
        workspaceActivate(app.buttons["Layers panel"])
        #if os(iOS)
        let paper = app.descendants(matching: .any)["layer-row-2"].firstMatch
        let start = paper.coordinate(withNormalizedOffset: CGVector(dx: 0.65, dy: 0.5))
        start.press(forDuration: 0.01, thenDragTo: start.withOffset(CGVector(dx: -90, dy: 0)))
        workspaceActivate(app.buttons["layer-delete-2"])
        XCTAssertTrue(paper.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["Undo"])
        reopenToolDrawer("Layers panel")
        XCTAssertTrue(paper.waitForExistence(timeout: 10))
        let ink = app.descendants(matching: .any)["layer-row-1"].firstMatch, alphaLock = app.buttons["layer-Alpha lock"]
        workspaceActivate(app.buttons["layer-thumbnail-1-content"])
        expectation(for: NSPredicate(format: "selected == NO"), evaluatedWith: alphaLock); waitForExpectations(timeout: 10)
        let inkStart = ink.coordinate(withNormalizedOffset: CGVector(dx: 0.35, dy: 0.5))
        inkStart.press(forDuration: 0.01, thenDragTo: inkStart.withOffset(CGVector(dx: 90, dy: 0)))
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: alphaLock); waitForExpectations(timeout: 10)
        XCTAssertTrue(paper.exists, "A right swipe never deletes")
        workspaceActivate(app.buttons["Undo"])
        reopenToolDrawer("Layers panel")
        expectation(for: NSPredicate(format: "selected == NO"), evaluatedWith: alphaLock); waitForExpectations(timeout: 10)
        #endif
        let layersPage = drawer.descendants(matching: .any)["layer-properties-pages"].firstMatch
        let layerList = layersPage.scrollViews["layer-rows"]
        let layerRows = layerList.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let opacity = drawer.buttons["number-value-layer-opacity"]
        let layerActions = drawer.buttons["layer-Layer actions"]
        let disclosure = drawer.buttons["layer-properties-disclosure"]
        let controls = [layersPage, layerList, opacity, layerActions, disclosure]
        var lastLayerBounds = [CGRect](), stableSince = Date()
        expectation(for: NSPredicate { _, _ in
            guard drawer.exists, controls.allSatisfy({ $0.exists }),
                let row = layerRows.allElementsBoundByIndex.first(where: {
                    !$0.frame.isEmpty && layerList.frame.contains($0.frame)
                }) else { lastLayerBounds = []; return false }
            let name = row.staticTexts.firstMatch
            let thumbnail = row.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-thumbnail-")).firstMatch
            guard name.exists, thumbnail.exists, !(name.value as? String ?? name.label).isEmpty else { lastLayerBounds = []; return false }
            let bounds = drawer.frame, frames = (controls + [row, name, thumbnail]).map(\.frame)
            guard !bounds.isEmpty, self.workspaceViewport(in: app).frame.contains(bounds),
                frames.allSatisfy({ !$0.isEmpty && bounds.contains($0) }),
                row.frame.contains(name.frame), row.frame.contains(thumbnail.frame),
                thumbnail.frame.maxX <= name.frame.minX else { lastLayerBounds = []; return false }
            if frames != lastLayerBounds { lastLayerBounds = frames; stableSince = Date() }
            return Date().timeIntervalSince(stableSince) > 0.3
        }, evaluatedWith: drawer)
        waitForExpectations(timeout: 10)
        if let row = layerRows.allElementsBoundByIndex.first(where: { !$0.frame.isEmpty && layerList.frame.contains($0.frame) }),
           let eye = row.buttons.allElementsBoundByIndex.min(by: { $0.frame.minX < $1.frame.minX }) {
            XCTAssertEqual(row.frame.width, drawer.frame.width, accuracy: 1, "Drawer rows run edge to edge like the panel's")
            for icon in [eye, drawer.buttons["layer-Alpha lock"], drawer.buttons["layer-New layer"]] {
                XCTAssertEqual(icon.frame.minX - drawer.frame.minX, 6, accuracy: 1, "\(icon.identifier) lines up with the rows' eye")
            }
            XCTAssertEqual(drawer.frame.maxX - layerActions.frame.maxX, 6, accuracy: 1, "Layer actions mirror the eye's inset")
        } else { XCTFail("The Layers drawer shows a row") }
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
        workspaceActivate(app.descendants(matching: .any)["zen-button"].firstMatch)
        expectation(for: NSPredicate { _, _ in !sketch.exists || !sketch.isEnabled }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "titlebar-zen-hidden-" + theme)
        workspaceActivate(app.descendants(matching: .any)["zen-button"].firstMatch)
        expectation(for: NSPredicate { _, _ in sketch.exists && sketch.isEnabled && sketch.isHittable }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        #if os(macOS)
        XCTAssertFalse(app.menuBars.menuBarItems["Help"].exists)
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkTitleBarSystemStatus(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"customize_workspace_ui"}]"#
        app.launch()
        func activate(_ element: XCUIElement) {
            XCTAssertTrue(element.waitForExistence(timeout: 15))
            element.clickOrTap()
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
        XCTAssertFalse(menu.buttons["Recovered Drawings…"].exists)
        workspaceActivate(menu.buttons["editor-menu-back"])
        workspaceActivate(menu.buttons["Window"])
        workspaceActivate(menu.buttons["Customize Title Bar…"])
        XCTAssertTrue(app.buttons["header-done"].waitForExistence(timeout: 10), "The shared Main Menu enters title-bar editing")
        workspaceActivate(app.buttons["header-cancel"])
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}

private func paperUIFileWork<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
    try await withCheckedThrowingContinuation { done in
        DispatchQueue.global(qos:.utility).async { done.resume(with:Result(catching:work)) }
    }
}

extension XCTestCase {
    @MainActor func checkOrdinaryPaperLibrary(in app: XCUIApplication, theme: String) async throws {
        let token = UUID().uuidString
        let fileName = "PaperScan"
        #if os(macOS)
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("Capy Paper " + token)
        try await paperUIFileWork { try FileManager.default.createDirectory(at:root,withIntermediateDirectories:false) }
        let photo = root.appendingPathComponent(fileName + ".png")
        #else
        let folder = "Capy Files " + token
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.1,0.3,0.9,1]}]"#
        app.launch(); capturePaintEditor(in:app,theme:theme)
        let canvas = app.descendants(matching:.any)["canvas"].firstMatch
        func expect(_ element: XCUIElement, value: String) {
            expectation(for:NSPredicate(format:"value == %@",value),evaluatedWith:element)
            waitForExpectations(timeout:20)
        }
        func createDrawing(defaults: Bool = false) {
            editorMenu(in:app,menu:"File",id:"new_document",label:"New…")
            let create = app.buttons["new-drawing-create"]
            XCTAssertTrue(create.waitForExistence(timeout:20))
            if defaults {
                chooseNewDrawingOption("new-drawing-unit",3,in:app)
                enterNewDrawingNumber("width","128",in:app)
                enterNewDrawingNumber("height","128",in:app)
            }
            XCTAssertEqual(newDrawingNumber("width",in:app).value as? String,"128","New reopens with the last drawing")
            XCTAssertEqual(newDrawingNumber("height",in:app).value as? String,"128")
            #if os(iOS)
            dismissEditorKeyboard(in:app)
            #endif
            workspaceActivate(create)
            XCTAssertTrue(create.waitForNonExistence(timeout:60))
            expect(canvas,value:"Canvas ready")
            capturePaintEditor(in:app,theme:theme)
            let title=editorDocumentTitle(in:app)
            expectation(for:NSPredicate(format:"label CONTAINS %@ OR value CONTAINS %@","128 × 128","128 × 128"),evaluatedWith:title)
            waitForExpectations(timeout:20)
            editorMenu(in:app,menu:"View",id:"fit_canvas",label:"Fit canvas")
        }
        createDrawing(defaults:true)
        editorMenu(in:app,menu:"Select",id:"select_all",label:"Select all pixels")
        editorMenu(in:app,menu:"Edit",id:"fill_selection",label:"Fill selection")
        expectation(for:NSPredicate { _,_ in let p=self.editorPixels(in:app);return Int(p[2])>Int(p[0])+50 },evaluatedWith:app)
        waitForExpectations(timeout:20)
        let bounds = bluePaperBounds(in:app)
        let imagePoints = [CGPoint(x:bounds.midX,y:bounds.midY),CGPoint(x:bounds.minX+bounds.width*0.1,y:bounds.midY)]
        expectBluePaper([true,true],at:imagePoints,in:app)
        editorMenu(in:app,menu:"Edit",id:"scale_rotate",label:"Transform")
        let uniform = app.buttons["tool-action-transform_uniform"]
        XCTAssertTrue(uniform.waitForExistence(timeout:10))
        if !uniform.isSelected { workspaceActivate(uniform) }
        let width = app.buttons["number-value-tool-transform_width"]
        revealEditorControl(width,in:app.scrollViews.containing(.button,identifier:width.identifier).firstMatch)
        workspaceActivate(width)
        app.textFields["number-entry-tool-transform_width"].typeText("50\n")
        expect(width,value:"50 %")
        expect(app.buttons["number-value-tool-transform_height"],value:"50 %")
        #if os(iOS)
        dismissEditorKeyboard(in:app)
        #endif
        let apply = app.buttons["tool-action-apply_transform"]
        revealEditorControl(apply,in:app.scrollViews.containing(.button,identifier:apply.identifier).firstMatch)
        workspaceActivate(apply)
        XCTAssertTrue(width.waitForNonExistence(timeout:20))
        editorMenu(in:app,menu:"Select",id:"deselect",label:"Deselect pixels")
        expectBluePaper([true,false],at:imagePoints,in:app)
        attachEditor(in:app,name:"paper-source-nonuniform-"+theme)
        editorMenu(in:app,menu:"File",id:"export_document",label:"Export…")
        chooseExportDestination(in:app)
        #if os(macOS)
        let save = app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(save.waitForExistence(timeout:20)); goToFolder(root,in:app)
        replaceEditorText(app.textFields["saveAsNameTextField"],with:photo.lastPathComponent,in:app)
        workspaceActivate(save);XCTAssertTrue(save.waitForNonExistence(timeout:20))
        let raw = try await paperUIFileWork {
            let size=(try FileManager.default.attributesOfItem(atPath:photo.path)[.size] as? NSNumber)?.intValue ?? Int.max
            guard size<=1024*1024 else { throw NSError(domain:"PaperUIFixture",code:1,userInfo:[NSLocalizedDescriptionKey:"Unbounded exported private PNG"]) }
            let data = try Data(contentsOf:photo)
            guard data.count<=1024*1024,let source=CGImageSourceCreateWithData(data as CFData,nil),
                let image=CGImageSourceCreateImageAtIndex(source,0,nil),image.width==128,image.height==128 else {
                throw NSError(domain:"PaperUIFixture",code:1,userInfo:[NSLocalizedDescriptionKey:"Missing bounded exported private128 PNG"])
            }
            return data
        }
        let sourceAttachment = XCTAttachment(data:raw,uniformTypeIdentifier:"public.png")
        sourceAttachment.name="paper-authored-import-source-"+theme;sourceAttachment.lifetime = .keepAlways;add(sourceAttachment)
        #else
        let controls = documentExportControls(in: app)
        let filename = controls.filename, save = controls.commit
        workspaceActivate(app.cells["DOC.sidebar.item.On My iPad"])
        XCTAssertFalse(app.cells[folder+", Folder"].exists,"Only a fresh owned provider folder")
        workspaceActivate(app.buttons["New Folder"])
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout:10))
        app.typeKey("a",modifierFlags:.command);app.typeText(folder+"\n")
        XCTAssertTrue(app.otherElements["DOC.browsingRoot Source: com.apple.FileProvider.LocalStorage, Title: "+folder].waitForExistence(timeout:15))
        replaceEditorText(filename,with:fileName,in:app)
        dismissEditorKeyboard(in:app)
        workspaceActivate(save);XCTAssertTrue(save.waitForNonExistence(timeout:30))
        let provider = XCTAttachment(string:"Owned provider folder: "+folder+"; one normal exported PaperScan.png; retain for exact scoped cleanup")
        provider.name="paper-provider-owned-folder-"+theme;provider.lifetime = .keepAlways;add(provider)
        #endif
        XCTAssertEqual(app.state,.runningForeground)
        createDrawing()
        let painter = app.buttons["workspace-switch-builtin:workspace:painter"]
        workspaceActivate(painter)
        expectation(for:NSPredicate(format:"selected == YES"),evaluatedWith:painter);waitForExpectations(timeout:15)
        let texture = app.buttons["property-texture"]
        let chooser = app.descendants(matching:.any)["texture-dialog"].firstMatch
        let footer = app.buttons["layer-properties-disclosure"]
        func properties() {
            if !footer.exists { workspaceActivate(app.buttons["Layers panel"]) }
            XCTAssertTrue(footer.waitForExistence(timeout:10))
            if !texture.exists {
                let thumbnail=app.buttons["layer-thumbnail-2-content"]
                if !thumbnail.exists { workspaceActivate(footer) }
                XCTAssertTrue(thumbnail.waitForExistence(timeout:10));workspaceActivate(thumbnail)
                let cancel=app.buttons["color-cancel"]
                XCTAssertTrue(cancel.waitForExistence(timeout:10));workspaceActivate(cancel)
                XCTAssertTrue(cancel.waitForNonExistence(timeout:10));workspaceActivate(footer)
            }
            XCTAssertTrue(texture.waitForExistence(timeout:10))
        }
        func openChooser() {
            properties()
            revealEditorControl(texture,in:app.scrollViews.containing(.any,identifier:"layer-properties").firstMatch)
            workspaceActivate(texture);XCTAssertTrue(chooser.waitForExistence(timeout:10))
        }
        func closeChooser(_ button: String = "texture-cancel") {
            workspaceActivate(app.buttons[button]);XCTAssertTrue(chooser.waitForNonExistence(timeout:10))
        }
        func tile(_ label: String) -> XCUIElement {
            app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","texture-tile-",label)).firstMatch
        }
        func revealTile(_ tile: XCUIElement) {
            revealEditorControl(tile,in:app.scrollViews.containing(.button,identifier:tile.identifier).firstMatch)
        }
        func useTile(_ item: XCUIElement) {
            workspaceActivate(item)
            expectation(for:NSPredicate(format:"selected == YES"),evaluatedWith:item);waitForExpectations(timeout:10)
            closeChooser("texture-apply")
        }
        func chooseTile(_ label: String) {
            openChooser();let item=tile(label);XCTAssertTrue(item.waitForExistence(timeout:10));revealTile(item)
            useTile(item);expect(texture,value:label)
        }
        func context(_ label: String, action: String) {
            openChooser();let item=tile(label);XCTAssertTrue(item.waitForExistence(timeout:10));revealTile(item)
            #if os(macOS)
            item.rightClick();let command=app.menuItems[action].firstMatch
            #else
            item.press(forDuration:0.8);let command=app.buttons[action].firstMatch
            #endif
            XCTAssertTrue(command.waitForExistence(timeout:10));workspaceActivate(command)
            XCTAssertTrue(chooser.exists,"Tile actions keep Choose Texture open")
        }
        func sample() -> Data { editorPixels(in:app,at:CGPoint(x:0.5,y:0.65)) }
        func settledSample() -> Data {
            let began=Date()
            var previous=sample(),stableSince=Date()
            expectation(for:NSPredicate { _,_ in
                let next=sample()
                if next != previous { previous=next;stableSince=Date() }
                return Date().timeIntervalSince(began)>2 && Date().timeIntervalSince(stableSince)>0.4
            },evaluatedWith:app)
            waitForExpectations(timeout:20)
            return previous
        }
        func samePixels(_ expected: Data) {
            expectation(for:NSPredicate { _,_ in sample()==expected },evaluatedWith:app);waitForExpectations(timeout:20)
        }
        func noError() {
            XCTAssertFalse(app.staticTexts["Canvas error"].exists);XCTAssertFalse(app.alerts.firstMatch.exists)
        }
        func captureState(_ phase: String) {
            let hierarchy=XCTAttachment(string:app.debugDescription)
            hierarchy.name="paper-"+phase+"-hierarchy-"+theme;hierarchy.lifetime = .keepAlways;add(hierarchy)
            let shot=XCTAttachment(screenshot:app.screenshot())
            shot.name="paper-"+phase+"-"+theme;shot.lifetime = .keepAlways;add(shot)
        }
        properties();expect(texture,value:"Fine tooth")
        let original=settledSample()
        openChooser();workspaceActivate(app.buttons["texture-import"]);XCTAssertTrue(chooser.waitForNonExistence(timeout:10))
        #if os(macOS)
        let open=app.windows.buttons["OKButton"].firstMatch
        XCTAssertTrue(open.waitForExistence(timeout:20));captureState("import-cancel-chooser")
        app.typeKey(XCUIKeyboardKey.escape.rawValue,modifierFlags:[])
        XCTAssertTrue(open.waitForNonExistence(timeout:15))
        #else
        let cancel=app.buttons["Cancel"].firstMatch
        XCTAssertTrue(cancel.waitForExistence(timeout:20));captureState("import-cancel-chooser");workspaceActivate(cancel)
        XCTAssertTrue(cancel.waitForNonExistence(timeout:15))
        #endif
        properties();expect(texture,value:"Fine tooth");samePixels(original);noError()
        openChooser();workspaceActivate(app.buttons["texture-import"]);XCTAssertTrue(chooser.waitForNonExistence(timeout:10))
        #if os(macOS)
        XCTAssertTrue(open.waitForExistence(timeout:20));goToFolder(photo,in:app)
        expectation(for:NSPredicate(format:"enabled == YES"),evaluatedWith:open);waitForExpectations(timeout:15)
        captureState("import-file-selected")
        workspaceActivate(open);XCTAssertTrue(open.waitForNonExistence(timeout:20))
        #else
        workspaceActivate(app.cells["DOC.sidebar.item.On My iPad"])
        workspaceActivate(app.cells[folder+", Folder"])
        XCTAssertTrue(app.otherElements["DOC.browsingRoot Source: com.apple.FileProvider.LocalStorage, Title: "+folder].waitForExistence(timeout:15))
        let file=fileProviderFile(fileName+".png",in:app).images.firstMatch
        XCTAssertTrue(file.waitForExistence(timeout:15));captureState("import-file-selected");workspaceActivate(file)
        XCTAssertTrue(file.waitForNonExistence(timeout:20))
        #endif
        properties();expect(texture,value:fileName);noError()
        openChooser()
        let importedTile=tile(fileName)
        XCTAssertTrue(importedTile.waitForExistence(timeout:20));revealTile(importedTile)
        XCTAssertTrue(importedTile.isSelected)
        let thumbnailBefore=try await ordinaryPaperThumbnail(importedTile,name:"paper-native-import-preview-"+theme)
        useTile(importedTile)
        let imported=settledSample()
        editorMenu(in:app,menu:"Edit",id:"undo",label:"Undo");properties();expect(texture,value:"Fine tooth")
        editorMenu(in:app,menu:"Edit",id:"redo",label:"Redo");properties();expect(texture,value:fileName)
        samePixels(imported)
        context(fileName,action:"Rename…")
        let dialog=app.descendants(matching:.any)["paper-name-dialog"].firstMatch
        let name=app.textFields["paper-library-name"]
        XCTAssertTrue(dialog.waitForExistence(timeout:10))
        replaceEditorText(name,with:"Cancelled",in:app)
        #if os(iOS)
        dismissEditorKeyboard(in:app)
        #endif
        workspaceActivate(dialog.buttons["Cancel"]);XCTAssertTrue(dialog.waitForNonExistence(timeout:10))
        XCTAssertTrue(tile(fileName).exists);closeChooser()
        expect(texture,value:fileName);samePixels(imported)
        context(fileName,action:"Rename…")
        XCTAssertTrue(dialog.waitForExistence(timeout:10));replaceEditorText(name,with:" ",in:app)
        #if os(iOS)
        dismissEditorKeyboard(in:app)
        #endif
        workspaceActivate(app.buttons["paper-name-save"])
        let renameError=app.staticTexts["paper-name-error"]
        XCTAssertTrue(renameError.waitForExistence(timeout:10));XCTAssertEqual(editorText(renameError),"Enter a name for the paper")
        XCTAssertTrue(dialog.exists)
        captureState("rename-invalid")
        replaceEditorText(name,with:"Rag",in:app)
        #if os(iOS)
        dismissEditorKeyboard(in:app)
        #endif
        workspaceActivate(app.buttons["paper-name-save"]);XCTAssertTrue(dialog.waitForNonExistence(timeout:20))
        XCTAssertTrue(tile("Rag").waitForExistence(timeout:10),"The open chooser follows the renamed tile")
        XCTAssertTrue(tile("Rag").isSelected,"The pending choice keeps its renamed tile");closeChooser()
        expect(texture,value:"Rag");samePixels(imported);noError()
        context("Rag",action:"Delete")
        let retained=tile("This drawing’s paper")
        XCTAssertTrue(retained.waitForExistence(timeout:10));XCTAssertFalse(tile("Rag").exists)
        XCTAssertTrue(retained.isSelected,"The pending choice falls back to the current tile");closeChooser()
        expect(texture,value:"This drawing’s paper");samePixels(imported)
        openChooser();XCTAssertFalse(tile("Rag").exists)
        XCTAssertTrue(retained.exists);XCTAssertTrue(retained.isSelected);captureState("deleted-material-retained")
        useTile(retained)
        context("This drawing’s paper",action:"Add to Library");closeChooser()
        expectation(for:NSPredicate { _,_ in
            let value=texture.value as? String ?? "";return !value.isEmpty && value != "This drawing’s paper"
        },evaluatedWith:texture);waitForExpectations(timeout:20)
        let addedName=try XCTUnwrap(texture.value as? String)
        samePixels(imported);noError()
        openChooser();let added=tile(addedName);XCTAssertTrue(added.waitForExistence(timeout:15));revealTile(added)
        XCTAssertTrue(added.isSelected)
        let thumbnailAfter=try await ordinaryPaperThumbnail(added,name:"paper-native-added-preview-"+theme)
        XCTAssertEqual(thumbnailAfter,thumbnailBefore,"Add retains the same visible imported material thumbnail pixels")
        useTile(added)
        chooseTile("Rough");editorMenu(in:app,menu:"Edit",id:"undo",label:"Undo")
        properties();expect(texture,value:addedName)
        editorMenu(in:app,menu:"Edit",id:"redo",label:"Redo");properties();expect(texture,value:"Rough")
        chooseTile(addedName)
        attachEditor(in:app,name:"paper-native-library-actions-"+theme)
        createDrawing()
        for command in ["Undo","Redo"] {
            let history=app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","toolbar-tile-commands-",command)).firstMatch
            XCTAssertTrue(history.exists);XCTAssertFalse(history.isEnabled,"New drawing starts with empty authored history")
        }
        workspaceActivate(painter);properties();expect(texture,value:addedName)
        openChooser();XCTAssertTrue(tile(addedName).isSelected)
        _=try await ordinaryPaperThumbnail(tile(addedName),name:"paper-native-warm-new-preview-"+theme)
        useTile(tile(addedName))
        noError();attachEditor(in:app,name:"paper-native-remembered-warm-new-"+theme)
        let scope=XCTAttachment(string:"Ordinary full app, real native menus/context actions/system chooser/provider; exact displayed crops/thumbnail samples and native values only. Add stores current Paper Image material. New is warm; cold Load/startup, SourceImage/package byte equality, physical Pencil/present/performance remain separate gates. Own input token: "+token)
        scope.name="paper-native-scope-"+theme;scope.lifetime = .keepAlways;add(scope)
    }

    @MainActor private func ordinaryPaperThumbnail(_ tile: XCUIElement, name: String) async throws -> Data {
        let shot=tile.screenshot()
        let attachment=XCTAttachment(screenshot:shot);attachment.name=name;attachment.lifetime = .keepAlways;add(attachment)
        let encoded=shot.pngRepresentation
        return try await paperUIFileWork {
            guard encoded.count<=4*1024*1024,let source=CGImageSourceCreateWithData(encoded as CFData,nil),
                let properties=CGImageSourceCopyPropertiesAtIndex(source,0,nil) as? [CFString:Any],
                let width=properties[kCGImagePropertyPixelWidth] as? Int,let height=properties[kCGImagePropertyPixelHeight] as? Int,
                width<=1024,height<=1024,
                let image=CGImageSourceCreateThumbnailAtIndex(source,0,[kCGImageSourceCreateThumbnailFromImageAlways:true,
                    kCGImageSourceCreateThumbnailWithTransform:true,kCGImageSourceThumbnailMaxPixelSize:max(width,height)] as CFDictionary),
                let crop=image.cropping(to:CGRect(x:Double(image.width)*0.2,y:Double(image.height)*0.1,
                    width:Double(image.width)*0.6,height:Double(image.height)*0.5)) else {
                throw NSError(domain:"PaperUIFixture",code:2,userInfo:[NSLocalizedDescriptionKey:"Missing bounded native Paper thumbnail capture"])
            }
            var bytes=Data(count:crop.width*crop.height*4)
            try bytes.withUnsafeMutableBytes { buffer in
                guard let context=CGContext(data:buffer.baseAddress,width:crop.width,height:crop.height,bitsPerComponent:8,
                    bytesPerRow:crop.width*4,space:CGColorSpace(name:CGColorSpace.sRGB)!,
                    bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue) else {
                    throw NSError(domain:"PaperUIFixture",code:3,userInfo:[NSLocalizedDescriptionKey:"Native Paper preview decode"])
                }
                context.draw(crop,in:CGRect(x:0,y:0,width:crop.width,height:crop.height))
            }
            return bytes
        }
    }
}

extension XCTestCase {
    @MainActor func checkRememberedPaperColdStart(in app: XCUIApplication, theme: String) async throws {
        let storage = try XCTUnwrap(ProcessInfo.processInfo.environment["CAPY_UI_DRAWING_STORAGE_" + theme.uppercased()],
            "Stage a fresh settings+papers-only owned seed; missing seed is an unmet gate")
        let name = URL(fileURLWithPath:storage).lastPathComponent
        XCTAssertTrue(name.hasPrefix("capy-test-drawing-paper-" + theme + "-"))
        guard name.hasPrefix("capy-test-drawing-paper-" + theme + "-") else { return }
        app.launchEnvironment["CAPY_STORAGE_DIR"] = storage
        app.launchEnvironment.removeValue(forKey:"CAPY_INITIAL_ACTIONS")
        app.launch()
        let canvas = app.descendants(matching:.any)["canvas"].firstMatch
        expectation(for:NSPredicate(format:"value == %@","Canvas ready"),evaluatedWith:canvas)
        waitForExpectations(timeout:60)
        let first = XCTAttachment(screenshot:app.screenshot())
        first.name="paper-cold-first-ready-before-controls-" + theme; first.lifetime = .keepAlways; add(first)
        let hierarchy = XCTAttachment(string:app.debugDescription)
        hierarchy.name="paper-cold-first-ready-hierarchy-" + theme; hierarchy.lifetime = .keepAlways; add(hierarchy)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        XCTAssertFalse(app.buttons["recovery-retry"].exists)
        XCTAssertFalse(app.buttons["recovery-later"].exists)
        for command in ["Undo","Redo"] {
            let history=app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","toolbar-tile-commands-",command)).firstMatch
            XCTAssertTrue(history.exists);XCTAssertFalse(history.isEnabled,"Settings+papers-only startup has no authored history")
        }
        let painter=app.buttons["workspace-switch-builtin:workspace:illustrator"]
        XCTAssertTrue(painter.waitForExistence(timeout:10));workspaceActivate(painter)
        expectation(for:NSPredicate(format:"selected == YES"),evaluatedWith:painter);waitForExpectations(timeout:15)
        let rows = artworkRows(in: app)
        XCTAssertEqual(rows.count, 2)
        let papers = rows.matching(NSPredicate(format: "label == %@", "Paper layer row"))
        XCTAssertEqual(papers.count, 1)
        let paper = papers.firstMatch
        XCTAssertTrue(paper.waitForExistence(timeout: 10))
        let paperName = paper.staticTexts["Paper"].firstMatch
        let list = app.scrollViews["layer-rows"]
        revealEditorControl(paperName, in: list); workspaceActivate(paperName)
        let selected = paper.buttons["layer-Select layer without changing drawing target"]
        let checked = list.buttons.matching(NSPredicate(format: "identifier == %@ AND selected == YES", "layer-Select layer without changing drawing target"))
        expectation(for: NSPredicate { _, _ in selected.isSelected && checked.count == 1 }, evaluatedWith: paper)
        waitForExpectations(timeout: 10)
        expectation(for: NSPredicate(format: "label == %@ OR value == %@", "Paper", "Paper"), evaluatedWith: app.staticTexts["properties-layer-name"])
        waitForExpectations(timeout: 10)
        let texture = app.buttons["property-texture"]
        XCTAssertTrue(texture.waitForExistence(timeout:10), app.debugDescription)
        expectation(for:NSPredicate(format:"value == %@","Cold paper"),evaluatedWith:texture);waitForExpectations(timeout:20)
        revealEditorControl(texture,in:app.scrollViews.containing(.any,identifier:"layer-properties").firstMatch)
        workspaceActivate(texture)
        let chooser=app.descendants(matching:.any)["texture-dialog"].firstMatch
        XCTAssertTrue(chooser.waitForExistence(timeout:10))
        let tile=app.buttons.matching(NSPredicate(format:"identifier BEGINSWITH %@ AND label == %@","texture-tile-","Cold paper")).firstMatch
        XCTAssertTrue(tile.waitForExistence(timeout:15))
        revealEditorControl(tile,in:app.scrollViews.containing(.button,identifier:tile.identifier).firstMatch)
        XCTAssertTrue(tile.isSelected)
        _=try await ordinaryPaperThumbnail(tile,name:"paper-cold-library-preview-" + theme)
        workspaceActivate(app.buttons["texture-apply"]);XCTAssertTrue(chooser.waitForNonExistence(timeout:10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists);XCTAssertFalse(app.alerts.firstMatch.exists)
        let scope=XCTAttachment(string:"Fresh process and fresh prelaunch settings+papers-only profile; initial raw screen precedes controls. Native value/selection/history coverage only; visually compare initial canvas and tile against retained shared seed PNG/96x96 preview. Canvas ready is not a first hardware-present acknowledgment. Explicit New after this initial load is warm and covered separately. Storage input: " + storage)
        scope.name="paper-cold-scope-" + theme;scope.lifetime = .keepAlways;add(scope)
    }
}
