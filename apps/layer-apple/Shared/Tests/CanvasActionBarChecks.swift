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
    @MainActor func checkCanvasActionBar(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"set_color","rgba":[0.2,0.45,0.8,1]}]"#
        app.launch(); capturePaintEditor(in: app)
        let window = workspaceViewport(in: app)
        let bar = app.descendants(matching: .any)["canvas-action-bar"].firstMatch
        let more = app.buttons["canvas-bar-more"]
        let menu = app.descendants(matching: .any)["canvas-bar-menu"].firstMatch
        let capy = app.descendants(matching: .any)["zen-button"].firstMatch
        func action(_ id: String) -> XCUIElement { app.buttons["canvas-bar-action-" + id] }
        func expect(_ element: XCUIElement, _ format: String, _ message: String = "") {
            let match = XCTNSPredicateExpectation(predicate: NSPredicate(format: format), object: element)
            if XCTWaiter.wait(for: [match], timeout: 10) != .completed {
                attachEditor(in: app, name: "canvas-bar-unexpected")
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
        var moreFrame = CGRect.zero
        func openMore() {
            XCTAssertTrue(more.waitForExistence(timeout: 10)); moreFrame = more.frame
            workspaceActivate(more); expect(menu, "exists == YES")
        }
        func closeMore() {
            press(CGPoint(x: moreFrame.midX, y: moreFrame.midY)); expect(menu, "exists == NO", "More toggles closed")
        }
        func activate(_ item: XCUIElement) {
            revealEditorControl(item, in: menu); workspaceActivate(item)
        }
        func perform(_ id: String, _ label: String) {
            if action(id).waitForExistence(timeout: 2) { workspaceActivate(action(id)); return }
            openMore(); activate(menu.buttons["menu-action-" + label]); expect(menu, "exists == NO")
        }
        func offered(_ id: String, _ label: String) -> Bool {
            if action(id).exists { return true }
            openMore(); defer { closeMore() }
            return menu.buttons["menu-action-" + label].waitForExistence(timeout: 3)
        }
        func choose(_ group: String, _ title: String, index: Int, _ label: String, segmented: Bool) {
            let control = app.buttons[segmented ? "canvas-bar-segment-\(group)-\(index)" : "canvas-bar-choice-" + group]
            if control.exists {
                workspaceActivate(control)
                if segmented { expect(control, "selected == YES") } else {
                    workspaceActivate(app.buttons["canvas-bar-choice-\(group)-\(index)"])
                    expect(control, "value == '\(label)'")
                }
                return
            }
            openMore()
            activate(menu.buttons["menu-action-" + title])
            activate(menu.buttons["menu-action-" + label])
            expect(menu, "exists == NO")
            openMore(); activate(menu.buttons["menu-action-" + title])
            expect(menu.buttons["menu-action-" + label], "selected == YES", "\(title) shows \(label)")
            closeMore()
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
        editorTool("Operation", in: app); editorChoice("Move", group: true, in: app)
        XCTAssertFalse(bar.exists, "No bar without a selection")
        editorMenu(in: app, menu: "Select", id: "select_all", label: "Select all pixels")
        expect(action("deselect"), "exists == YES AND enabled == YES", "Select all shows the selection bar")
        XCTAssertTrue(window.frame.contains(bar.frame), "\(bar.frame) inside \(window.frame)")
        XCTAssertEqual(bar.frame.height, 52, accuracy: 1)
        attachEditor(in: app, name: "canvas-bar-selection")
        perform("fill_selection", "Fill selection")
        expectation(for: NSPredicate { _, _ in
            let pixel = self.editorPixels(in: app); return Int(pixel[2]) > Int(pixel[0]) + 50
        }, evaluatedWith: app)
        waitForExpectations(timeout: 15)
        press(CGPoint(x: bar.frame.minX + 3, y: bar.frame.midY))
        expect(action("deselect"), "exists == YES", "A tap on the bar padding keeps the selection")
        #if os(macOS)
        let shown = bar.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "canvas-bar-action-"))
        let windowed = shown.count
        fullscreen(true)
        expectation(for: NSPredicate { _, _ in shown.count > windowed }, evaluatedWith: bar)
        waitForExpectations(timeout: 10)
        #endif
        editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
        for id in ["refine", "adjust"] {
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
            workspaceActivate(app.buttons["number-increase-selection-refine-value"])
            attachEditor(in: app, name: "canvas-bar-refine")
            workspaceActivate(app.buttons["selection-refine-apply"])
            expect(panel, "exists == NO", "Apply closes the Refine panel")
            choose("Transform Outline")
            expect(action("apply_transform"), "exists == YES", "Transform Outline shows the transform bar")
            workspaceActivate(action("apply_transform"))
            expect(action("deselect"), "exists == YES", "Applying the outline returns to the selection bar")
        }

        let paper = bluePaperBounds(in: app)
        let sides = [CGPoint(x: paper.minX + paper.width * 0.05, y: paper.midY), CGPoint(x: paper.midX, y: paper.midY),
            CGPoint(x: paper.maxX - paper.width * 0.05, y: paper.midY)]
        let filled = [true, true, true], turned = [false, true, false]
        for apply in [false, true] {
            var hull = CGRect.zero
            if !apply {
                for _ in 0..<2 { editorMenu(in: app, menu: "View", id: "zoom_out", label: "Zoom out") }
                hull = bluePaperBounds(in: app)
            }
            perform("scale_rotate", "Transform")
            expect(action("apply_transform"), "exists == YES", "Transform replaces the selection bar")
            XCTAssertFalse(action("deselect").exists)
            if !apply {
                choose("transform-mode", "Mode", index: 2, "Distort", segmented: true)
                XCTAssertTrue(offered("transform_perspective", "Perspective"), "Distort offers Perspective")
                choose("transform-mode", "Mode", index: 0, "Free", segmented: true)
                XCTAssertFalse(offered("transform_perspective", "Perspective"))
                choose("transform-interpolation", "Interpolation", index: 0, "Nearest", segmented: false)
                attachEditor(in: app, name: "canvas-bar-transform")
                choose("transform-mode", "Mode", index: 3, "Warp", segmented: true)
                choose("transform-warp-grid", "Grid", index: 0, "3 × 3", segmented: false)
                let node = CGPoint(x: hull.minX + hull.width / 3, y: hull.minY)
                let probes = [CGPoint(x: node.x + hull.width * 0.08, y: hull.minY + hull.height * 0.06),
                    CGPoint(x: hull.minX + hull.width * 0.9, y: hull.minY + hull.height * 0.06)]
                expectBluePaper([true, true], at: probes, in: app)
                drag(node, CGPoint(x: node.x, y: hull.minY + hull.height * 0.3))
                expectBluePaper([false, true], at: probes, in: app)
                attachEditor(in: app, name: "canvas-bar-warp")
                perform("reset_transform", "Reset transform")
                expectBluePaper([true, true], at: probes, in: app)
                editorMenu(in: app, menu: "View", id: "fit_canvas", label: "Fit canvas")
                expectBluePaper(filled, at: sides, in: app)
            }
            perform("transform_rotate_right", "Rotate 90° right")
            expectBluePaper(turned, at: sides, in: app)
            workspaceActivate(action(apply ? "apply_transform" : "cancel_transform"))
            expect(action("apply_transform"), "exists == NO")
            expect(action("deselect"), "exists == YES", "The selection bar returns after the transform")
            expectBluePaper(apply ? turned : filled, at: sides, in: app)
        }
        editorHistory("Undo", in: app); expectBluePaper(filled, at: sides, in: app)
        editorHistory("Redo", in: app); expectBluePaper(turned, at: sides, in: app)

        perform("scale_rotate", "Transform")
        expect(action("apply_transform"), "exists == YES")
        openMore()
        expect(menu.buttons["command-show_canvas_action_bar"], "selected == YES", "More ends with the bar toggle")
        closeMore()
        XCTAssertTrue(action("apply_transform").exists, "More keeps the transform")
        openMore()
        activate(menu.buttons["command-show_canvas_action_bar"])
        expect(action("transform_rotate_right"), "exists == NO", "Hiding the bar leaves Cancel and Apply")
        expect(action("apply_transform"), "exists == YES")
        attachEditor(in: app, name: "canvas-bar-completion-only")
        workspaceActivate(action("cancel_transform"))
        expect(bar, "exists == NO", "No selection bar while the toggle is off")
        editorMenu(in: app, menu: "View", id: "show_canvas_action_bar", label: "Show canvas action bar")
        expect(action("deselect"), "exists == YES", "The toggle restores the selection bar")

        let undo = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "toolbar-tile-commands-", "Undo")).firstMatch
        workspaceActivate(capy)
        expect(undo, "exists == NO", "Zen hides the docked chrome")
        XCTAssertTrue(action("deselect").exists, "Zen keeps the bar")
        workspaceActivate(capy)
        expect(undo, "exists == YES")
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
        attachEditor(in: app, name: "canvas-bar-beside-selection")
        let placed = bar.frame
        editorMenu(in: app, menu: "View", id: "zoom_out", label: "Zoom out")
        expectation(for: NSPredicate { _, _ in
            bar.exists && max(abs(bar.frame.minY - placed.minY), abs(bar.frame.midX - placed.midX)) > 5
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "canvas-bar-after-zoom")
        #endif
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
