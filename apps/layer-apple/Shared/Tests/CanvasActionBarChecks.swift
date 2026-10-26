import XCTest

extension XCTestCase {
    @MainActor func activateCanvasBarAction(_ id: String, label: String, in app: XCUIApplication) {
        let action = app.buttons["canvas-bar-action-" + id]
        if action.waitForExistence(timeout: 5) { workspaceActivate(action); return }
        let menu = app.descendants(matching: .any)["canvas-bar-menu"].firstMatch
        workspaceActivate(app.buttons["canvas-bar-more"])
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        let item = menu.buttons["menu-action-" + label]
        revealEditorControl(item, in: menu); workspaceActivate(item)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
    }
    @MainActor func checkCanvasActionBar(in app: XCUIApplication, theme: String = "light") {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app, theme: theme)
        editorMenu(in: app, menu: "File", id: "new_document", label: "New drawing")
        XCTAssertTrue(app.buttons["new-drawing-create"].waitForExistence(timeout: 60))
        workspaceActivate(app.buttons["new-drawing-texture"])
        let usePaper = app.buttons["new-drawing-paper-use"]
        XCTAssertTrue(usePaper.waitForExistence(timeout: 10))
        enterNewDrawingNumber("paper-relief", "0", in: app)
        workspaceActivate(usePaper)
        XCTAssertTrue(usePaper.waitForNonExistence(timeout: 10))
        workspaceActivate(app.buttons["new-drawing-paper-color"])
        let useColor = app.buttons["new-drawing-color-use"]
        XCTAssertTrue(useColor.waitForExistence(timeout: 10))
        let white = app.buttons["new-drawing-paper-color-0"]
        XCTAssertEqual(white.label, "White")
        workspaceActivate(white)
        workspaceActivate(useColor)
        XCTAssertTrue(useColor.waitForNonExistence(timeout: 10))
        createNewDrawing(width: "1440", height: "1080", depth: 0, in: app)
        expectation(for: NSPredicate { _, _ in
            self.editorPixels(in: app).enumerated().allSatisfy { $0.offset % 4 == 3 || $0.element > 250 }
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)

        let window = workspaceViewport(in: app)
        let bar = app.descendants(matching: .any)["canvas-action-bar"].firstMatch
        let more = app.buttons["canvas-bar-more"]
        let menu = app.descendants(matching: .any)["canvas-bar-menu"].firstMatch
        let capy = app.descendants(matching: .any)["zen-button"].firstMatch
        func action(_ id: String) -> XCUIElement { app.buttons["canvas-bar-action-" + id] }
        func expect(_ element: XCUIElement, _ format: String, _ message: String = "") {
            let match = XCTNSPredicateExpectation(predicate: NSPredicate(format: format), object: element)
            if XCTWaiter.wait(for: [match], timeout: 10) != .completed {
                attachEditor(in: app, name: "canvas-bar-unexpected-\(theme)")
                XCTFail("\(element) \(format) \(message)")
            }
        }
        func drag(_ from: CGPoint, _ to: CGPoint) {
            let start = window.coordinate(withNormalizedOffset: CGVector(dx: from.x, dy: from.y))
            let end = window.coordinate(withNormalizedOffset: CGVector(dx: to.x, dy: to.y))
            #if os(macOS)
            start.click(forDuration: 0.05, thenDragTo: end)
            #else
            start.press(forDuration: 0.05, thenDragTo: end)
            #endif
        }
        func press(_ point: CGPoint) {
            let target = window.coordinate(withNormalizedOffset: .zero)
                .withOffset(CGVector(dx: point.x - window.frame.minX, dy: point.y - window.frame.minY))
            #if os(macOS)
            target.click()
            #else
            target.tap()
            #endif
        }
        func historyState() -> [Bool] {
            ["Undo", "Redo"].map { label in
                let buttons = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", label))
                XCTAssertTrue(buttons.firstMatch.waitForExistence(timeout: 5)); XCTAssertEqual(buttons.count, 1)
                return buttons.firstMatch.isEnabled
            }
        }
        var moreFrame = CGRect.zero
        func openMore() {
            XCTAssertTrue(more.waitForExistence(timeout: 10)); moreFrame = more.frame
            workspaceActivate(more); expect(menu, "exists == YES")
        }
        func closeMore() {
            press(CGPoint(x: moreFrame.midX, y: moreFrame.midY)); expect(menu, "exists == NO", "More toggles closed")
        }
        func activate(_ item: XCUIElement, label: String? = nil) {
            revealEditorControl(item, in: menu)
            if let label { XCTAssertEqual(item.label, label) }
            workspaceActivate(item)
        }
        func perform(_ id: String, _ label: String, menuItem: String) {
            if action(id).waitForExistence(timeout: 2) { workspaceActivate(action(id)); return }
            openMore(); activate(menu.buttons[menuItem], label: label); expect(menu, "exists == NO")
        }
        func offered(_ id: String, _ label: String) -> Bool {
            if action(id).exists { return true }
            openMore(); defer { closeMore() }
            return menu.buttons["menu-action-" + label].waitForExistence(timeout: 3)
        }
        func choose(_ group: String, _ title: String, index: Int, _ label: String, menuItem: String) {
            let segment = app.buttons["canvas-bar-segment-\(group)-\(index)"], dropdown = app.buttons["canvas-bar-choice-" + group]
            if segment.exists {
                workspaceActivate(segment); expect(segment, "selected == YES")
                XCTAssertEqual(segment.frame.height, 26, accuracy: 1, "Worded radios are panel-height controls")
                return
            }
            if dropdown.exists {
                workspaceActivate(dropdown)
                workspaceActivate(app.buttons["canvas-bar-choice-\(group)-\(index)"])
                expect(dropdown, "value == '\(label)'")
                return
            }
            openMore()
            activate(menu.buttons["menu-action-" + title])
            activate(menu.buttons[menuItem], label: label)
            expect(menu, "exists == NO")
            openMore(); activate(menu.buttons["menu-action-" + title])
            expect(menu.buttons[menuItem], "selected == YES", "\(title) shows \(label)")
            closeMore()
        }
        func chooseFromMenu(_ id: String, _ title: String, _ label: String) {
            let button = app.buttons["canvas-bar-menu-" + id]
            if button.exists {
                workspaceActivate(button)
                let items = app.descendants(matching: .any)["canvas-bar-menu-items-" + id]
                expect(items, "exists == YES", "The \(title) menu opens from the bar")
                workspaceActivate(items.buttons["menu-action-" + label]); expect(items, "exists == NO")
                return
            }
            openMore(); activate(menu.buttons["menu-action-" + title]); activate(menu.buttons["menu-action-" + label])
            expect(menu, "exists == NO")
        }
        func resetTransform() {
            editorMenu(in: app, menu: "Edit", id: "search_commands", label: "Search Commands…")
            let search = app.textFields["command-search"]
            XCTAssertTrue(search.waitForExistence(timeout: 5), "Reset is a command, not a bar item")
            search.typeText("Reset transform\n")
            XCTAssertTrue(search.waitForNonExistence(timeout: 5))
        }

        #if os(macOS)
        let narrow = window.frame.width
        func fullscreen(_ enter: Bool) {
            workspaceActivate(app.menuBars.menuBarItems["View"])
            workspaceActivate(app.menuItems[enter ? "Enter Full Screen" : "Exit Full Screen"].firstMatch)
            expectation(for: NSPredicate { _, _ in (window.frame.width > narrow + 200) == enter }, evaluatedWith: app)
            waitForExpectations(timeout: 15)
        }
        addTeardownBlock { await MainActor.run { if window.frame.width > narrow + 200 { fullscreen(false) } } }
        #endif

        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        XCTAssertFalse(bar.waitForExistence(timeout: 2), "Painting tools hide the selection bar")
        editorMenu(in: app, menu: "Select", id: "deselect", label: "Deselect pixels")
        editorTool("Operation", in: app); editorChoice("Operation", group: true, in: app)
        XCTAssertFalse(bar.exists, "No bar without a selection")
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        expect(action("deselect"), "exists == YES AND enabled == YES", "Select all shows the selection bar")
        XCTAssertTrue(window.frame.contains(bar.frame), "\(bar.frame) inside \(window.frame)")
        XCTAssertEqual(bar.frame.height, 36, accuracy: 1, "The bar is one strip of Small tiles")
        XCTAssertEqual(action("deselect").frame.width, 36, accuracy: 1, "Icon items are square tiles")
        attachEditor(in: app, name: "canvas-bar-selection-\(theme)")
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        expect(action("deselect"), "exists == YES AND enabled == YES")
        let unfilledPixels = editorPixels(in: app)
        perform("fill_selection", "Fill selection", menuItem: "menu-action-Fill selection")
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app); return Int(pixel[2]) > Int(pixel[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        expect(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-tile-", "Undo")).firstMatch, "enabled == YES")
        let filledPixels = editorPixels(in: app), filledCamera = app.buttons["camera-status"].label
        let filledHistory = historyState()
        func checkFillHistory() {
            editorHistory("Undo", in: app); expectPixels(unfilledPixels, in: app)
            editorHistory("Redo", in: app); expectPixels(filledPixels, in: app)
            XCTAssertEqual(historyState(), filledHistory)
            expect(action("deselect"), "exists == YES AND enabled == YES")
        }
        press(CGPoint(x: action("deselect").frame.minX - 6, y: bar.frame.midY))
        expect(action("deselect"), "exists == YES", "A tap on the divider before Deselect keeps the selection")
        let divider = window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: action("deselect").frame.minX - 6 - window.frame.minX, dy: bar.frame.midY - window.frame.minY))
        #if os(macOS)
        divider.click(forDuration: 0.8, thenDragTo: divider)
        #else
        divider.press(forDuration: 0.8)
        #endif
        expect(action("deselect"), "exists == YES AND enabled == YES", "Holding the mounted divider keeps the selection")
        XCTAssertFalse(menu.exists)
        XCTAssertEqual(app.buttons["camera-status"].label, filledCamera)
        XCTAssertEqual(editorPixels(in: app), filledPixels)
        XCTAssertEqual(historyState(), filledHistory)
        #if os(macOS)
        checkFillHistory()
        #endif
        #if os(macOS)
        let selectionActions = ["scale_rotate", "fill_selection", "clear_selected", "invert_selection", "deselect"]
        func onBar() -> Int { selectionActions.filter { action($0).exists }.count }
        let windowed = onBar()
        fullscreen(true)
        expectation(for: NSPredicate { _, _ in windowed < selectionActions.count ? onBar() > windowed : onBar() == windowed },
            evaluatedWith: bar)
        waitForExpectations(timeout: 10)
        #endif
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        #if os(iOS)
        let camera = app.buttons["camera-status"], canvas = app.descendants(matching: .any)["canvas"].firstMatch
        let beforePinch = camera.label
        editorTool("Lasso selection", in: app); editorChoice("Lasso selection", in: app)
        expect(camera, "label == '\(beforePinch)'", "The navigation fixture preserves the fitted camera")
        expect(bar, "exists == YES")
        withHiddenChrome(in: app) {
            expect(action("deselect"), "exists == YES AND enabled == YES")
            let geometry = XCTAttachment(string: "window=\(window.frame) canvas=\(canvas.frame) bar=\(bar.frame) capy=\(capy.frame)\n\(app.debugDescription)")
            geometry.name = "canvas-bar-before-pinch-\(theme)"; geometry.lifetime = .keepAlways; add(geometry)
            attachEditor(in: app, name: "canvas-bar-pinch-mounted-\(theme)")
            canvas.pinch(withScale: 3, velocity: 10)
        }
        expect(camera, "label != '\(beforePinch)'", "The native pinch changes the camera")
        editorTool("Operation", in: app); editorChoice("Operation", group: true, in: app)
        expect(action("deselect"), "exists == YES AND enabled == YES")
        editorMenu(in: app, menu: "View", id: "reset_rotation", label: "Reset rotation")
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        expect(camera, "label == '\(beforePinch)'", "Fit restores the camera before the original bar journey")
        expectPixels(filledPixels, in: app)
        XCTAssertEqual(historyState(), filledHistory)
        checkFillHistory()
        #endif
        for id in ["copy", "adjust", "mask", "refine"] {
            let button = app.buttons["canvas-bar-menu-" + id], items = app.descendants(matching: .any)["canvas-bar-menu-items-" + id]
            if button.exists {
                let frame = button.frame
                workspaceActivate(button)
                expect(items, "exists == YES", "The \(id) menu opens from the bar")
                XCTAssertGreaterThan(items.buttons.count, 0, "The \(id) menu lists actions")
                press(CGPoint(x: frame.midX, y: frame.midY)); expect(items, "exists == NO")
            } else {
                openMore(); defer { closeMore() }
                XCTAssertTrue(menu.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "menu-action-")).count > 0)
            }
        }
        let refine = app.buttons["canvas-bar-menu-refine"]
        if refine.exists {
            func choose(_ label: String) {
                workspaceActivate(refine)
                let items = app.descendants(matching: .any)["canvas-bar-menu-items-refine"]
                expect(items, "exists == YES")
                workspaceActivate(items.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", label)).firstMatch)
            }
            choose("Feather")
            let panel = app.descendants(matching: .any)["selection-refine-panel"].firstMatch
            expect(panel, "exists == YES", "Feather opens the Refine panel")
            workspaceActivate(app.buttons["number-value-selection-refine-value"])
            app.textFields["number-entry-selection-refine-value"].typeText("2\n")
            attachEditor(in: app, name: "canvas-bar-refine-\(theme)")
            workspaceActivate(app.buttons["selection-refine-apply"])
            expect(panel, "exists == NO", "Apply closes the Refine panel")
            choose("Transform Outline")
            expect(action("apply_transform"), "exists == YES", "Transform Outline shows the transform bar")
            workspaceActivate(action("apply_transform"))
            expect(action("deselect"), "exists == YES", "Applying the outline returns to the selection bar")
        }

        let paper = bluePaperBounds(in: app)
        let sampleY = paper.midY + paper.height * 0.1
        let sides = [CGPoint(x: paper.minX + paper.width * 0.05, y: sampleY), CGPoint(x: paper.midX, y: sampleY),
            CGPoint(x: paper.maxX - paper.width * 0.05, y: sampleY)]
        let filled = [true, true, true], turned = [false, true, false]
        for apply in [false, true] {
            var hull = CGRect.zero
            if !apply {
                for _ in 0..<2 { editorMenu(in: app, menu: "View", id: "zoom_out", label: "Zoom out") }
                hull = bluePaperBounds(in: app)
            }
            perform("scale_rotate", "Transform", menuItem: "menu-action-Transform")
            expect(action("apply_transform"), "exists == YES", "Transform replaces the selection bar")
            XCTAssertFalse(action("deselect").exists)
            if !apply {
                choose("transform-mode", "Mode", index: 1, "Distort", menuItem: "menu-action-Distort")
                XCTAssertTrue(offered("transform_perspective", "Perspective"), "Distort offers Perspective")
                XCTAssertFalse(offered("transform_uniform", "Keep proportions"), "Keep proportions belongs to Scale")
                choose("transform-mode", "Mode", index: 0, "Scale", menuItem: "menu-action-Scale")
                XCTAssertFalse(offered("transform_perspective", "Perspective"))
                XCTAssertTrue(offered("transform_uniform", "Keep proportions"), "Scale offers Keep proportions")
                choose("transform-interpolation", "Interpolation", index: 0, "Nearest neighbor", menuItem: "command-transform_nearest")
                attachEditor(in: app, name: "canvas-bar-transform-\(theme)")
                choose("transform-mode", "Mode", index: 2, "Warp", menuItem: "menu-action-Warp")
                chooseFromMenu("warp_grid", "Grid", "3 × 3 warp grid")
                let node = CGPoint(x: hull.minX + hull.width / 3, y: hull.minY)
                let probes = [CGPoint(x: node.x + hull.width * 0.08, y: hull.minY + hull.height * 0.06),
                    CGPoint(x: hull.minX + hull.width * 0.9, y: hull.minY + hull.height * 0.06)]
                expectBluePaper([true, true], at: probes, in: app)
                drag(node, CGPoint(x: node.x, y: hull.minY + hull.height * 0.3))
                expectBluePaper([false, true], at: probes, in: app)
                attachEditor(in: app, name: "canvas-bar-warp-\(theme)")
                resetTransform()
                expectBluePaper([true, true], at: probes, in: app)
                XCTAssertFalse(app.buttons["recovery-retry"].exists, "A long transform defers session checkpoints")
                editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
                expectBluePaper(filled, at: sides, in: app)
            }
            perform("transform_rotate_right", "Rotate 90° right", menuItem: "command-transform_rotate_right")
            expectBluePaper(turned, at: sides, in: app)
            workspaceActivate(action(apply ? "apply_transform" : "cancel_transform"))
            expect(action("apply_transform"), "exists == NO")
            expect(action("deselect"), "exists == YES", "The selection bar returns after the transform")
            expectBluePaper(apply ? turned : filled, at: sides, in: app)
        }
        editorHistory("Undo", in: app); expectBluePaper(filled, at: sides, in: app)
        editorHistory("Redo", in: app); expectBluePaper(turned, at: sides, in: app)

        perform("scale_rotate", "Transform", menuItem: "menu-action-Transform")
        expect(action("apply_transform"), "exists == YES")
        openMore()
        expect(menu.buttons["command-show_canvas_action_bar"], "selected == YES", "More ends with the bar toggle")
        closeMore()
        XCTAssertTrue(action("apply_transform").exists, "More keeps the transform")
        openMore()
        activate(menu.buttons["command-show_canvas_action_bar"])
        expect(action("transform_flip_horizontal"), "exists == NO", "Hiding the bar leaves Cancel and Apply")
        expect(action("apply_transform"), "exists == YES")
        attachEditor(in: app, name: "canvas-bar-completion-only-\(theme)")
        workspaceActivate(action("cancel_transform"))
        expect(bar, "exists == NO", "No selection bar while the toggle is off")
        editorMenu(in: app, menu: "View", id: "show_canvas_action_bar", label: "Show canvas action bar")
        expect(action("deselect"), "exists == YES", "The toggle restores the selection bar")

        let undo = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-commands-", "Undo")).firstMatch
        expect(undo, "exists == YES", "The docked Undo is visible before Zen")
        let undoEnabled = undo.isEnabled
        withHiddenChrome(in: app) {
            expectation(for: NSPredicate { _, _ in !undo.exists || !undo.isEnabled }, evaluatedWith: undo)
            waitForExpectations(timeout: 10)
            XCTAssertTrue(action("deselect").exists, "Zen keeps the bar")
        }
        expect(undo, "exists == YES")
        if undoEnabled { expect(undo, "enabled == YES") }
        workspaceActivate(action("deselect"))
        expect(bar, "exists == NO", "Deselect removes the bar")

        #if os(macOS)
        let photo = app.buttons["workspace-switch-builtin:workspace:photographer"]
        workspaceActivate(photo)
        expect(photo, "selected == YES")
        editorTool("Rectangle select", in: app)
        let blue = bluePaperBounds(in: app), frame = window.frame
        let from = CGVector(dx: blue.minX + blue.width * 0.3, dy: blue.minY + blue.height * 0.2)
        let to = CGVector(dx: blue.minX + blue.width * 0.55, dy: blue.minY + blue.height * 0.45)
        window.coordinate(withNormalizedOffset: from).click(forDuration: 0.05, thenDragTo: window.coordinate(withNormalizedOffset: to))
        expect(action("deselect"), "exists == YES", "A rectangle selection shows the selection bar")
        let selection = CGRect(x: frame.minX + from.dx * frame.width, y: frame.minY + from.dy * frame.height,
            width: (to.dx - from.dx) * frame.width, height: (to.dy - from.dy) * frame.height)
        XCTAssertGreaterThanOrEqual(bar.frame.minY, selection.maxY - 2, "The bar sits below the selection")
        XCTAssertLessThan(bar.frame.minY - selection.maxY, 60)
        XCTAssertTrue(bar.frame.minX <= selection.midX && selection.midX <= bar.frame.maxX,
            "The bar centres on the selection as far as the work area allows")
        attachEditor(in: app, name: "canvas-bar-beside-selection-\(theme)")
        let placed = bar.frame
        editorMenu(in: app, menu: "View", id: "zoom_out", label: "Zoom out")
        expectation(for: NSPredicate { _, _ in
            bar.exists && max(abs(bar.frame.minY - placed.minY), abs(bar.frame.midX - placed.midX)) > 5
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "canvas-bar-after-zoom-\(theme)")
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
