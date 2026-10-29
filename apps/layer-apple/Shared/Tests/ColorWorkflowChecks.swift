import XCTest

extension XCTestCase {
    #if os(iOS)
    @MainActor func checkDialogKeyboardHeight(_ dialog: XCUIElement, in app: XCUIApplication, openKeyboard: () -> Void) {
        if app.keyboards.firstMatch.exists { dismissEditorKeyboard(in: app) }
        func outerFrame() -> CGRect {
            app.otherElements.containing(.any, identifier: dialog.identifier).allElementsBoundByIndex
                .map(\.frame).filter { abs($0.width - dialog.frame.width) <= 2 }
                .max { $0.height < $1.height } ?? .zero
        }
        let initial = outerFrame()
        XCTAssertGreaterThan(initial.height, 0, "The native sheet has a matching-width dialog ancestor")
        XCTAssertLessThanOrEqual(initial.height - dialog.frame.height, 40, "The native sheet fits its preferred content")
        openKeyboard()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        dismissEditorKeyboard(in: app)
        let restored = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            abs(outerFrame().height - initial.height) <= 2 && outerFrame().height - dialog.frame.height <= 40
        }, object: app)
        let result = XCTWaiter.wait(for: [restored], timeout: 10)
        if app.state == .runningForeground { attachEditor(in: app, name: dialog.identifier + "-after-keyboard") }
        XCTAssertEqual(result, .completed,
            "Keyboard dismissal must restore the native sheet height: before \(initial), after \(outerFrame()), content \(dialog.frame)")
    }
    @MainActor func fileProviderRoot(_ provider: String, in app: XCUIApplication) -> XCUIElement {
        app.cells.matching(NSPredicate(format: "identifier == %@ OR identifier == %@",
            provider + ", Container", provider + ", Folder")).firstMatch
    }
    @MainActor func fileProviderFile(_ name: String, in app: XCUIApplication) -> XCUIElement {
        let file = URL(fileURLWithPath: name)
        return app.cells.matching(NSPredicate(format: "identifier == %@ OR identifier BEGINSWITH %@",
            file.deletingPathExtension().lastPathComponent + ", " + file.pathExtension, name + ",")).firstMatch
    }
    @MainActor func submitFileProviderSelection(in app: XCUIApplication) {
        let picker = app.navigationBars["FullDocumentManagerViewControllerNavigationBar"]
        let open = picker.buttons.matching(NSPredicate(format: "label == %@ AND enabled == true", "Open"))
        XCTAssertTrue(open.firstMatch.waitForExistence(timeout: 10))
        XCTAssertEqual(open.count, 1)
        workspaceActivate(open.firstMatch)
        XCTAssertTrue(picker.waitForNonExistence(timeout: 10))
    }
    @MainActor func dismissEditorKeyboard(in app: XCUIApplication) {
        func zeroKey(_ node: any XCUIElementSnapshot) -> Bool {
            node.elementType == .key && node.label == "0" || node.children.contains(where: zeroKey)
        }
        typealias Pad = (frame: CGRect, backdrop: CGRect?)
        func numberPads(_ node: any XCUIElementSnapshot) -> [Pad] {
            node.children.flatMap { child -> [Pad] in
                let nested = numberPads(child)
                if !nested.isEmpty { return nested }
                guard child.elementType == .popover && zeroKey(child) else { return [] }
                let regions = node.children.filter { $0.identifier == "PopoverDismissRegion" }
                return [(child.frame, regions.count == 1 ? regions[0].frame : nil)]
            }
        }
        func usable(_ frame: CGRect) -> Bool {
            [frame.minX, frame.minY, frame.maxX, frame.maxY].allSatisfy(\.isFinite) && frame.width > 0 && frame.height > 0
        }
        func fail(_ message: String) {
            let image = XCTAttachment(screenshot: app.screenshot()), tree = XCTAttachment(string: app.debugDescription)
            image.name = "keyboard-dismissal-blocked"; tree.name = "keyboard-dismissal-hierarchy"
            image.lifetime = .keepAlways; tree.lifetime = .keepAlways; add(image); add(tree)
            XCTFail(message)
        }
        let window = workspaceViewport(in: app)
        let keyboard = app.keyboards.firstMatch
        let hide = keyboard.buttons["Hide keyboard"]
        let numberPad = app.popovers.containing(.key, identifier: "0").firstMatch
        func keyboardShown() -> Bool { keyboard.exists && keyboard.frame.intersects(window.frame) }
        let visible = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            keyboardShown() || numberPad.exists
        }, object: app)
        let ready = XCTWaiter.wait(for: [visible], timeout: 5)
        guard ready == .completed else { return }
        let pads: [Pad]
        do { pads = numberPad.exists ? numberPads(try app.snapshot()) : [] }
        catch { XCTFail("Cannot inspect the editor keyboard: \(error)"); return }
        guard pads.count <= 1 else { fail("The editor exposes multiple number pads: \(pads)"); return }
        if let pad = pads.first {
            let bounds = window.frame
            guard let backdrop = pad.backdrop, usable(backdrop), usable(pad.frame), usable(bounds), pad.frame.intersects(bounds) else {
                fail("The number pad needs its own dismissal region: \(pad), window \(bounds)")
                return
            }
            let region = backdrop.intersection(bounds), covered = pad.frame.intersection(region)
            guard usable(region), usable(covered) else {
                fail("The number pad needs a visible dismissal region: \(pad), window \(bounds)"); return
            }
            let candidates = [
                CGRect(x: region.minX, y: region.minY, width: covered.minX - region.minX, height: region.height),
                CGRect(x: covered.maxX, y: region.minY, width: region.maxX - covered.maxX, height: region.height),
                CGRect(x: region.minX, y: region.minY, width: region.width, height: covered.minY - region.minY),
                CGRect(x: region.minX, y: covered.maxY, width: region.width, height: region.maxY - covered.maxY)
            ].filter { usable($0) && region.contains($0) }
            guard let target = candidates.max(by: { $0.width * $0.height < $1.width * $1.height }) else {
                fail("The number pad covers its dismissal region: \(pad), window \(bounds)")
                return
            }
            let point = CGPoint(x: target.midX, y: target.midY)
            guard point.x.isFinite, point.y.isFinite, bounds.contains(point), region.contains(point), !pad.frame.contains(point) else {
                fail("The number pad must have a dismissal point inside its editor window: \(pad), window \(bounds)")
                return
            }
            window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: point.x - bounds.minX, dy: point.y - bounds.minY)).clickOrTap()
            let absent = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                app.textFields.firstMatch.exists && !numberPad.exists
            }, object: app)
            let result = XCTWaiter.wait(for: [absent], timeout: 5)
            guard result == .completed else { fail("The number pad must close while its editor fields remain visible"); return }
        }
        if XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in keyboardShown() }, object: app)], timeout: 5) == .completed {
            guard hide.waitForExistence(timeout: 15), hide.isHittable else { fail("The keyboard needs a hittable Hide button"); return }
            hide.clickOrTap()
            let hidden = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in !keyboardShown() }, object: app)
            XCTAssertEqual(XCTWaiter.wait(for: [hidden], timeout: 5), .completed, "Hide keyboard uncovers the editor")
        }
    }
    @MainActor func editorTextMatches(_ field: XCUIElement, _ expected: String) -> Bool {
        guard let actual = field.value as? String else { return false }
        return actual == expected || (expected.isEmpty && actual == field.placeholderValue)
    }
    #endif
    @MainActor func replaceEditorText(_ field: XCUIElement, with value: String, in app: XCUIApplication) {
        #if os(macOS)
        workspaceActivate(field)
        field.typeKey("a", modifierFlags: .command)
        field.typeText(value.isEmpty ? XCUIKeyboardKey.delete.rawValue : value)
        #else
        if !field.isHittable { dismissEditorKeyboard(in: app) }
        let focused = app.textFields.matching(NSPredicate(format: "identifier == %@ AND hasKeyboardFocus == true", field.identifier)).firstMatch
        let scroll = app.scrollViews.containing(.textField, identifier: field.identifier).firstMatch
        for _ in 0..<2 {
            if focused.exists { break }
            if scroll.exists { revealEditorControl(field, in: scroll) }
            workspaceActivate(field)
            if focused.waitForExistence(timeout: 1) { break }
        }
        XCTAssertTrue(focused.waitForExistence(timeout: 3))
        let current = field.value as? String ?? ""
        let count = current == field.placeholderValue ? 0 : current.count
        field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: count) + value)
        #endif
        #if os(macOS)
        XCTAssertEqual(field.value as? String, value)
        #else
        XCTAssertTrue(editorTextMatches(field, value), "Expected '\(value)', got \(String(describing: field.value))")
        #endif
    }
    @MainActor func checkNativeSDRCreationAndColorEditing(in app: XCUIApplication) {
        // Initial actions run only after native GPU startup completes.
        let create = app.buttons["new-drawing-create"]
        guard create.waitForExistence(timeout: 60) else {
            XCTFail("The initial New drawing dialog did not open after canvas startup")
            return
        }
        chooseNewDrawingOption("new-drawing-space", 1, in: app)
        chooseNewDrawingOption("new-drawing-depth", 1, in: app)
        let useColor = app.buttons["new-drawing-color-use"]
        workspaceActivate(app.buttons["new-drawing-paper-color"])
        workspaceActivate(app.buttons["new-drawing-color-mode-transparent"])
        workspaceActivate(useColor)
        XCTAssertTrue(useColor.waitForNonExistence(timeout: 10))
        chooseNewDrawingOption("new-drawing-unit", 3, in: app)
        enterNewDrawingNumber("width", "79", in: app)
        enterNewDrawingNumber("height", "53", in: app)
        workspaceActivate(app.buttons["new-drawing-presets-button"])
        workspaceActivate(app.buttons["new-drawing-save-as"])
        replaceEditorText(app.textFields["new-drawing-save-name"], with: "Studio P3", in: app)
        #if os(iOS)
        dismissEditorKeyboard(in: app)
        #endif
        workspaceActivate(app.buttons["new-drawing-save-confirm"])
        let select = app.buttons["new-drawing-select"]
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: select)
        waitForExpectations(timeout: 10)
        workspaceActivate(select)
        XCTAssertTrue(select.waitForNonExistence(timeout: 10))
        attachEditor(in: app, name: "sdr-new-options")
        workspaceActivate(create)
        #if os(iOS)
        if !create.waitForNonExistence(timeout: 2) { workspaceActivate(create) }
        #endif
        XCTAssertTrue(create.waitForNonExistence(timeout: 30))
        let title = editorDocumentTitle(in: app)
        expectation(for: NSPredicate(format: "label CONTAINS %@ OR value CONTAINS %@", "79 × 53", "79 × 53"), evaluatedWith: title)
        waitForExpectations(timeout: 30)
        workspaceActivate(app.buttons["paint-edit-color"].firstMatch)
        let use = app.buttons["color-use"]
        XCTAssertTrue(use.waitForExistence(timeout: 10))
        editColorValue("0-0", "37", in: app)
        workspaceActivate(use)
        XCTAssertTrue(use.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["paint-edit-color"].firstMatch)
        XCTAssertTrue(use.waitForExistence(timeout: 10))
        XCTAssertEqual(colorValueText("0-0", in: app), "37")
        attachEditor(in: app, name: "sdr-tagged-paint")
        workspaceActivate(app.buttons["color-cancel"])
        XCTAssertTrue(use.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.buttons["paint-palettes"].exists, "The picker uses Edit Color instead of a palette popup")
        #if os(macOS)
        app.typeKey("n", modifierFlags: .command)
        #else
        editorMenu(in: app, menu: "File", id: "new_document", label: "New…")
        #endif
        XCTAssertTrue(create.waitForExistence(timeout: 10))
        XCTAssertEqual(newDrawingNumber("width", in: app).value as? String, "79")
        XCTAssertEqual(newDrawingNumber("height", in: app).value as? String, "53")
        expectNewDrawingText("new-drawing-caption", in: app) { $0.hasPrefix("Studio P3") }
        workspaceActivate(app.buttons["new-drawing-cancel"])
        XCTAssertTrue(create.waitForNonExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
