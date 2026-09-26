import XCTest

#if os(macOS)
extension XCUIElement { @MainActor func clickOrTap() { click() } }
extension XCUICoordinate { @MainActor func clickOrTap() { click() } }
#else
extension XCUIElement { @MainActor func clickOrTap() { tap() } }
extension XCUICoordinate { @MainActor func clickOrTap() { tap() } }
#endif

extension XCTestCase {
    @MainActor func workspaceViewport(in app: XCUIApplication) -> XCUIElement {
        // Child frames use screen coordinates in windowed iPad scenes, while
        // the application frame can retain a zero origin. Use the editor window.
        return app.windows.firstMatch
    }
    @MainActor func workspaceActivate(_ element: XCUIElement) {
        XCTAssertTrue(element.waitForExistence(timeout: 10))
        #if os(macOS)
        if element.isHittable {
            element.click()
        } else {
            // SwiftUI's offset controls inside a scroll view can have a valid
            // visible frame without an XCTest hit point. Use measured editor
            // bounds for this in-app fallback; resulting state is still checked.
            let window = XCUIApplication().windows.firstMatch
            let frame = element.frame
            guard frame.width > 0, frame.height > 0,
                frame.midX.isFinite, frame.midY.isFinite, window.frame.contains(frame) else {
                XCTFail("The workspace control must have finite bounds inside its editor window")
                return
            }
            window.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: frame.midX - window.frame.minX, dy: frame.midY - window.frame.minY)).click()
        }
        #else
        element.tap()
        #endif
    }
    @MainActor private func workspaceText(_ field: XCUIElement, _ value: String) {
        workspaceActivate(field)
        field.typeKey("a", modifierFlags: .command)
        field.typeText(value)
    }
    @MainActor func checkCollapsedColumnsDrawersAndZen(in app: XCUIApplication) {
        workspaceActivate(app.buttons["column-icon-toolbar"])
        let column = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "column-connection-4-")).firstMatch
        XCTAssertTrue(column.waitForExistence(timeout: 10), "The collapsed column opens as its stacked groups")
        let group = app.descendants(matching: .any)["workspace-group-6"].firstMatch
        workspaceActivate(group.buttons["panel-tab-brushes"])
        XCTAssertTrue(group.buttons["brush-1"].waitForExistence(timeout: 5))
        workspaceActivate(group.buttons["panel-tab-toolbar"])
        let pen = group.buttons["toolbar-tile-toolbar-1"]
        workspaceActivate(pen)
        let drawer = app.descendants(matching: .any)["tool-drawer"].firstMatch
        if !drawer.waitForExistence(timeout: 2) { workspaceActivate(pen) }
        XCTAssertTrue(drawer.waitForExistence(timeout: 10))
        XCTAssertTrue(workspaceViewport(in: app).frame.contains(drawer.frame), "The shared drawer bounds must stay within the viewport")
        XCTAssertTrue(drawer.buttons["brush-1"].exists)
        let window = workspaceViewport(in: app).frame, bounds = drawer.frame
        let outside = workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(
            dx: (bounds.midX - window.minX) / window.width, dy: min(0.95, (bounds.maxY + 40 - window.minY) / window.height)))
        outside.clickOrTap()
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: drawer)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(column.exists, "Open columns use explicit dismissal")
        XCTAssertFalse(app.buttons["expand-column-4"].exists)
        workspaceActivate(app.buttons["column-icon-toolbar"])
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: column)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(app.buttons["column-icon-toolbar"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachWorkspaceScreen(app, name: "columns-drawers-zen")
    }
    @MainActor func checkToolbarCustomization(in app: XCUIApplication) {
        XCTAssertTrue(app.textFields["toolbar-name"].waitForExistence(timeout: 20))
        for query in ["Undo", "Divider", "Manage Toolbars"] {
            workspaceText(app.textFields["tool-picker-search"], query)
            workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "tool-choice-" + query)).firstMatch)
        }
        attachWorkspaceScreen(app, name: "toolbar-tool-picker")
        workspaceActivate(app.buttons["tool-picker-confirm"])
        let created = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-options-", "Toolbar options for Toolbar 1")).firstMatch
        XCTAssertTrue(created.waitForExistence(timeout: 10))
        let identifier = created.identifier
        let panel = String(identifier.dropFirst("toolbar-options-".count))
        let options = app.descendants(matching: .any)[identifier].firstMatch
        func menu(_ prefix: String) {
            workspaceActivate(options)
            workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "menu-action-" + prefix)).firstMatch)
        }
        menu("Rename ")
        workspaceText(app.textFields["toolbar-name"], "Quick tools")
        attachWorkspaceScreen(app, name: "toolbar-rename-prompt")
        workspaceActivate(app.buttons["toolbar-prompt-confirm"])
        menu("Duplicate ")
        workspaceText(app.textFields["toolbar-name"], "Copy tools")
        workspaceActivate(app.buttons["toolbar-prompt-confirm"])
        let copied = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@", "toolbar-options-", "Toolbar options for Copy tools")).firstMatch
        XCTAssertTrue(copied.waitForExistence(timeout: 10))
        let copyIdentifier = copied.identifier
        let copyPanel = String(copyIdentifier.dropFirst("toolbar-options-".count))
        // Exercise the actual command installed by the picker, without OS menus.
        workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label BEGINSWITH %@", "toolbar-tile-" + panel + "-", "Manage Toolbars")).firstMatch)
        workspaceActivate(app.buttons["managed-toolbar-" + copyPanel])
        attachWorkspaceScreen(app, name: "toolbar-manager-selection")
        workspaceActivate(app.buttons["delete-managed-toolbar"])
        attachWorkspaceScreen(app, name: "toolbar-delete-prompt")
        workspaceActivate(app.buttons["toolbar-prompt-cancel"])
        workspaceActivate(app.buttons["delete-managed-toolbar"])
        workspaceActivate(app.buttons["toolbar-prompt-confirm"])
        workspaceActivate(app.buttons["Close"])
        XCTAssertFalse(app.descendants(matching: .any)[copyIdentifier].firstMatch.exists)
        XCTAssertTrue(options.exists)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        #if os(iOS)
        // The workspace root owns tear-off gestures. Its empty regions must
        // still pass two-finger navigation through to the underlying canvas.
        let status = app.staticTexts["camera-status"]
        XCTAssertTrue(status.waitForExistence(timeout: 5))
        let originalCamera = status.label
        app.otherElements["canvas"].pinch(withScale: 1.4, velocity: 0.6)
        expectation(for: NSPredicate(format: "label != %@", originalCamera), evaluatedWith: status)
        waitForExpectations(timeout: 5)
        #endif
        attachWorkspaceScreen(app, name: "toolbar-customization")
    }
    @MainActor func checkLayerConfiguration(in app: XCUIApplication) {
        app.launchEnvironment["CAPY_LAYER_INPUT_PROBE"] = "1"
        app.launchEnvironment["CAPY_CAPTURE_PROBE"] = "1"
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"add_layer"},{"type":"customize","action":{"type":"show_all_controls","panel":"layers"}}]"#
        app.launch()
        let configuration = app.scrollViews.containing(.button, identifier: "configuration-layer").firstMatch
        XCTAssertTrue(configuration.waitForExistence(timeout: 20))
        let rows = app.scrollViews["layer-rows"]
        XCTAssertTrue(rows.waitForExistence(timeout: 10))
        waitForLayerPreviews(in: app)
        func order() -> String { rows.value as? String ?? "" }
        func expectOrder(_ value: String) {
            expectation(for: NSPredicate { _, _ in order() == value }, evaluatedWith: rows)
            waitForExpectations(timeout: 5)
        }
        func close() { workspaceActivate(app.buttons["close-panel-configuration"]) }
        func reopen() {
            let tab = app.buttons["panel-tab-layers"]
            #if os(macOS)
            tab.rightClick()
            #else
            tab.press(forDuration: 0.7)
            #endif
            workspaceActivate(app.buttons["menu-action-Configure Layers panel…"])
            XCTAssertTrue(configuration.waitForExistence(timeout: 5))
        }
        func command(_ id: String) { workspaceActivate(configuration.buttons["configuration-command-" + id]) }
        let original = order()
        XCTAssertEqual(original.split(separator: ",").count, 3)
        command("add_layer")
        expectation(for: NSPredicate { _, _ in order().split(separator: ",").count == 4 }, evaluatedWith: rows)
        waitForExpectations(timeout: 5)
        let added = order()
        XCTAssertFalse(configuration.buttons["configuration-command-raise_layer"].isEnabled)
        command("lower_layer")
        expectation(for: NSPredicate { _, _ in order() != added }, evaluatedWith: rows)
        waitForExpectations(timeout: 5)
        let lowered = order()
        close()
        editorHistory("Undo", in: app); expectOrder(added)
        editorHistory("Redo", in: app); expectOrder(lowered)
        reopen()
        command("raise_layer"); expectOrder(added)
        command("delete_layer"); expectOrder(original)
        workspaceActivate(configuration.buttons["configuration-layer"])
        workspaceActivate(app.buttons["configuration-layer-option-1"])
        let value = configuration.buttons["number-value-layer-opacity"]
        XCTAssertEqual(value.value as? String, "100.0 %")
        workspaceActivate(configuration.buttons["number-decrease-layer-opacity"])
        expectation(for: NSPredicate(format: "value != %@", "100.0 %"), evaluatedWith: value)
        waitForExpectations(timeout: 5)
        let decreased = value.value as? String ?? ""
        XCTAssertEqual(decreased, "99.0 %")
        close()
        let liveValue = app.buttons["number-value-layer-opacity"]
        editorHistory("Undo", in: app)
        expectation(for: NSPredicate(format: "value == %@", "100"), evaluatedWith: liveValue)
        waitForExpectations(timeout: 5)
        editorHistory("Redo", in: app)
        expectation(for: NSPredicate(format: "value == %@", "99"), evaluatedWith: liveValue)
        waitForExpectations(timeout: 5)
        reopen()
        XCTAssertEqual(value.value as? String, decreased)
        waitForLayerPreviews(in: app)
        attachWorkspaceScreen(app, name: "layer-configuration")
        let capture = XCTAttachment(screenshot: configuration.screenshot())
        capture.name = "layer-configuration-controls"; capture.lifetime = .keepAlways; add(capture)
        func expectEnabled(_ enabled: Bool, _ message: String) {
            expectation(for: NSPredicate(format: "enabled == %@", NSNumber(value: enabled)), evaluatedWith: value)
            waitForExpectations(timeout: 5)
            XCTAssertEqual(value.isEnabled, enabled, message)
        }
        workspaceActivate(app.buttons["layer-Lock editing"])
        expectEnabled(false, "Locked opacity must retain the shared disabled state")
        workspaceActivate(app.buttons["layer-Lock editing"])
        expectEnabled(true, "Unlocking restores opacity")
        workspaceActivate(configuration.buttons["configuration-layer"])
        workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "configuration-layer-option-", "Paper")).firstMatch)
        expectEnabled(false, "Paper has no layer opacity")
        close()
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: configuration)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func checkPanelConfigurationAndLiveDrag(in app: XCUIApplication) {
        let toggle = app.buttons["configure-visible-brush_size"]
        XCTAssertTrue(toggle.waitForExistence(timeout: 20))
        workspaceActivate(app.buttons["configuration-size-8"])
        let values = app.buttons.matching(identifier: "number-value-Brush size")
        XCTAssertEqual(values.count, 2, "The preset must update both the live panel and configuration")
        for value in values.allElementsBoundByIndex {
            expectation(for: NSPredicate(format: "value == %@", "8.0 px"), evaluatedWith: value)
        }
        waitForExpectations(timeout: 5)
        let configuration = app.scrollViews.containing(.button, identifier: "configure-visible-brush_size").firstMatch
        workspaceActivate(configuration.buttons["brush-color"])
        let popup = app.descendants(matching: .any)["toolbar-control-popup"].firstMatch
        XCTAssertTrue(popup.waitForExistence(timeout: 5))
        workspaceActivate(popup.buttons["color-background"])
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: popup.buttons["color-background"])
        waitForExpectations(timeout: 5)
        workspaceActivate(popup.buttons["Done"])
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: popup)
        waitForExpectations(timeout: 5)
        let colorToggle = app.buttons["configure-visible-brush_color"]
        if colorToggle.value as? String == "Off" { workspaceActivate(colorToggle) }
        expectation(for: NSPredicate(format: "value == %@", "On"), evaluatedWith: colorToggle)
        waitForExpectations(timeout: 5)
        workspaceActivate(toggle)
        expectation(for: NSPredicate(format: "value == %@", "Off"), evaluatedWith: toggle)
        waitForExpectations(timeout: 5)
        workspaceActivate(app.buttons["close-panel-configuration"])
        let size = app.buttons["number-value-Brush size"]
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: size)
        waitForExpectations(timeout: 5)
        let liveColor = app.buttons["brush-color"]
        let liveControls = app.scrollViews.containing(.button, identifier: "brush-color").firstMatch
        revealEditorControl(liveColor, in: liveControls)
        workspaceActivate(liveColor)
        XCTAssertTrue(popup.waitForExistence(timeout: 5), "The live panel swatch must open the same color picker")
        XCTAssertTrue(popup.buttons["color-background"].isSelected)
        workspaceActivate(popup.buttons["color-foreground"])
        expectation(for: NSPredicate(format: "selected == YES"), evaluatedWith: popup.buttons["color-foreground"])
        waitForExpectations(timeout: 5)
        workspaceActivate(popup.buttons["Done"])
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: popup)
        waitForExpectations(timeout: 5)
        attachWorkspaceScreen(app, name: "panel-brush-color")
        let group = app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-group-"))
            .containing(.button, identifier: "panel-tab-sizes").firstMatch
        let grip = group.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "group-options-")).firstMatch
        XCTAssertTrue(grip.waitForExistence(timeout: 5))
        let original = grip.frame
        let target = workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.54, dy: 0.48))
        let start = grip.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        #if os(macOS)
        start.click(forDuration: 0.1, thenDragTo: target)
        #else
        start.press(forDuration: 0.1, thenDragTo: target)
        #endif
        XCTAssertTrue(grip.waitForExistence(timeout: 5))
        expectation(for: NSPredicate { _, _ in grip.frame.minX > original.maxX + 30 }, evaluatedWith: grip)
        waitForExpectations(timeout: 5)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachWorkspaceScreen(app, name: "panel-configuration-drag")
    }
    @MainActor private func attachWorkspaceScreen(_ app: XCUIApplication, name: String) {
        #if os(macOS)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        #else
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        #endif
        shot.name = name; shot.lifetime = .keepAlways; add(shot)
    }
}
