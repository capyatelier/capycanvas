import XCTest
import CoreGraphics

extension XCTestCase {
    @MainActor func editorMenu(in app: XCUIApplication, menu: String, id: String, label: String) {
        #if os(macOS)
        workspaceActivate(app.menuBars.menuBarItems[menu])
        workspaceActivate(app.menuItems[label].firstMatch)
        #else
        let button = app.buttons["menu-" + menu]
        XCTAssertTrue(button.waitForExistence(timeout: 5))
        // Keep artwork checks below the simulator's reserved window-control edge.
        button.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.85)).tap()
        XCTAssertEqual(button.value as? String, "Expanded", "Open the requested \(menu) menu")
        let item = app.buttons["command-" + id]
        workspaceActivate(item)
        XCTAssertTrue(item.waitForNonExistence(timeout: 5))
        #endif
    }

    @MainActor private func revealTransform(_ element: XCUIElement, in app: XCUIApplication) {
        revealEditorControl(element, in: app.scrollViews.containing(.button, identifier: "number-value-tool-transform_x").firstMatch)
    }
    @MainActor private func editTransform(_ id: String, _ expression: String, in app: XCUIApplication) {
        let entry = app.textFields["number-entry-tool-transform_" + id]
        if entry.exists {
            entry.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: (entry.value as? String ?? "").count))
        } else {
            let value = app.buttons["number-value-tool-transform_" + id]
            revealTransform(value, in: app); workspaceActivate(value)
        }
        XCTAssertTrue(entry.waitForExistence(timeout: 5))
        entry.typeText(expression + "\n")
    }
    @MainActor private func expectTransform(_ id: String, _ value: String, in app: XCUIApplication) {
        expectation(for: NSPredicate(format: "value == %@", value), evaluatedWith: app.buttons["number-value-tool-transform_" + id])
        waitForExpectations(timeout: 5)
    }

    @MainActor private func bluePaperBounds(in app: XCUIApplication) -> CGRect {
        // Locate the visible blue paper, without reading document geometry.
        let scan = stride(from: 0.2, through: 0.8, by: 0.001).map { CGFloat($0) }
        let horizontal = editorPixelSamples(in: app, at: scan.map { CGPoint(x: $0, y: 0.55) }, size: 1)
        let vertical = editorPixelSamples(in: app, at: scan.map { CGPoint(x: 0.5, y: $0) }, size: 1)
        func blue(_ p: Data) -> Bool { Int(p[2]) > Int(p[0]) + 50 }
        let xs = zip(scan, horizontal).filter { blue($0.1) }.map { $0.0 }
        let ys = zip(scan, vertical).filter { blue($0.1) }.map { $0.0 }
        guard let left = xs.first, let right = xs.last, let top = ys.first, let bottom = ys.last else {
            XCTFail("Filled paper must have visible bounds"); return .zero
        }
        let bounds = CGRect(x: left, y: top, width: right - left, height: bottom - top)
        XCTAssertGreaterThan(bounds.width, 0.15); XCTAssertGreaterThan(bounds.height, 0.15)
        return bounds
    }

    @MainActor private func expectBluePaper(_ expected: [Bool], at points: [CGPoint], in app: XCUIApplication) {
        XCTAssertEqual(expected.count, points.count)
        expectation(for: NSPredicate { _, _ in
            let actual = self.editorPixelSamples(in: app, at: points, size: 8)
            for (index, pixel) in actual.enumerated() {
                for i in stride(from: 0, to: pixel.count, by: 4) {
                    let red = Int(pixel[i]), green = Int(pixel[i + 1]), blue = Int(pixel[i + 2])
                    if expected[index] {
                        if blue <= red + 50 { return false }
                    } else if red <= 250 || green <= 250 || blue <= 250 { return false }
                }
            }
            return true
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
    }

    @MainActor func checkSelectionAndTransform(in app: XCUIApplication) {
        // Only choose the starting color. Selection, artwork and transforms
        // below are created through the same visible controls as normal use.
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch()
        capturePaintEditor(in: app)

        func menu(_ menu: String, _ id: String, _ label: String) {
            editorMenu(in: app, menu: menu, id: id, label: label)
        }
        func command(_ label: String) {
            let item = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-commands-", label)).firstMatch
            XCTAssertTrue(item.isEnabled)
            workspaceActivate(item)
        }
        func pixels() -> Data { editorPixels(in: app) }
        func expectPixels(_ expected: Data) {
            expectation(for: NSPredicate { _, _ in pixels() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        func attach(_ name: String) { attachEditor(in: app, name: name) }
        func value(_ id: String) -> XCUIElement { app.buttons["number-value-tool-transform_" + id] }
        func reveal(_ element: XCUIElement) { revealTransform(element, in: app) }
        func edit(_ id: String, _ expression: String) { editTransform(id, expression, in: app) }
        func expectField(_ id: String, _ value: String) { expectTransform(id, value, in: app) }

        let paper = pixels()
        menu("Select", "select_all", "Select all pixels")
        menu("Edit", "fill_selection", "Fill selection")
        expectation(for: NSPredicate { _, _ in let sample = pixels(); return Int(sample[2]) > Int(sample[0]) + 50 }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        let painted = pixels()
        XCTAssertNotEqual(painted, paper)
        attach("selection-filled")
        command("Undo"); expectPixels(paper)
        command("Redo"); expectPixels(painted)
        menu("Select", "deselect", "Deselect pixels")
        // Selection changes have their own Undo entry. Restore it before
        // checking that cancelling a transform adds no artwork history.
        command("Undo"); expectPixels(painted)

        for apply in [false, true] {
            menu("Edit", "scale_rotate", "Scale / rotate")
            XCTAssertTrue(value("x").waitForExistence(timeout: 5))
            let aspect = app.buttons["tool-action-transform_aspect"]
            reveal(aspect)
            if !aspect.isSelected { workspaceActivate(aspect) }
            XCTAssertTrue(aspect.isSelected)
            edit("width", "50")
            expectField("width", "50 %"); expectField("height", "50 %")
            reveal(aspect); workspaceActivate(aspect)
            XCTAssertFalse(aspect.isSelected)
            edit("height", "75")
            expectField("width", "50 %"); expectField("height", "75 %")
            // Zero scale is a semantic rejection, not a valid empty preview.
            edit("width", "0")
            XCTAssertTrue(app.staticTexts["number-error-tool-transform_width"].waitForExistence(timeout: 5))
            edit("width", "100")
            XCTAssertTrue(app.staticTexts["number-error-tool-transform_width"].waitForNonExistence(timeout: 5))
            edit("height", "100")
            edit("x", "4096")
            expectPixels(paper)
            attach(apply ? "transform-before-apply" : "transform-before-cancel")
            let finish = app.buttons["tool-action-" + (apply ? "apply_transform" : "cancel_transform")]
            reveal(finish); workspaceActivate(finish)
            XCTAssertTrue(value("x").waitForNonExistence(timeout: 5))
            if apply {
                expectPixels(paper)
                command("Undo"); expectPixels(painted)
                command("Redo"); expectPixels(paper)
                attach("transform-redone")
            } else {
                expectPixels(painted)
                // Cancel must leave the existing fill as the next Undo entry.
                command("Undo"); expectPixels(paper)
                command("Redo"); expectPixels(painted)
            }
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
    @MainActor func checkTransformRotationAndHandles(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        func blue(_ p: Data) -> Bool { Int(p[2]) > Int(p[0]) + 50 }
        expectation(for: NSPredicate { _, _ in blue(self.editorPixels(in: app)) }, evaluatedWith: app)
        waitForExpectations(timeout: 10)

        let bounds = bluePaperBounds(in: app)
        let viewport = workspaceViewport(in: app), originalFrame = viewport.frame
        let center = CGPoint(x: bounds.midX, y: bounds.midY)
        let rotationHeight = bounds.width * originalFrame.width / originalFrame.height
        let points = [CGPoint(x: bounds.minX + bounds.width * 0.1, y: center.y), center,
            CGPoint(x: center.x + bounds.width * 0.2, y: center.y),
            CGPoint(x: center.x, y: center.y - rotationHeight * 0.2),
            CGPoint(x: bounds.maxX - bounds.width * 0.1, y: center.y)]
        func expectInk(_ expected: [Bool]) {
            expectBluePaper(expected, at: points, in: app)
        }
        let filled = [true, true, true, true, true], blank = [false, false, false, false, false]
        func begin() {
            editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
            let aspect = app.buttons["tool-action-transform_aspect"]
            revealTransform(aspect, in: app)
            if aspect.isSelected { workspaceActivate(aspect) }
        }
        func finish(_ apply: Bool) {
            let button = app.buttons["tool-action-" + (apply ? "apply_transform" : "cancel_transform")]
            revealTransform(button, in: app); workspaceActivate(button)
            XCTAssertTrue(app.buttons["number-value-tool-transform_x"].waitForNonExistence(timeout: 5))
        }
        func history(_ expected: [Bool]) {
            editorHistory("Undo", in: app); expectInk(filled)
            editorHistory("Redo", in: app); expectInk(expected)
            editorHistory("Undo", in: app); expectInk(filled)
        }
        expectInk(filled)
        for apply in [false, true] {
            begin()
            editTransform("width", "50", in: app); editTransform("height", "25", in: app)
            expectInk([false, true, true, false, false])
            editTransform("angle", "90", in: app); expectTransform("angle", "90.0 °", in: app)
            let rotated = [false, true, false, true, false]
            expectInk(rotated)
            attachEditor(in: app, name: apply ? "rotation-before-apply" : "rotation-before-cancel")
            finish(apply)
            if apply { expectInk(rotated); history(rotated) }
            else {
                expectInk(filled)
                editorHistory("Undo", in: app); expectInk(blank)
                editorHistory("Redo", in: app); expectInk(filled)
            }
        }
        #if os(macOS)
        func drag(_ start: CGPoint, _ end: CGPoint, modifiers: XCUIElement.KeyModifierFlags = []) {
            XCUIElement.perform(withKeyModifiers: modifiers) {
                viewport.coordinate(withNormalizedOffset: CGVector(dx: start.x, dy: start.y)).click(forDuration: 0.05,
                    thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: end.x, dy: end.y)))
            }
            editorDocumentTitle(in: app).hover()
        }
        func near(_ id: String, _ expected: Double, tolerance: Double = 1) {
            let control = app.buttons["number-value-tool-transform_" + id]
            expectation(for: NSPredicate { _, _ in
                let text = control.value as? String ?? ""
                guard let value = Double(text.split(separator: " ").first ?? "") else { return false }
                return abs(value - expected) <= tolerance
            }, evaluatedWith: control)
            waitForExpectations(timeout: 5)
        }
        for (name, modifiers, width, height, x): (String, XCUIElement.KeyModifierFlags, Double, Double, Double) in [
            ("edge-scale", [], 75, 100, -256), ("shift-scale", .shift, 75, 75, -256),
            ("option-scale", .option, 50, 100, 0)] {
            begin()
            drag(CGPoint(x: bounds.maxX, y: center.y), CGPoint(x: center.x + bounds.width * 0.25, y: center.y), modifiers: modifiers)
            near("width", width); near("height", height); near("x", x, tolerance: 6); near("y", 0)
            let scaled = [name != "option-scale", true, true, true, false]
            expectInk(scaled); attachEditor(in: app, name: name)
            finish(true); history(scaled)
        }
        // Every corner must use the visible handle and keep the opposite
        // corner fixed. Sample all four quadrants independently of the fields.
        let title = editorDocumentTitle(in: app)
        let extent = (title.value as? String ?? title.label).components(separatedBy: " · ").last!
        let dimensions = extent.components(separatedBy: " × ").compactMap(Double.init)
        XCTAssertEqual(dimensions.count, 2)
        let corners = [CGPoint(x: -1, y: -1), CGPoint(x: 1, y: -1), CGPoint(x: -1, y: 1), CGPoint(x: 1, y: 1)]
        let quadrantSamples = corners.map {
            CGPoint(x: center.x + $0.x * bounds.width * 0.375, y: center.y + $0.y * bounds.height * 0.375)
        }
        for corner in corners {
            begin()
            drag(CGPoint(x: center.x + corner.x * bounds.width * 0.5, y: center.y + corner.y * bounds.height * 0.5),
                CGPoint(x: center.x + corner.x * bounds.width * 0.25, y: center.y + corner.y * bounds.height * 0.25))
            near("width", 75); near("height", 75)
            near("x", -corner.x * dimensions[0] / 8, tolerance: 6)
            near("y", -corner.y * dimensions[1] / 8, tolerance: 6)
            let scaled = corners.map { $0.x != corner.x && $0.y != corner.y }
            expectBluePaper(scaled, at: quadrantSamples, in: app)
            attachEditor(in: app, name: "corner-scale-\(Int(corner.x))-\(Int(corner.y))")
            finish(true); expectBluePaper(scaled, at: quadrantSamples, in: app)
            editorHistory("Undo", in: app); expectBluePaper([true, true, true, true], at: quadrantSamples, in: app)
            editorHistory("Redo", in: app); expectBluePaper(scaled, at: quadrantSamples, in: app)
            editorHistory("Undo", in: app); expectInk(filled)
        }
        begin()
        drag(center, CGPoint(x: center.x + bounds.width * 0.15, y: center.y + bounds.height * 0.1), modifiers: .shift)
        near("x", 307.2, tolerance: 6); near("y", 0)
        expectInk([false, true, true, true, true]); attachEditor(in: app, name: "shift-move")
        app.typeKey(XCUIKeyboardKey.escape.rawValue, modifierFlags: [])
        XCTAssertTrue(app.buttons["number-value-tool-transform_x"].waitForNonExistence(timeout: 5))
        expectInk(filled)
        editorHistory("Undo", in: app); expectInk(blank)
        editorHistory("Redo", in: app); expectInk(filled)

        begin(); editTransform("width", "50", in: app); editTransform("height", "25", in: app)
        // The shared overlay places the rotation grip 30 logical points above
        // the top edge (2.5 times its 12-point native hit reach).
        let radius = bounds.height * originalFrame.height * 0.125 + 30
        let angle = 71.0 * Double.pi / 180
        drag(CGPoint(x: center.x, y: center.y - radius / originalFrame.height),
            CGPoint(x: center.x + sin(angle) * radius / originalFrame.width,
                y: center.y - cos(angle) * radius / originalFrame.height), modifiers: .shift)
        near("angle", 75, tolerance: 0.1)
        let rotated = [false, true, false, true, false]
        expectInk(rotated); attachEditor(in: app, name: "shift-rotate-handle")
        finish(true); history(rotated)
        #endif
        XCTAssertEqual(viewport.frame, originalFrame)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkTransformFieldRetainsScroll(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
        let angle = app.buttons["number-value-tool-transform_angle"]
        let scroll = app.scrollViews.containing(.button, identifier: "number-value-tool-transform_x").firstMatch
        revealTransform(angle, in: app)
        XCTAssertTrue(scroll.frame.contains(angle.frame))
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let canvasFrame = canvas.frame
        attachEditor(in: app, name: "angle-before-edit")
        workspaceActivate(angle)
        let entry = app.textFields["number-entry-tool-transform_angle"]
        entry.typeText("15")
        attachEditor(in: app, name: "angle-during-edit")
        XCTAssertEqual(canvas.frame, canvasFrame, "Opening the keyboard must preserve the canvas bounds")
        #if os(iOS)
        let keyboard = app.keyboards.firstMatch
        XCTAssertTrue(keyboard.waitForExistence(timeout: 5))
        XCTAssertFalse(keyboard.frame.intersects(entry.frame), "The keyboard must not cover the edited field")
        XCTAssertLessThanOrEqual(scroll.frame.maxY, keyboard.frame.minY)
        #endif
        entry.typeText("\n")
        expectTransform("angle", "15.0 °", in: app)
        attachEditor(in: app, name: "angle-after-edit")
        XCTAssertTrue(scroll.frame.contains(angle.frame), "The edited Angle control must remain visible after Return")
        XCTAssertEqual(canvas.frame, canvasFrame, "Closing the keyboard must preserve the canvas bounds")
        let cancel = app.buttons["tool-action-cancel_transform"]
        revealTransform(cancel, in: app); workspaceActivate(cancel)
    }

    @MainActor private func artworkRows(in app: XCUIApplication) -> XCUIElementQuery {
        #if os(macOS)
        app.groups.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        #else
        app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        #endif
    }

    @MainActor private func layerContext(_ label: String, on element: XCUIElement, in app: XCUIApplication) {
        #if os(macOS)
        element.rightClick()
        #else
        element.press(forDuration: 0.6)
        #endif
        let action = app.buttons["menu-action-" + label]
        revealEditorControl(action, in: app.scrollViews.containing(.button, identifier: "menu-action-" + label).firstMatch)
        workspaceActivate(action)
        XCTAssertTrue(action.waitForNonExistence(timeout: 5))
    }

    @MainActor private func finishTransform(_ apply: Bool, in app: XCUIApplication) {
        let action = app.buttons["tool-action-" + (apply ? "apply_transform" : "cancel_transform")]
        revealTransform(action, in: app); workspaceActivate(action)
        XCTAssertTrue(app.buttons["number-value-tool-transform_x"].waitForNonExistence(timeout: 5))
    }

    @MainActor func checkMaskActionsAndHistory(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        func selectAll() { editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels") }
        func fill() { editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection") }
        selectAll(); fill()
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app); return Int(pixel[2]) > Int(pixel[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let bounds = bluePaperBounds(in: app), frame = workspaceViewport(in: app).frame
        let points = [0.1, 0.35, 0.65, 0.9].map { CGPoint(x: bounds.minX + bounds.width * $0, y: bounds.midY) }
        let full = [true, true, true, true], center = [false, true, true, false]
        let hole = [true, false, false, true], blank = [false, false, false, false]
        let initialCount = artworkRows(in: app).count
        let originalID = artworkRows(in: app).element(boundBy: 0).identifier
        var rowID = originalID
        func expectLayerCount(_ count: Int) {
            expectation(for: NSPredicate(format: "count == %d", count), evaluatedWith: artworkRows(in: app))
            waitForExpectations(timeout: 5)
        }
        func target(_ mask: Bool) -> XCUIElement {
            artworkRows(in: app)[rowID].buttons[mask ? "Edit layer mask" : "Edit layer content"]
        }
        func samples() -> [Data] { editorPixelSamples(in: app, at: points, size: 8) }
        func expectSamples(_ expected: [Data]) {
            expectation(for: NSPredicate { _, _ in samples() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        func expectMask(_ exists: Bool) {
            if exists { XCTAssertTrue(target(true).waitForExistence(timeout: 5)) }
            else { XCTAssertTrue(target(true).waitForNonExistence(timeout: 5)) }
        }
        func expectInk(_ expected: [Bool]) { expectBluePaper(expected, at: points, in: app) }
        func history(_ before: [Data], _ after: [Data], beforeMask: Bool = true, afterMask: Bool = true) {
            editorHistory("Undo", in: app); expectSamples(before); expectMask(beforeMask)
            editorHistory("Redo", in: app); expectSamples(after); expectMask(afterMask)
            editorHistory("Undo", in: app); expectSamples(before); expectMask(beforeMask)
        }
        func action(_ label: String, expected: [Bool], maskAfter: Bool = true) {
            let before = samples()
            layerContext(label, on: target(true), in: app)
            expectInk(expected); expectMask(maskAfter)
            history(before, samples(), afterMask: maskAfter)
        }
        func contentMaskAction(_ label: String) {
            #if os(macOS)
            target(false).rightClick()
            #else
            target(false).press(forDuration: 0.6)
            #endif
            workspaceActivate(app.buttons["menu-action-Mask"])
            let action = app.buttons[label]
            workspaceActivate(action)
            XCTAssertTrue(action.waitForNonExistence(timeout: 5))
        }

        // Transform the full selection on a temporary painted layer. Removing
        // that layer leaves a centered selection over the untouched blue art.
        workspaceActivate(app.buttons["layer-New layer"])
        expectLayerCount(initialCount + 1)
        fill()
        editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
        let aspect = app.buttons["tool-action-transform_aspect"]
        revealTransform(aspect, in: app)
        if aspect.isSelected { workspaceActivate(aspect) }
        editTransform("width", "50", in: app)
        finishTransform(true, in: app)
        workspaceActivate(app.buttons["layer-Delete selected layers"])
        expectLayerCount(initialCount)
        expectInk(full); expectMask(false)
        let unmasked = samples()
        for hide in [false, true] {
            contentMaskAction(hide ? "Mask: hide selection" : "Mask: reveal selection")
            expectInk(hide ? hole : center); expectMask(true)
            attachEditor(in: app, name: hide ? "mask-hide-selection" : "mask-reveal-selection")
            history(unmasked, samples(), beforeMask: false)
        }
        editorHistory("Redo", in: app); expectInk(hole); expectMask(true)

        // Copy is not an artwork edit and must retain the pending Redo.
        layerContext("Invert mask", on: target(true), in: app); expectInk(center)
        editorHistory("Undo", in: app); expectInk(hole)
        layerContext("Copy mask", on: target(true), in: app); expectInk(hole)
        editorHistory("Redo", in: app); expectInk(center)
        editorHistory("Undo", in: app); expectInk(hole)
        for (label, expected) in [("Enable mask", full), ("Invert mask", center),
            ("Reveal all", full), ("Hide all", blank), ("Delete mask", full)] {
            action(label, expected: expected, maskAfter: label != "Delete mask")
        }
        action("Apply mask to layer", expected: hole, maskAfter: false)
        attachEditor(in: app, name: "mask-actions-restored")

        let beforeInspection = samples()
        layerContext("Show mask area", on: target(true), in: app)
        expectation(for: NSPredicate { _, _ in samples() != beforeInspection }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let inspection = samples()
        for index in [0, 3] { XCTAssertEqual(inspection[index], beforeInspection[index]) }
        for index in [1, 2] {
            XCTAssertGreaterThan(inspection[index][2], inspection[index][0])
            XCTAssertGreaterThan(inspection[index][0], inspection[index][1])
        }
        attachEditor(in: app, name: "mask-area-inspection")
        history(beforeInspection, inspection)

        // Replacement consumes the current selection in the same undo step.
        selectAll()
        for (label, expected) in [("Replace mask: reveal selection", full), ("Replace mask: hide selection", blank)] {
            action(label, expected: expected)
        }
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        layerContext("Reveal all", on: target(true), in: app); expectInk(full)
        action("Replace with copied mask", expected: hole)
        editorHistory("Undo", in: app); expectInk(hole)

        // Paste the copied nontrivial mask onto a different filled layer.
        workspaceActivate(app.buttons["layer-New layer"])
        expectLayerCount(initialCount + 1)
        rowID = artworkRows(in: app).element(boundBy: 0).identifier
        XCTAssertNotEqual(rowID, originalID)
        selectAll(); fill()
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectInk(full); expectMask(false)
        let beforePaste = samples()
        contentMaskAction("Paste mask")
        expectInk(hole); expectMask(true)
        attachEditor(in: app, name: "mask-pasted-layer")
        history(beforePaste, samples(), beforeMask: false)
        workspaceActivate(app.buttons["layer-Delete selected layers"])
        expectLayerCount(initialCount)
        rowID = originalID; expectInk(hole); expectMask(true)
        XCTAssertEqual(workspaceViewport(in: app).frame, frame)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkLayerContentActionsAndHistory(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.9,0.25,0.2,1]},{"type":"color","action":{"op":"swap"}},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        func selectAll() { editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels") }
        func fill() { editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection") }
        selectAll(); fill()
        expectation(for: NSPredicate { _, _ in
            let p = self.editorPixels(in: app); return Int(p[2]) > Int(p[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let bounds = bluePaperBounds(in: app), frame = workspaceViewport(in: app).frame
        let points = [0.1, 0.35, 0.65, 0.9].map { CGPoint(x: bounds.minX + bounds.width * $0, y: bounds.midY) }
        let rows = artworkRows(in: app), initialCount = rows.count
        let originalID = rows.element(boundBy: 0).identifier
        var rowID = originalID
        enum Ink { case paper, blue, red }
        let blue: [Ink] = [.paper, .blue, .blue, .paper]
        let red: [Ink] = [.paper, .red, .red, .paper]
        let fullRed: [Ink] = [.red, .red, .red, .red]
        let blank: [Ink] = [.paper, .paper, .paper, .paper]
        func samples() -> [Data] { editorPixelSamples(in: app, at: points, size: 8) }
        func expectSamples(_ expected: [Data]) {
            expectation(for: NSPredicate { _, _ in samples() == expected }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        func expectInk(_ expected: [Ink]) {
            expectation(for: NSPredicate { _, _ in
                for (pixel, ink) in zip(samples(), expected) {
                    for i in stride(from: 0, to: pixel.count, by: 4) {
                        let r = Int(pixel[i]), g = Int(pixel[i + 1]), b = Int(pixel[i + 2])
                        switch ink {
                        case .paper: if min(r, g, b) <= 250 { return false }
                        case .blue: if b <= r + 50 { return false }
                        case .red: if r <= b + 50 { return false }
                        }
                    }
                }
                return true
            }, evaluatedWith: app)
            waitForExpectations(timeout: 10)
        }
        func expectCount(_ count: Int) {
            expectation(for: NSPredicate(format: "count == %d", count), evaluatedWith: rows)
            waitForExpectations(timeout: 5)
        }
        func content() -> XCUIElement { rows[rowID].buttons["Edit layer content"] }
        func action(_ label: String) { layerContext(label, on: content(), in: app) }
        func flag(_ label: String, _ selected: Bool) {
            expectation(for: NSPredicate(format: "selected == %@", NSNumber(value: selected)),
                evaluatedWith: app.buttons["layer-" + label])
            waitForExpectations(timeout: 5)
        }
        func history(_ before: [Data], _ after: [Data]) {
            for (command, pixels) in [("Undo", before), ("Redo", after), ("Undo", before)] {
                editorHistory(command, in: app); expectSamples(pixels)
            }
        }

        // A bounded paint layer exposes loss of alpha or accidental edits to
        // the source when clearing, recoloring and removing its duplicate.
        editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
        let aspect = app.buttons["tool-action-transform_aspect"]
        revealTransform(aspect, in: app)
        if aspect.isSelected { workspaceActivate(aspect) }
        editTransform("width", "50", in: app); finishTransform(true, in: app)
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        expectInk(blue)
        let original = samples()
        action("Duplicate"); expectCount(initialCount + 1); expectSamples(original)
        rowID = rows.element(boundBy: 0).identifier
        XCTAssertNotEqual(rowID, originalID)
        editorHistory("Undo", in: app); expectCount(initialCount); expectSamples(original)
        editorHistory("Redo", in: app); expectCount(initialCount + 1); expectSamples(original)
        workspaceActivate(rows[originalID].buttons["layer-Hide layer"]); expectSamples(original)
        action("Clear layer"); expectInk(blank)
        history(original, samples())
        attachEditor(in: app, name: "layer-duplicate-clear-restored")

        action("Alpha lock"); flag("Alpha lock", true)
        editorHistory("Undo", in: app); flag("Alpha lock", false)
        editorHistory("Redo", in: app); flag("Alpha lock", true)
        workspaceActivate(app.buttons["color-swap"]); selectAll(); fill()
        expectInk(red); attachEditor(in: app, name: "layer-alpha-locked-fill")
        history(original, samples()); flag("Alpha lock", true)
        action("Alpha lock"); flag("Alpha lock", false)
        fill(); expectInk(fullRed)
        history(original, samples())
        editorHistory("Redo", in: app); expectInk(fullRed)
        let opaqueRed = samples()

        action("Lock editing"); flag("Lock editing", true)
        XCTAssertFalse(app.buttons["layer-Alpha lock"].isEnabled)
        XCTAssertFalse(app.buttons["number-value-layer-opacity"].isEnabled)
        #if os(macOS)
        content().rightClick()
        #else
        content().press(forDuration: 0.6)
        #endif
        XCTAssertTrue(app.buttons["menu-action-Clear layer"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["menu-action-Clear layer"].isEnabled)
        workspaceActivate(app.buttons["menu-action-Lock editing"])
        flag("Lock editing", false)
        editorHistory("Undo", in: app); flag("Lock editing", true); expectSamples(opaqueRed)
        editorHistory("Redo", in: app); flag("Lock editing", false); expectSamples(opaqueRed)

        workspaceActivate(rows[originalID].buttons["layer-Show layer"]); expectSamples(opaqueRed)
        action("Clip to layer below"); flag("Clip to layer below", true); expectInk(red)
        attachEditor(in: app, name: "layer-clipped-duplicate")
        history(opaqueRed, samples()); flag("Clip to layer below", false)
        action("Delete layer"); expectCount(initialCount); expectSamples(original)
        rowID = originalID

        // New clipping layers and bulk duplication must preserve the base and
        // clipping relationship together, with one history entry per command.
        action("New clipping layer"); expectCount(initialCount + 1)
        rowID = rows.element(boundBy: 0).identifier
        let clippingID = rowID
        flag("Clip to layer below", true); expectSamples(original)
        editorHistory("Undo", in: app); expectCount(initialCount); expectSamples(original)
        editorHistory("Redo", in: app); expectCount(initialCount + 1); flag("Clip to layer below", true)
        selectAll(); fill(); expectInk(red)
        let clipped = samples()
        history(original, clipped)
        editorHistory("Redo", in: app); expectSamples(clipped)
        workspaceActivate(rows[originalID].buttons["layer-Select layer without changing drawing target"])
        XCTAssertTrue(content().isSelected, "Checking the base must retain the clipping layer as drawing target")
        action("Duplicate selected layers"); expectCount(initialCount + 3); expectSamples(clipped)
        rowID = rows.element(boundBy: 0).identifier
        XCTAssertNotEqual(rowID, clippingID)
        editorHistory("Undo", in: app); expectCount(initialCount + 1); expectSamples(clipped)
        editorHistory("Redo", in: app); expectCount(initialCount + 3); expectSamples(clipped)
        attachEditor(in: app, name: "layer-clipping-stack-duplicated")
        // History restores the artwork target; checked rows are UI selection.
        let copiedBase = rows.element(boundBy: 1).buttons["layer-Select layer without changing drawing target"]
        if !copiedBase.isSelected { workspaceActivate(copiedBase) }
        action("Delete selected layers"); expectCount(initialCount + 1); expectSamples(clipped)
        editorHistory("Undo", in: app); expectCount(initialCount + 3); expectSamples(clipped)
        editorHistory("Redo", in: app); expectCount(initialCount + 1); expectSamples(clipped)
        rowID = clippingID
        action("Delete layer"); expectCount(initialCount); expectSamples(original)
        attachEditor(in: app, name: "layer-original-preserved")
        XCTAssertEqual(workspaceViewport(in: app).frame, frame)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkMaskTransforms(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app); return Int(pixel[2]) > Int(pixel[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let bounds = bluePaperBounds(in: app), viewport = workspaceViewport(in: app)
        let frame = viewport.frame
        let points = [0.1, 0.35, 0.6, 0.9].map {
            CGPoint(x: bounds.minX + bounds.width * $0, y: bounds.midY)
        }
        func expectInk(_ expected: [Bool]) { expectBluePaper(expected, at: points, in: app) }
        let row = artworkRows(in: app).element(boundBy: 0)
        let rowID = row.identifier
        let current = artworkRows(in: app)[rowID]
        workspaceActivate(app.buttons["layer-Add layer mask"])
        let mask = current.buttons["Edit layer mask"], content = current.buttons["Edit layer content"]
        XCTAssertTrue(mask.waitForExistence(timeout: 5))
        attachEditor(in: app, name: "mask-before-unlink")
        workspaceActivate(current.buttons["layer-Unlink mask from layer"])
        attachEditor(in: app, name: "mask-after-unlink")
        XCTAssertTrue(current.buttons["layer-Link mask to layer"].waitForExistence(timeout: 5))
        workspaceActivate(mask)
        layerContext("Invert mask", on: mask, in: app)
        expectInk([false, false, false, false])
        // The inverted full-selection mask hides the paper. Shrinking only the
        // mask leaves a blue border around a white center, all through native UI.
        editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
        let aspect = app.buttons["tool-action-transform_aspect"]
        revealTransform(aspect, in: app)
        if !aspect.isSelected { workspaceActivate(aspect) }
        editTransform("width", "50", in: app)
        finishTransform(true, in: app)
        let baseline = [true, false, false, true]
        expectInk(baseline); attachEditor(in: app, name: "independent-mask-scaled")

        for linked in [false, true] {
            if linked {
                workspaceActivate(current.buttons["layer-Link mask to layer"])
                XCTAssertTrue(current.buttons["layer-Unlink mask from layer"].waitForExistence(timeout: 5))
            }
            for editMask in [false, true] {
                let target = editMask ? mask : content
                workspaceActivate(target)
                expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: target)
                waitForExpectations(timeout: 5)
                let moved = linked ? [false, true, false, false]
                    : editMask ? [true, true, false, false] : [false, false, false, true]
                let name = "\(linked ? "linked" : "independent")-\(editMask ? "mask" : "content")"
                func history() {
                    editorHistory("Undo", in: app); expectInk(baseline)
                    editorHistory("Redo", in: app); expectInk(moved)
                    editorHistory("Undo", in: app); expectInk(baseline)
                }
                editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
                editTransform("x", "512", in: app); expectInk(moved)
                attachEditor(in: app, name: "transform-" + name)
                finishTransform(true, in: app); expectInk(moved); history()
                // Cancellation must retain the same target and committed mask.
                editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
                editTransform("x", "512", in: app); expectInk(moved)
                finishTransform(false, in: app); expectInk(baseline)
                XCTAssertTrue(target.isSelected)
                #if os(macOS)
                // Cancel already returns to Move. Activating its toolbar tile
                // again opens the tool drawer over the canvas.
                XCTAssertTrue(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                    "tool-group-", "Move")).firstMatch.isSelected)
                viewport.coordinate(withNormalizedOffset: CGVector(dx: bounds.midX, dy: bounds.midY)).click(forDuration: 0.05,
                    thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(
                        dx: bounds.midX + bounds.width * 0.25, dy: bounds.midY)))
                editorDocumentTitle(in: app).hover()
                expectInk(moved); attachEditor(in: app, name: "move-" + name); history()
                #endif
                XCTAssertEqual(viewport.frame, frame)
            }
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkGroupArtworkWorkflow(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app); return Int(pixel[2]) > Int(pixel[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let bounds = bluePaperBounds(in: app), viewport = workspaceViewport(in: app)
        let frame = viewport.frame, rows = artworkRows(in: app)
        let firstID = rows.element(boundBy: 0).identifier
        // Two separated pieces on separate layers make a partial group move visible.
        for x in [-512, 512] {
            if x > 0 {
                workspaceActivate(app.buttons["layer-New layer"])
                // Applying the first transform also transforms its selection.
                editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
                editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
            }
            editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
            let aspect = app.buttons["tool-action-transform_aspect"]
            revealTransform(aspect, in: app)
            if aspect.isSelected { workspaceActivate(aspect) }
            editTransform("width", "25", in: app); editTransform("height", "50", in: app)
            editTransform("x", String(x), in: app)
            expectTransform("width", "25 %", in: app); expectTransform("height", "50 %", in: app)
            expectTransform("x", "\(x).0 px", in: app)
            finishTransform(true, in: app)
        }
        let points = [0.2, 0.4, 0.55, 0.7, 0.9].map {
            CGPoint(x: bounds.minX + bounds.width * $0, y: bounds.midY)
        }
        let baseline = [true, false, false, true, false], blank = [false, false, false, false, false]
        func expectInk(_ expected: [Bool]) { expectBluePaper(expected, at: points, in: app) }
        attachEditor(in: app, name: "group-two-layer-fixture")
        expectInk(baseline)
        let first = rows[firstID]
        workspaceActivate(first.buttons["layer-Select layer without changing drawing target"])
        layerContext("Group selected layers", on: first.buttons["Edit layer content"], in: app)
        expectation(for: NSPredicate { _, _ in rows.count == 4 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let groupID = rows.element(boundBy: 0).identifier
        let group = rows[groupID]
        expectInk(baseline)
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5); expectInk(baseline)
        editorHistory("Redo", in: app)
        XCTAssertTrue(group.waitForExistence(timeout: 5)); expectInk(baseline)
        workspaceActivate(group.staticTexts["Group"])
        let collapse = group.buttons["Collapse or expand group"]
        expectation(for: NSPredicate(format: "selected == true"), evaluatedWith: collapse)
        waitForExpectations(timeout: 5)
        workspaceActivate(collapse)
        expectation(for: NSPredicate { _, _ in rows.count == 2 }, evaluatedWith: app)
        waitForExpectations(timeout: 5); expectInk(baseline)
        workspaceActivate(collapse)
        expectation(for: NSPredicate { _, _ in rows.count == 4 }, evaluatedWith: app)
        waitForExpectations(timeout: 5); expectInk(baseline)
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "tool-group-", "Move")).firstMatch.isSelected)
        #if os(macOS)
        viewport.coordinate(withNormalizedOffset: CGVector(dx: bounds.midX, dy: bounds.midY)).click(forDuration: 0.05,
            thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(dx: bounds.midX + bounds.width * 0.125, dy: bounds.midY)))
        editorDocumentTitle(in: app).hover()
        let moved = [false, true, false, false, true]
        expectInk(moved); attachEditor(in: app, name: "group-move-released")
        editorHistory("Undo", in: app); expectInk(baseline)
        editorHistory("Redo", in: app); expectInk(moved)
        editorHistory("Undo", in: app); expectInk(baseline)
        #endif
        workspaceActivate(group.buttons["layer-Hide layer"]); expectInk(blank)
        editorHistory("Undo", in: app); expectInk(baseline)
        editorHistory("Redo", in: app); expectInk(blank)
        editorHistory("Undo", in: app); expectInk(baseline)
        layerContext("Ungroup", on: group.staticTexts["Group"], in: app)
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5); expectInk(baseline)
        editorHistory("Undo", in: app)
        XCTAssertTrue(group.waitForExistence(timeout: 5)); expectInk(baseline)
        editorHistory("Redo", in: app)
        XCTAssertTrue(group.waitForNonExistence(timeout: 5)); expectInk(baseline)
        attachEditor(in: app, name: "group-ungrouped")
        XCTAssertEqual(viewport.frame, frame)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkMoveAndTransformCancellation(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        editorMenu(in: app, menu: "Edit", id: "fill_selection", label: "Fill selection")
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app)
            return Int(pixel[2]) > Int(pixel[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let bounds = bluePaperBounds(in: app)
        let viewport = workspaceViewport(in: app), frame = viewport.frame
        let center = CGPoint(x: bounds.midX, y: bounds.midY)
        let points = [CGPoint(x: bounds.minX + bounds.width * 0.1, y: center.y),
            CGPoint(x: center.x, y: bounds.minY + bounds.height * 0.1), center,
            CGPoint(x: bounds.maxX - bounds.width * 0.1, y: center.y),
            CGPoint(x: center.x, y: bounds.maxY - bounds.height * 0.1)]
        let filled = [true, true, true, true, true], blank = [false, false, false, false, false]
        func expectInk(_ value: [Bool]) { expectBluePaper(value, at: points, in: app) }
        editorTool("Operation", in: app); editorChoice("Move", group: true, in: app)
        XCTAssertFalse(app.buttons["number-value-tool-transform_x"].exists)
        expectInk(filled)
        #if os(macOS)
        viewport.coordinate(withNormalizedOffset: CGVector(dx: center.x, dy: center.y)).click(forDuration: 0.05,
            thenDragTo: viewport.coordinate(withNormalizedOffset: CGVector(
                dx: center.x + bounds.width * 0.25, dy: center.y + bounds.height * 0.2)))
        editorDocumentTitle(in: app).hover()
        let baseline = [false, false, true, true, true], previous = filled
        expectInk(baseline); attachEditor(in: app, name: "move-layer-released")
        editorHistory("Undo", in: app); expectInk(previous)
        editorHistory("Redo", in: app); expectInk(baseline)
        #else
        // Touch navigation cannot substitute for Pencil artwork manipulation.
        let baseline = filled, previous = blank
        #endif
        for leaveApp in [false, true] {
            editorMenu(in: app, menu: "Edit", id: "scale_rotate", label: "Scale / rotate")
            editTransform("x", "4096", in: app); expectInk(blank)
            if leaveApp {
                #if os(macOS)
                app.typeKey("h", modifierFlags: .command)
                XCTAssertTrue(app.wait(for: .runningBackground, timeout: 10))
                #else
                XCUIDevice.shared.press(.home)
                expectation(for: NSPredicate { _, _ in
                    app.state == .runningBackground || app.state == .runningBackgroundSuspended
                }, evaluatedWith: app)
                waitForExpectations(timeout: 10)
                #endif
                app.activate()
                XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
                let canvas = app.descendants(matching: .any)["canvas"].firstMatch
                XCTAssertTrue(canvas.waitForExistence(timeout: 20), "Returning to the app must restore the editor")
                XCTAssertEqual(viewport.frame, frame)
            } else { editorTool("Pen", in: app) }
            XCTAssertTrue(app.buttons["number-value-tool-transform_x"].waitForNonExistence(timeout: 5))
            expectInk(baseline)
            attachEditor(in: app, name: leaveApp ? "transform-after-background" : "transform-after-tool-change")
            // The preceding artwork edit must still be the very next Undo.
            editorHistory("Undo", in: app); expectInk(previous)
            editorHistory("Redo", in: app); expectInk(baseline)
            XCTAssertEqual(viewport.frame, frame)
        }
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

}
