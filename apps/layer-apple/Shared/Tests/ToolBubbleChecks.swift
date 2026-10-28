import XCTest

extension XCTestCase {
    @MainActor func checkToolBubbles(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:painter"}}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 60)
        let painter = app.buttons["workspace-switch-builtin:workspace:painter"]
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: painter)
        waitForExpectations(timeout: 30)
        let viewport = workspaceViewport(in: app)
        func destination(_ x: CGFloat, _ y: CGFloat) -> XCUICoordinate {
            viewport.coordinate(withNormalizedOffset: CGVector(dx: x, dy: y))
        }
        func take(_ source: XCUIElement, _ x: CGFloat, _ y: CGFloat) {
            XCTAssertTrue(source.waitForExistence(timeout: 10))
            source.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.85))
                .press(forDuration: 0.05, thenDragTo: destination(x, y), withVelocity: .slow, thenHoldForDuration: 0.1)
        }
        func faces() -> XCUIElementQuery {
            app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "bubble-face-"))
        }
        func menu(_ source: XCUIElement) {
            #if os(macOS)
            source.rightClick()
            #else
            source.press(forDuration: 1.2)
            #endif
            XCTAssertTrue(app.descendants(matching: .any)["bubble-context-menu"].firstMatch.waitForExistence(timeout: 10))
        }
        let brushSource = app.buttons["header-tool-drawing_brush"]
        take(brushSource, 0.32, 0.42)
        expectation(for: NSPredicate { _, _ in faces().count > 0 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let brush = faces().firstMatch
        XCTAssertTrue(brush.isSelected)
        expectation(for: NSPredicate { _, _ in
            #if os(macOS)
            let reference = app.windows.firstMatch.frame
            #else
            let reference = canvas.frame
            #endif
            let frame = brush.frame
            let points = [20.0, 32.0].map { distance in
                CGPoint(x: (frame.midX + distance - reference.minX) / reference.width,
                    y: (frame.midY - reference.minY) / reference.height)
            }
            let pixels = self.editorPixelSamples(in: app, at: points, size: 1)
            return (0..<3).map { abs(Int(pixels[0][$0]) - Int(pixels[1][$0])) }.max()! > 12
        }, evaluatedWith: brush)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "bubble-brush-halo-\(theme)")
        menu(brush)
        let putBack = app.buttons["menu-action-Put back"]
        XCTAssertTrue(putBack.waitForExistence(timeout: 5))
        workspaceActivate(putBack)
        expectation(for: NSPredicate { _, _ in faces().count == 0 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        take(brushSource, 0.32, 0.42)
        XCTAssertTrue(brush.waitForExistence(timeout: 10))
        let retainedBrushID = brush.identifier
        let colorSource = app.buttons.matching(NSPredicate(format: "label == %@", "Brush color")).firstMatch
        take(colorSource, 0.68, 0.42)
        let wheel = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "bubble-wheel-")).firstMatch
        XCTAssertTrue(wheel.waitForExistence(timeout: 10))
        let wheelPoint = wheel.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.12))
        wheelPoint.press(forDuration: 0.05, thenDragTo: wheel.coordinate(withNormalizedOffset: CGVector(dx: 0.85, dy: 0.5)),
            withVelocity: .slow, thenHoldForDuration: 0.1)
        menu(wheel)
        let recent = app.buttons["menu-action-Recent colors"], pair = app.buttons["menu-action-Primary and secondary"]
        XCTAssertTrue(recent.waitForExistence(timeout: 5)); workspaceActivate(recent)
        XCTAssertTrue(pair.waitForExistence(timeout: 5), "Changing one option retains the menu")
        workspaceActivate(pair)
        XCTAssertTrue(putBack.exists, "Several option edits retain Put back")
        workspaceActivate(putBack)
        XCTAssertTrue(wheel.waitForNonExistence(timeout: 10))
        take(colorSource, 0.68, 0.42)
        let background = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND identifier ENDSWITH %@", "bubble-bead-", "-background-0")).firstMatch
        let foreground = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND identifier ENDSWITH %@", "bubble-bead-", "-foreground-0")).firstMatch
        XCTAssertTrue(background.waitForExistence(timeout: 10))
        XCTAssertTrue(foreground.waitForExistence(timeout: 10))
        expectation(for: NSPredicate { _, _ in
            #if os(macOS)
            let reference = app.windows.firstMatch.frame
            #else
            let reference = canvas.frame
            #endif
            let back = background.frame, front = foreground.frame
            let dx = back.midX - front.midX, dy = back.midY - front.midY, distance = max(1, hypot(dx, dy))
            let point = CGPoint(x: (back.midX + dx / distance * back.width / 4 - reference.minX) / reference.width,
                y: (back.midY + dy / distance * back.width / 4 - reference.minY) / reference.height)
            let pixel = self.editorPixels(in: app, at: point, size: 1)
            return (0..<3).allSatisfy { pixel[$0] >= 250 }
        }, evaluatedWith: background)
        waitForExpectations(timeout: 10)
        menu(wheel); workspaceActivate(putBack)
        XCTAssertTrue(wheel.waitForNonExistence(timeout: 10))
        let layersSource = app.buttons.matching(NSPredicate(format: "identifier == %@ AND label == %@", "header-tool-panel", "Layers panel")).firstMatch
        take(layersSource, 0.68, 0.5)
        let card = app.descendants(matching: .any)["layer-properties-pages"].firstMatch
        XCTAssertTrue(card.waitForExistence(timeout: 10))
        let add = app.buttons["bubble-new-layer"], properties = app.buttons["layer-properties-disclosure"]
        XCTAssertTrue(add.waitForExistence(timeout: 5))
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let initial = rows.count
        XCTAssertGreaterThan(initial, 0, "The card exposes its layer rows")
        XCTAssertTrue(add.isEnabled)
        workspaceActivate(add)
        let added = XCTAttachment(string: app.debugDescription)
        added.name = "bubble-layers-after-add-\(theme)"; self.add(added)
        expectation(for: NSPredicate { _, _ in rows.count > initial }, evaluatedWith: card)
        waitForExpectations(timeout: 10)
        workspaceActivate(properties)
        expectation(for: NSPredicate(format: "label == %@", "All layers"), evaluatedWith: properties)
        waitForExpectations(timeout: 10)
        workspaceActivate(properties)
        XCTAssertTrue(add.waitForExistence(timeout: 10))
        attachEditor(in: app, name: "bubble-layers-card-\(theme)")
        destination(0.48, 0.7).tap()
        XCTAssertTrue(card.waitForNonExistence(timeout: 10), "An outside contact folds the card")
        let brushFace = app.descendants(matching: .any)[retainedBrushID].firstMatch
        let bubble = app.descendants(matching: .any)["tool-bubble-" + String(retainedBrushID.dropFirst("bubble-face-".count))].firstMatch
        let ringLabels = ["Brush size", "Strength", "Texture strength", "Material hardness", "Tip angle", "Opacity"]
        func haloLabels() -> [String] {
            bubble.descendants(matching: .any).matching(NSPredicate(format: "label IN %@", ringLabels))
                .allElementsBoundByIndex.map(\.label).sorted()
        }
        func expect(_ condition: @escaping () -> Bool) {
            expectation(for: NSPredicate { _, _ in condition() }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        XCTAssertTrue(brushFace.isSelected)
        let enabledHalos = haloLabels()
        XCTAssertFalse(enabledHalos.isEmpty, "The selected brush shows its slider halo")
        let popup = app.descendants(matching: .any)["bubble-context-menu"].firstMatch
        let showSliders = app.buttons["menu-action-Show Sliders"]
        func toggleSliders(from selected: Bool) {
            menu(brushFace)
            XCTAssertTrue(showSliders.waitForExistence(timeout: 5))
            XCTAssertEqual(showSliders.isSelected, selected)
            workspaceActivate(showSliders)
            expect { popup.exists && showSliders.exists && showSliders.isSelected == !selected }
            destination(0.95, 0.9).clickOrTap()
            XCTAssertTrue(popup.waitForNonExistence(timeout: 10))
        }
        toggleSliders(from: true)
        expect { brushFace.exists && brushFace.isSelected && haloLabels().isEmpty }
        toggleSliders(from: false)
        expect { brushFace.exists && brushFace.isSelected && haloLabels() == enabledHalos }
        workspaceActivate(app.descendants(matching: .any)["zen-button"].firstMatch)
        expect { !brushSource.exists || !brushSource.isEnabled }
        XCTAssertGreaterThan(faces().count, 0, "Bubbles remain accessible in Zen")
        attachEditor(in: app, name: "bubble-zen-\(theme)")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}

enum HDRBubbleJourney { case controls, textEntry }

extension XCTestCase {
    @MainActor func checkHDRToolBubbles(in app: XCUIApplication, theme: String, depth: Int, journey: HDRBubbleJourney = .controls) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:illustrator"}},{"type":"invoke","command":"new_document"}]"#
        app.launch()
        createNewDrawing(width: "2048", height: "1536", depth: depth, in: app)
        capturePaintEditor(in: app, theme: theme)
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 60)
        XCTAssertTrue(app.buttons["screen-status"].waitForExistence(timeout: 15), "The actual Float drawing exposes its HDR presentation")
        let sketch = app.buttons["workspace-switch-builtin:workspace:painter"]
        workspaceActivate(sketch)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: sketch)
        waitForExpectations(timeout: 10)
        let viewport = workspaceViewport(in: app)
        let source = app.buttons.matching(NSPredicate(format: "label == %@", "Brush color")).firstMatch
        XCTAssertTrue(source.waitForExistence(timeout: 15))
        func take() {
            source.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.85))
                .press(forDuration: 0.05, thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: 0.58, dy: 0.42)),
                    withVelocity: .slow, thenHoldForDuration: 0.1)
        }
        take()
        let slider = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "bubble-intensity-")).firstMatch
        let wheel = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "bubble-wheel-")).firstMatch
        XCTAssertTrue(slider.waitForExistence(timeout: 15))
        XCTAssertTrue(wheel.exists)
        let nativeControl = XCTAttachment(string: "wheel=\(wheel.frame) slider=\(slider.frame) value=\(String(reflecting: slider.value))\n\(slider.debugDescription)")
        nativeControl.name = "hdr-native-intensity-\(theme)"; nativeControl.lifetime = .keepAlways; add(nativeControl)
        XCTAssertEqual(wheel.frame.width, 202, accuracy: 1, "Wheel accessibility keeps the shared drawable bounds inside the plate")
        XCTAssertEqual(wheel.frame.height, 202, accuracy: 1)
        XCTAssertLessThan(slider.frame.height, wheel.frame.height / 2, "Intensity has its own tight accessible bounds")
        XCTAssertFalse(wheel.descendants(matching: .any)[slider.identifier].firstMatch.exists,
            "The independent intensity control is a sibling of the wheel")
        XCTAssertTrue(slider.isEnabled)
        func value() -> Double? {
            if let value = slider.value as? NSNumber { return value.doubleValue }
            guard let text = slider.value as? String, let token = text.split(separator: " ").first else { return nil }
            return Double(token)
        }
        func expectEV(_ expected: Double, _ message: String) {
            expectation(for: NSPredicate { _, _ in value().map { abs($0 - expected) < 0.035 } ?? false }, evaluatedWith: slider)
            waitForExpectations(timeout: 10)
            XCTAssertEqual(value() ?? .nan, expected, accuracy: 0.035, message)
        }
        func arcPoint() -> XCUICoordinate { slider.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.82)) }
        expectEV(0, "A new Float drawing starts at zero EV")
        let start = arcPoint(), end = slider.coordinate(withNormalizedOffset: CGVector(dx: 0.86, dy: 0.35))
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.1)
        expectation(for: NSPredicate { _, _ in (value() ?? 0) > 2.5 }, evaluatedWith: slider)
        waitForExpectations(timeout: 10)
        #if os(iOS)
        let beforeArrow = value() ?? .nan
        app.typeKey(XCUIKeyboardKey.rightArrow.rawValue, modifierFlags: [])
        expectEV(beforeArrow + 0.1, "Right Arrow edits the focused arc")
        app.typeKey(XCUIKeyboardKey.rightArrow.rawValue, modifierFlags: [])
        expectEV(beforeArrow + 0.2, "Right Arrow retains focus after the model update")
        arcPoint().doubleTap()
        expectEV(0, "A native double contact resets intensity before the tool shortcuts")
        #else
        app.typeKey(XCUIKeyboardKey.home.rawValue, modifierFlags: [])
        expectEV(0, "Home resets the focused native arc")
        #endif
        app.typeKey(XCUIKeyboardKey.rightArrow.rawValue, modifierFlags: [])
        expectEV(0.1, "Right Arrow nudges the focused native arc")
        #if os(iOS)
        let eraser = app.buttons["header-tool-eraser"]
        app.typeKey("e", modifierFlags: [])
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: eraser)
        waitForExpectations(timeout: 5)
        expectEV(0.1, "An unhandled tool shortcut preserves the intensity value")
        app.typeKey("p", modifierFlags: [])
        expectation(for: NSPredicate(format: "selected == NO"), evaluatedWith: eraser)
        waitForExpectations(timeout: 5)
        if case .textEntry = journey {
            editorMenu(in: app, menu: "File", id: "new_document", label: "New drawing")
            let name = app.textFields["new-drawing-name"]
            replaceEditorText(name, with: "Hdr", in: app)
            app.typeKey("e", modifierFlags: [])
            expectation(for: NSPredicate { _, _ in (name.value as? String)?.lowercased() == "hdre" }, evaluatedWith: name)
            waitForExpectations(timeout: 5)
            dismissEditorKeyboard(in: app)
            workspaceActivate(app.buttons["new-drawing-cancel"])
            XCTAssertTrue(app.buttons["new-drawing-create"].waitForNonExistence(timeout: 10))
            XCTAssertFalse(eraser.isSelected, "Text entry must not execute the canvas Eraser shortcut")
            expectEV(0.1, "Cancelling the text dialog preserves the original HDR drawing")
            return
        }
        #endif
        #if os(macOS)
        arcPoint().doubleClick()
        #else
        arcPoint().doubleTap()
        #endif
        expectEV(0, "A native double contact resets intensity")
        arcPoint().clickOrTap()
        expectEV(2, "The arc midpoint selects two EV")
        let committed = value()
        XCTAssertNotNil(committed)
        wheel.coordinate(withNormalizedOffset: CGVector(dx: 0.55, dy: 0.45)).clickOrTap()
        expectEV(committed ?? .nan, "Wheel input preserves its committed EV")
        #if os(iOS)
        let originalWidth = wheel.frame.width
        wheel.pinch(withScale: 1.25, velocity: 1)
        expectation(for: NSPredicate { _, _ in wheel.frame.width > originalWidth + 8 }, evaluatedWith: wheel)
        waitForExpectations(timeout: 10)
        expectEV(committed ?? .nan, "A native HDR bubble pinch preserves its committed EV")
        #endif
        #if os(macOS)
        arcPoint().rightClick()
        #else
        arcPoint().press(forDuration: 1.2)
        #endif
        XCTAssertTrue(app.descendants(matching: .any)["bubble-context-menu"].firstMatch.waitForExistence(timeout: 10))
        let recent = app.buttons["menu-action-Recent colors"], pair = app.buttons["menu-action-Primary and secondary"]
        XCTAssertTrue(recent.waitForExistence(timeout: 5)); workspaceActivate(recent)
        XCTAssertTrue(pair.waitForExistence(timeout: 5)); workspaceActivate(pair)
        let putBack = app.buttons["menu-action-Put back"]
        XCTAssertTrue(putBack.exists)
        workspaceActivate(putBack)
        XCTAssertTrue(slider.waitForNonExistence(timeout: 10))
        take()
        XCTAssertTrue(slider.waitForExistence(timeout: 10))
        #if os(macOS)
        arcPoint().doubleClick()
        #else
        arcPoint().doubleTap()
        #endif
        expectEV(0, "Resetting the restored arc precedes the disabled-input check")
        let transparent = app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND identifier ENDSWITH %@", "bubble-bead-", "-transparent-0")).firstMatch
        XCTAssertTrue(transparent.waitForExistence(timeout: 10))
        workspaceActivate(transparent)
        expectation(for: NSPredicate(format: "enabled == NO"), evaluatedWith: slider)
        waitForExpectations(timeout: 10)
        let disabledEV = value()
        arcPoint().clickOrTap()
        expectEV(disabledEV ?? .nan, "Transparent paint disables intensity editing")
        attachEditor(in: app, name: "hdr-bubble-\(depth == 2 ? "F16" : "F32")-\(theme)")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}

extension EditorLaunchTests {
    #if os(iOS)
    @MainActor func testHDRToolBubbleTextEntry() { checkHDRToolBubbles(in: editorCaptureApplication(), theme: "light", depth: 2, journey: .textEntry) }
    @MainActor func testHDRToolBubbleTextEntryDark() { checkHDRToolBubbles(in: editorCaptureApplication(), theme: "dark", depth: 2, journey: .textEntry) }
    #endif
    @MainActor func testHDRToolBubbles() { checkHDRToolBubbles(in: editorCaptureApplication(), theme: "light", depth: 2) }
    @MainActor func testHDRToolBubblesDark() { checkHDRToolBubbles(in: editorCaptureApplication(), theme: "dark", depth: 2) }
    @MainActor func testHDRToolBubblesF32() { checkHDRToolBubbles(in: editorCaptureApplication(), theme: "light", depth: 3) }
    @MainActor func testHDRToolBubblesF32Dark() { checkHDRToolBubbles(in: editorCaptureApplication(), theme: "dark", depth: 3) }
}
