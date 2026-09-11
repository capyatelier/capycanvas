import XCTest

extension XCTestCase {
    @MainActor func workspaceViewport(in app: XCUIApplication) -> XCUIElement {
        #if os(macOS)
        return app.windows.firstMatch
        #else
        return app
        #endif
    }
    @MainActor private func workspaceActivate(_ element: XCUIElement) {
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
        #if os(macOS)
        field.typeKey("a", modifierFlags: .command)
        field.typeText(value)
        #else
        let length = (field.value as? String)?.count ?? 0
        field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: length) + value)
        #endif
    }
    @MainActor func checkCollapsedColumnsDrawersAndZen(in app: XCUIApplication) {
        workspaceActivate(app.buttons["column-icon-toolbar"])
        let column = app.descendants(matching: .any)["column-drawer-4"].firstMatch
        XCTAssertTrue(column.waitForExistence(timeout: 10))
        workspaceActivate(app.buttons["drawer-tab-brushes"])
        XCTAssertTrue(column.buttons["brush-1"].waitForExistence(timeout: 5))
        workspaceActivate(app.buttons["drawer-tab-toolbar"])
        let pen = column.buttons["toolbar-tile-toolbar-1"]
        workspaceActivate(pen)
        let drawer = app.descendants(matching: .any)["tool-drawer"].firstMatch
        if !drawer.waitForExistence(timeout: 2) { workspaceActivate(pen) }
        XCTAssertTrue(drawer.waitForExistence(timeout: 10))
        XCTAssertTrue(workspaceViewport(in: app).frame.contains(drawer.frame), "The shared drawer bounds must stay within the viewport")
        XCTAssertTrue(drawer.buttons["brush-1"].exists)
        let outside = workspaceViewport(in: app).coordinate(withNormalizedOffset: CGVector(dx: 0.75, dy: 0.7))
        #if os(macOS)
        outside.click()
        #else
        outside.tap()
        #endif
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: drawer)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(column.exists, "Column drawers use explicit dismissal")
        workspaceActivate(app.buttons["expand-column-4"])
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: column)
        waitForExpectations(timeout: 5)
        XCTAssertTrue(app.buttons["panel-tab-toolbar"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachWorkspaceScreen(app, name: "columns-drawers-zen")
    }
    @MainActor func checkToolbarCustomization(in app: XCUIApplication) {
        XCTAssertTrue(app.textFields["toolbar-name"].waitForExistence(timeout: 20))
        for query in ["Undo", "Divider", "Manage Toolbars"] {
            workspaceText(app.textFields["tool-picker-search"], query)
            workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "tool-choice-" + query)).firstMatch)
        }
        workspaceActivate(app.buttons["tool-picker-confirm"])
        let created = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@ AND value == %@", "toolbar-options-", "Toolbar 1")).firstMatch
        XCTAssertTrue(created.waitForExistence(timeout: 10))
        let identifier = created.identifier
        let panel = String(identifier.dropFirst("toolbar-options-".count))
        let options = app.descendants(matching: .any)[identifier].firstMatch
        func menu(_ prefix: String) {
            workspaceActivate(options)
            workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-action-" + prefix)).firstMatch)
        }
        menu("Rename ")
        workspaceText(app.textFields["toolbar-name"], "Quick tools")
        workspaceActivate(app.buttons["toolbar-prompt-confirm"])
        menu("Duplicate ")
        workspaceText(app.textFields["toolbar-name"], "Copy tools")
        workspaceActivate(app.buttons["toolbar-prompt-confirm"])
        let copied = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@ AND value == %@", "toolbar-options-", "Copy tools")).firstMatch
        XCTAssertTrue(copied.waitForExistence(timeout: 10))
        let copyIdentifier = copied.identifier
        let copyPanel = String(copyIdentifier.dropFirst("toolbar-options-".count))
        // Exercise the actual command installed by the picker, without OS menus.
        workspaceActivate(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label BEGINSWITH %@", "toolbar-tile-" + panel + "-", "Manage Toolbars")).firstMatch)
        workspaceActivate(app.buttons["managed-toolbar-" + copyPanel])
        workspaceActivate(app.buttons["delete-managed-toolbar"])
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
        workspaceActivate(app.buttons["configuration-brush-color"])
        let popup = app.descendants(matching: .any)["toolbar-control-popup"].firstMatch
        XCTAssertTrue(popup.waitForExistence(timeout: 5))
        workspaceActivate(app.buttons["color-background"])
        XCTAssertTrue(app.buttons["color-background"].isSelected)
        workspaceActivate(app.buttons["Done"])
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: popup)
        waitForExpectations(timeout: 5)
        workspaceActivate(toggle)
        expectation(for: NSPredicate(format: "value == %@", "Off"), evaluatedWith: toggle)
        waitForExpectations(timeout: 5)
        workspaceActivate(app.buttons["close-panel-configuration"])
        let size = app.buttons["number-value-Brush size"]
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: size)
        waitForExpectations(timeout: 5)
        let grip = app.descendants(matching: .any)["group-options-6"].firstMatch
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
        XCTAssertGreaterThan(grip.frame.minX, original.maxX + 30, "Tearing off a panel must retain its gesture across the new floating group, including a hidden tab")
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
