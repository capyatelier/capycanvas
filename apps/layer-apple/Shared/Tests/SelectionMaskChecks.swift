import XCTest

extension XCTestCase {
    @MainActor func checkSelectionMasks(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch(); capturePaintEditor(in: app)
        let photo = app.buttons["workspace-switch-builtin:workspace:photographer"]
        workspaceActivate(photo)
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: photo)
        waitForExpectations(timeout: 10)
        for label in ["Rectangle select", "Ellipse select", "Polygonal lasso", "Select by color"] {
            XCTAssertTrue(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-toolbar-", label)).firstMatch.waitForExistence(timeout: 10), "Photo toolbar has \(label)")
        }
        editorTool("Rectangle select", in: app)
        let settings = app.buttons["column-icon-tool_settings"]
        if settings.waitForExistence(timeout: 5) { workspaceActivate(settings) }
        let modes = ["selection_new", "selection_add", "selection_subtract", "selection_intersect"]
            .map { app.buttons["tool-action-" + $0] }
        for mode in modes { XCTAssertTrue(mode.waitForExistence(timeout: 10)) }
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: modes[0])
        waitForExpectations(timeout: 5)
        let viewport = workspaceViewport(in: app)
        func drag(_ from: CGVector, _ to: CGVector) {
            let start = viewport.coordinate(withNormalizedOffset: from), end = viewport.coordinate(withNormalizedOffset: to)
            #if os(macOS)
            start.click(forDuration: 0.05, thenDragTo: end)
            #else
            start.press(forDuration: 0.05, thenDragTo: end)
            #endif
        }
        drag(CGVector(dx: 0.4, dy: 0.4), CGVector(dx: 0.5, dy: 0.55))
        workspaceActivate(modes[1])
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: modes[1])
        waitForExpectations(timeout: 5)
        drag(CGVector(dx: 0.52, dy: 0.45), CGVector(dx: 0.6, dy: 0.6))
        attachEditor(in: app, name: "selection-add")
        workspaceActivate(modes[0])
        workspaceActivate(app.buttons["selection-menu-selection"])
        let grow = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Grow")).firstMatch
        workspaceActivate(grow)
        let apply = app.buttons["selection-resize-apply"]
        XCTAssertTrue(apply.waitForExistence(timeout: 10), "Grow opens the distance dialog")
        attachEditor(in: app, name: "selection-grow-dialog")
        workspaceActivate(apply)
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: apply)
        waitForExpectations(timeout: 10)
        editorMenu(in: app, menu: "Select", id: "quick_mask", label: "Quick Mask")
        let quick = app.descendants(matching: .any)["layer-row-0"].firstMatch
        XCTAssertTrue(quick.waitForExistence(timeout: 10), "Quick Mask adds a temporary Layers row")
        XCTAssertTrue(app.buttons["selection-load-0"].waitForExistence(timeout: 5))
        let thumbnail = app.buttons["layer-thumbnail-0-content"]
        expectation(for: NSPredicate(format: "value == %@", "Preview ready"), evaluatedWith: thumbnail)
        waitForExpectations(timeout: 20)
        attachEditor(in: app, name: "quick-mask")
        workspaceActivate(app.buttons["selection-load-0"])
        expectation(for: NSPredicate(format: "exists == NO"), evaluatedWith: quick)
        waitForExpectations(timeout: 10)
        let rows = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let count = rows.count
        workspaceActivate(app.buttons["layer-New Selection Layer"])
        expectation(for: NSPredicate(format: "count == %d", count + 1), evaluatedWith: rows)
        waitForExpectations(timeout: 10)
        let load = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "selection-load-")).firstMatch
        XCTAssertTrue(load.waitForExistence(timeout: 10), "Selection layers show the Load control")
        attachEditor(in: app, name: "selection-layer")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }
}
