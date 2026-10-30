import XCTest
import ImageIO

extension XCTestCase {
    @MainActor func checkToolbarComponents(in app: XCUIApplication, theme: String = "light") {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        #if os(macOS)
        XCTAssertFalse(app.menuBars.menuBarItems["Help"].exists)
        #endif
        editorTool("Pencil", in: app)
        let sketch = app.buttons["workspace-switch-builtin:workspace:painter"]
        workspaceActivate(sketch)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: sketch)
        waitForExpectations(timeout: 10)
        let sliders = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "component-slider-"))
        expectation(for: NSPredicate(format: "count == 2"), evaluatedWith: sliders)
        waitForExpectations(timeout: 15)
        #if os(macOS)
        XCTAssertFalse(app.menuBars.menuBarItems["Help"].exists, "Changing workspace must retain the absent native Help menu")
        #endif
        let caps = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "slider-cap-"))
        let sizeCap = caps.matching(NSPredicate(format: "label == %@", "Brush size")).firstMatch
        let strengthCap = caps.matching(NSPredicate(format: "label == %@", "Strength")).firstMatch
        guard sizeCap.exists && strengthCap.exists else { return XCTFail("Brush size and Strength caps must name their controls") }
        let size = sliders["component-slider-" + String(sizeCap.identifier.dropFirst("slider-cap-".count))]
        let strength = sliders["component-slider-" + String(strengthCap.identifier.dropFirst("slider-cap-".count))]
        XCTAssertTrue(size.exists && strength.exists)
        XCTAssertNotEqual(size.identifier, strength.identifier)
        let paper = editorPixels(in: app)
        let undoButtons = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", "Undo"))
        let undo = undoButtons.firstMatch
        XCTAssertTrue(undo.waitForExistence(timeout: 10)); XCTAssertEqual(undoButtons.count, 1)
        let undoEnabled = undo.isEnabled
        func tap(_ element: XCUIElement, _ offset: CGVector) {
            let point = element.coordinate(withNormalizedOffset: offset)
            point.clickOrTap()
        }
        tap(size, CGVector(dx: 0.5, dy: 0.25))
        let preview = app.descendants(matching: .any)["brush-slider-preview"].firstMatch
        XCTAssertTrue(preview.waitForExistence(timeout: 10), "A slider tap opens the stamp preview")
        let caption = app.staticTexts["slider-preview-caption"]
        XCTAssertTrue(caption.waitForExistence(timeout: 5))
        let first = editorText(caption)
        XCTAssertFalse(first.isEmpty, "The preview names the value")
        tap(size, CGVector(dx: 0.5, dy: 0.75))
        expectation(for: NSPredicate { _, _ in self.editorText(caption) != first }, evaluatedWith: caption)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "brush-size-preview-\(theme)")
        let bookmark = app.buttons["slider-bookmark"]
        XCTAssertEqual(bookmark.label, "Bookmark this value")
        workspaceActivate(bookmark)
        expectation(for: NSPredicate(format: "label == %@", "Remove bookmark"), evaluatedWith: bookmark)
        waitForExpectations(timeout: 10)
        workspaceActivate(bookmark)
        expectation(for: NSPredicate(format: "label == %@", "Bookmark this value"), evaluatedWith: bookmark)
        waitForExpectations(timeout: 10)
        workspaceActivate(sketch)
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: preview)
        waitForExpectations(timeout: 10)
        func stampPixels() -> Data {
            let source = CGImageSourceCreateWithData(preview.screenshot().pngRepresentation as CFData, nil)!
            let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil)! as NSDictionary
            let extent = max(properties[kCGImagePropertyPixelWidth] as! Int, properties[kCGImagePropertyPixelHeight] as! Int)
            let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: extent] as CFDictionary)!
            let scale = CGFloat(image.height) / preview.frame.height
            let top = max(caption.frame.maxY, bookmark.frame.maxY) - preview.frame.minY + 8
            let crop = image.cropping(to: CGRect(x: 4 * scale, y: top * scale,
                width: CGFloat(image.width) - 8 * scale, height: CGFloat(image.height) - (top + 4) * scale))!
            let data = NSMutableData()
            let destination = CGImageDestinationCreateWithData(data, "public.png" as CFString, 1, nil)!
            CGImageDestinationAddImage(destination, crop, nil)
            XCTAssertTrue(CGImageDestinationFinalize(destination))
            return data as Data
        }
        func strengthEnd(_ full: Bool) {
            tap(strength, strength.frame.height > strength.frame.width
                ? CGVector(dx: 0.5, dy: full ? 0.01 : 0.99) : CGVector(dx: full ? 0.99 : 0.01, dy: 0.5))
            XCTAssertTrue(preview.waitForExistence(timeout: 10))
            expectation(for: NSPredicate { _, _ in self.editorText(caption) == (full ? "Strength: 100 %" : "Strength: 0 %") }, evaluatedWith: caption)
            waitForExpectations(timeout: 10)
        }
        strengthEnd(false)
        let empty = stampPixels(), footprint = preview.frame
        XCTAssertEqual(editorText(caption), "Strength: 0 %")
        attachEditor(in: app, name: "strength-zero-preview-\(theme)")
        strengthEnd(true)
        expectation(for: NSPredicate { _, _ in stampPixels() != empty }, evaluatedWith: preview)
        waitForExpectations(timeout: 15)
        XCTAssertEqual(editorText(caption), "Strength: 100 %")
        XCTAssertEqual(preview.frame, footprint, "Zero Strength keeps the canonical stamp viewport")
        attachEditor(in: app, name: "strength-full-preview-\(theme)")
        strengthEnd(false)
        expectation(for: NSPredicate { _, _ in stampPixels() == empty }, evaluatedWith: preview)
        waitForExpectations(timeout: 15)
        XCTAssertEqual(editorText(caption), "Strength: 0 %")
        func sketchTool(_ id: String) {
            let button = app.buttons["header-tool-" + id]
            workspaceActivate(button)
            expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: button)
            waitForExpectations(timeout: 10)
        }
        sketchTool("eraser")
        XCTAssertTrue(preview.waitForNonExistence(timeout: 10), "A tool change retires its preview")
        XCTAssertEqual(editorPixels(in: app), paper, "Stamp previews preserve artwork")
        XCTAssertEqual(undo.isEnabled, undoEnabled, "Stamp previews preserve artwork history")
        let start = strength.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.8))
        let end = strength.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.3))
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        #else
        start.press(forDuration: 0.05, thenDragTo: end)
        #endif
        XCTAssertFalse(preview.exists, "A completed drag closes its preview")
        func order() -> [String] { sliders.allElementsBoundByIndex.map(\.identifier) }
        let original = order()
        func dragCap(hold: TimeInterval) {
            let from = caps.element(boundBy: 1).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            let to = caps.element(boundBy: 0).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1))
            #if os(macOS)
            from.click(forDuration: hold, thenDragTo: to)
            #else
            from.press(forDuration: hold, thenDragTo: to)
            #endif
        }
        dragCap(hold: 0.05)
        XCTAssertEqual(order(), original, "A quick cap drag never reorders")
        dragCap(hold: 0.8)
        expectation(for: NSPredicate { _, _ in order() == original.reversed() }, evaluatedWith: sliders)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(preview.exists)
        let photo = app.buttons["workspace-switch-builtin:workspace:photographer"]
        tap(strength, CGVector(dx: 0.5, dy: 0.5))
        XCTAssertTrue(preview.waitForExistence(timeout: 10))
        workspaceActivate(photo)
        XCTAssertTrue(preview.waitForNonExistence(timeout: 10), "A workspace change retires its preview")
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: photo)
        waitForExpectations(timeout: 10)
        let more = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "toolbar-more-")).firstMatch
        XCTAssertTrue(more.waitForExistence(timeout: 10), "Photo's command bar carries Tool Options")
        attachEditor(in: app, name: "photo-tool-options-\(theme)")
        let options = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "toolbar-component-")).firstMatch
        func openDisplayMenu(fromEmptySpace: Bool) {
            let end = options.descendants(matching: .any).allElementsBoundByIndex.map(\.frame)
                .filter { $0.width < options.frame.width && $0.maxX < more.frame.minX }.map(\.maxX).max() ?? options.frame.minX
            let point = fromEmptySpace ? workspacePoint(options)?
                .withOffset(CGVector(dx: (end + more.frame.minX) / 2 - options.frame.midX, dy: 0))
                : workspacePoint(more)
            #if os(macOS)
            point?.rightClick()
            #else
            point?.press(forDuration: 0.8)
            #endif
        }
        for fromEmptySpace in [true, false] {
            openDisplayMenu(fromEmptySpace: fromEmptySpace)
            let sliders = app.buttons["menu-action-Show Sliders"]
            XCTAssertTrue(sliders.waitForExistence(timeout: 5), fromEmptySpace ? "Empty options space opens the display menu" : "More opens the display menu")
            workspaceActivate(sliders)
            expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: sliders)
            waitForExpectations(timeout: 5)
        }
        editorTool("Rectangle select", in: app)
        let segments = app.descendants(matching: .any)["toolbar-segments-selection-mode"]
        XCTAssertTrue(segments.waitForExistence(timeout: 10), "Selection modes join the options bar")
        let dropdown = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "toolbar-choice-")).firstMatch
        XCTAssertTrue(dropdown.exists, "Selection tools keep a list choice")
        XCTAssertEqual(segments.frame.height, 26, accuracy: 1, "Segments are panel-height controls")
        XCTAssertEqual(dropdown.frame.height, 26, accuracy: 1, "Dropdowns are panel-height controls")
        XCTAssertEqual(segments.frame.midY, dropdown.frame.midY, accuracy: 1, "Segments center with the dropdown")
        attachEditor(in: app, name: "photo-selection-options-\(theme)")
        workspaceActivate(more)
        XCTAssertTrue(app.descendants(matching: .any)["tool-drawer"].firstMatch.waitForExistence(timeout: 10), "More opens the tool drawer")
        attachEditor(in: app, name: "photo-tool-options-drawer-\(theme)")
        workspaceActivate(sketch)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: sketch)
        waitForExpectations(timeout: 10)
        expectation(for: NSPredicate(format: "count == 2"), evaluatedWith: sliders)
        waitForExpectations(timeout: 15)
        sketchTool("drawing_brush")
        XCTAssertTrue(editorText(app.buttons["header-tool-drawing_brush"]).hasPrefix("Pencil"), "The drawing slot restores the retained Pencil choice")
        tap(strength, CGVector(dx: 0.5, dy: 0.5))
        XCTAssertTrue(preview.waitForExistence(timeout: 10))
        editorMenu(in: app, menu: "File", id: "new_document", label: "New…")
        let create = app.buttons["new-drawing-create"]
        XCTAssertTrue(create.waitForExistence(timeout: 15)); workspaceActivate(create)
        XCTAssertTrue(create.waitForNonExistence(timeout: 60))
        XCTAssertTrue(preview.waitForNonExistence(timeout: 10), "A new drawing retires the old preview")
        tap(strength, CGVector(dx: 0.5, dy: 0.5))
        XCTAssertTrue(preview.waitForExistence(timeout: 15), "The current drawing can request a fresh stamp")
        attachEditor(in: app, name: "stamp-new-drawing-\(theme)")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
