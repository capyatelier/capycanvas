import XCTest

extension XCTestCase {
    @MainActor func checkWorkspaceSwitcher(in app: XCUIApplication, theme: String = "light") {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"workspace_manager","command":{"type":"manage"}}]"#
        app.launch()
        let painter = app.descendants(matching: .any)["workspace-select-builtin:workspace:painter"].firstMatch
        let illustrator = app.descendants(matching: .any)["workspace-select-builtin:workspace:illustrator"].firstMatch
        let photographer = app.descendants(matching: .any)["workspace-select-builtin:workspace:photographer"].firstMatch
        let grip = app.buttons["workspace-grip-builtin:workspace:painter"]
        let menu = app.descendants(matching: .any)["workspace-row-menu"].firstMatch
        let pin = app.buttons["workspace-pin-builtin:workspace:painter"]
        func activateMenu(_ control: XCUIElement) {
            XCTAssertTrue(control.waitForExistence(timeout: 10))
            if control.isHittable { workspaceActivate(control); return }
            // XCTest can omit a hit point for offset SwiftUI overlays even
            // though their visible bounds are correct. Deliver a native pointer
            // inside the measured menu and verify the resulting editor state.
            let bounds = control.frame, viewport = workspaceViewport(in: app)
            guard bounds.width > 0, bounds.height > 0, viewport.frame.contains(bounds), menu.frame.contains(bounds) else {
                XCTFail("The menu action must be inside its visible menu and app viewport"); return
            }
            let point = viewport.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: bounds.midX - viewport.frame.minX, dy: bounds.midY - viewport.frame.minY))
            point.clickOrTap()
        }
        XCTAssertTrue(painter.waitForExistence(timeout: 30))
        let create = app.buttons["workspace-action-new"], remove = app.buttons["workspace-action-delete"]
        let rows = app.descendants(matching: .any)["workspace-manager-rows"].firstMatch
        XCTAssertTrue(create.waitForExistence(timeout: 10)); XCTAssertTrue(remove.exists)
        XCTAssertFalse(remove.isEnabled, "Included workspaces cannot be deleted")
        XCTAssertGreaterThanOrEqual(create.frame.minY, rows.frame.maxY - 1)
        XCTAssertGreaterThanOrEqual(remove.frame.minX, create.frame.maxX - 1)
        XCTAssertGreaterThanOrEqual(grip.frame.minX, pin.frame.maxX - 1)
        XCTAssertFalse(app.textFields["workspace-manager-search"].exists)
        XCTAssertFalse(app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-options-")).firstMatch.exists)
        workspaceActivate(illustrator)
        XCTAssertTrue(illustrator.isSelected)
        XCTAssertTrue(pin.waitForExistence(timeout: 10)); XCTAssertTrue(pin.isSelected)
        workspaceActivate(pin)
        XCTAssertTrue(illustrator.isSelected)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))

        // Native touch hold retains its menu after release. A fresh held contact
        // then continues into a drag; explicit grips require no hold.
        #if os(iOS)
        painter.press(forDuration: 0.8)
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertFalse(pin.isSelected)
        activateMenu(app.buttons["menu-action-Move Down"])
        workspaceActivate(grip); activateMenu(app.buttons["menu-action-Move Up"])
        workspaceActivate(pin)
        #else
        workspaceActivate(pin)
        #endif
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        func drag(_ source: XCUIElement, to target: XCUICoordinate, held: Bool) {
            let start = source.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            #if os(macOS)
            start.click(forDuration: 0.05, thenDragTo: target)
            #else
            start.press(forDuration: held ? 0.8 : 0.05, thenDragTo: target)
            #endif
        }
        #if os(iOS)
        drag(painter, to: photographer.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.95)), held: false)
        XCTAssertLessThan(painter.frame.minY, illustrator.frame.minY, "Early row motion must not reorder before the native hold")
        XCTAssertTrue(illustrator.isSelected)
        #endif
        drag(painter, to: photographer.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.95)), held: true)
        expectation(for: NSPredicate { _, _ in painter.frame.minY > photographer.frame.minY }, evaluatedWith: painter)
        waitForExpectations(timeout: 10)
        XCTAssertFalse(menu.exists, "Dragging with the held contact must close its menu")
        XCTAssertTrue(illustrator.isSelected, "A completed reorder must not select or preview the dragged row")
        drag(grip, to: illustrator.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: 0.05)), held: false)
        expectation(for: NSPredicate { _, _ in painter.frame.minY < illustrator.frame.minY }, evaluatedWith: painter)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(illustrator.isSelected)

        workspaceActivate(grip); activateMenu(app.buttons["menu-action-Move Down"])
        expectation(for: NSPredicate { _, _ in painter.frame.minY > illustrator.frame.minY && painter.frame.minY < photographer.frame.minY }, evaluatedWith: painter)
        waitForExpectations(timeout: 10)
        workspaceActivate(pin)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        app.terminate(); app.launch()
        XCTAssertTrue(painter.waitForExistence(timeout: 30))
        XCTAssertGreaterThan(painter.frame.minY, illustrator.frame.minY)
        XCTAssertLessThan(painter.frame.minY, photographer.frame.minY)
        XCTAssertTrue(pin.waitForExistence(timeout: 10)); XCTAssertFalse(pin.isSelected)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "workspace-switcher-restarted")
    }
    @MainActor func checkWorkspaceInlineNameAdmission(in app: XCUIApplication, theme: String) {
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = """
        [{"type":"set_theme","theme":"\(theme)"},{"type":"workspace_manager","command":{"type":"manage"}}]
        """
        app.launch()
        let manager = app.descendants(matching: .any)["workspace-library-manager"].firstMatch
        let create = app.buttons["workspace-action-new"]
        XCTAssertTrue(create.waitForExistence(timeout: 30))
        workspaceActivate(create)
        let formName = app.textFields["workspace-form-name"]
        XCTAssertTrue(formName.waitForExistence(timeout: 10))
        replaceEditorText(formName, with: "Inline admission original", in: app)
        let confirm = app.buttons["workspace-form-confirm"]
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: confirm)
        waitForExpectations(timeout: 10)
        workspaceActivate(confirm)
        XCTAssertTrue(formName.waitForNonExistence(timeout: 15))
        XCTAssertTrue(manager.exists, "Creating a workspace keeps the manager open")
        let created = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
            "workspace-select-", "Inline admission original")).firstMatch
        XCTAssertTrue(created.waitForExistence(timeout: 10)); XCTAssertTrue(created.isSelected)
        workspaceActivate(app.buttons["workspace-manager-close"])
        XCTAssertTrue(manager.waitForNonExistence(timeout: 15))
        let activeBuiltin = app.buttons["workspace-switch-builtin:workspace:painter"]
        let compactSwitcher = app.descendants(matching: .any)["workspace-switcher"].firstMatch.buttons.firstMatch
        if activeBuiltin.exists { workspaceActivate(activeBuiltin) }
        else {
            XCTAssertTrue(compactSwitcher.waitForExistence(timeout: 10))
            workspaceActivate(compactSwitcher)
            let switcherMenu = app.descendants(matching: .any)["workspace-switcher-menu"].firstMatch
            XCTAssertTrue(switcherMenu.waitForExistence(timeout: 10))
            workspaceActivate(switcherMenu.buttons["menu-action-Sketch"])
            XCTAssertTrue(switcherMenu.waitForNonExistence(timeout: 10))
        }
        expectation(for: NSPredicate { _, _ in
            activeBuiltin.exists && activeBuiltin.isSelected || compactSwitcher.exists && compactSwitcher.value as? String == "Sketch"
        }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        app.terminate(); app.launch()
        let title = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
            "workspace-select-", "Inline admission original")).firstMatch
        guard title.waitForExistence(timeout: 30) else { XCTFail("The created workspace must survive restart"); return }
        let id = String(title.identifier.dropFirst("workspace-select-".count))
        XCTAssertNotNil(UUID(uuidString: id))
        let row = app.descendants(matching: .any)["workspace-select-" + id].firstMatch
        let originalRowLabel = row.label
        XCTAssertEqual(originalRowLabel.components(separatedBy: "Inline admission original").count, 2)
        let pin = app.buttons["workspace-pin-" + id]
        let rows = app.descendants(matching: .any)["workspace-manager-rows"].firstMatch
        let name = app.textFields["workspace-name-" + id]
        let error = app.descendants(matching: .any)["workspace-manager-error"].firstMatch
        func order() -> [String] {
            rows.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH %@", "workspace-select-"))
                .allElementsBoundByIndex.map(\.identifier)
        }
        workspaceActivate(row)
        let selected = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            row.isSelected && app.buttons["workspace-manager-apply"].isEnabled && !manager.progressIndicators.firstMatch.exists
        }, object: row)
        guard XCTWaiter.wait(for: [selected], timeout: 10) == .completed else {
            let hierarchy = XCTAttachment(string: app.debugDescription)
            hierarchy.name = "workspace-inline-selection-" + theme
            hierarchy.lifetime = .keepAlways
            add(hierarchy)
            attachEditor(in: app, name: "workspace-inline-selection-" + theme)
            XCTFail("The original row click must publish the selected ready preview within ten seconds")
            return
        }
        XCTAssertTrue(row.isSelected)
        let originalOrder = order(), originalPin = pin.isSelected
        XCTAssertTrue(rows.exists); XCTAssertTrue(originalOrder.contains("workspace-select-" + id))
        var latest = "Inline admission original"
        for (label, identifier) in [("New", "workspace-action-new"), ("Delete", "workspace-action-delete"), ("Apply", "workspace-manager-apply")] {
            let currentTitle = app.buttons["workspace-select-" + id].firstMatch
            XCTAssertTrue(currentTitle.waitForExistence(timeout: 10))
            #if os(macOS)
            currentTitle.doubleClick()
            #else
            currentTitle.doubleTap()
            #endif
            guard name.waitForExistence(timeout: 10) else { XCTFail("Rename must mount in the original row"); return }
            XCTAssertEqual(name.value as? String, latest, "The real row must reopen its exact latest accepted name")
            replaceEditorText(name, with: "", in: app)
            XCTAssertEqual(name.value as? String, "")
            let followUp = app.buttons[identifier]
            XCTAssertTrue(followUp.exists); XCTAssertTrue(followUp.isEnabled)
            workspaceActivate(followUp)
            XCTAssertTrue(error.waitForExistence(timeout: 10), "The async empty-name refusal must be visible before checking the retained rename")
            XCTAssertTrue(manager.exists); XCTAssertTrue(name.exists)
            XCTAssertEqual(name.value as? String, "", "\(label) must retain the rejected inline draft")
            XCTAssertFalse(formName.exists); XCTAssertFalse(confirm.exists, "\(label) must not replace Rename with another prompt")
            XCTAssertTrue(row.isSelected, "\(label) must preserve the selected preview")
            XCTAssertEqual(order(), originalOrder); XCTAssertEqual(pin.isSelected, originalPin)
            latest = "Inline admission after " + label
            replaceEditorText(name, with: latest, in: app)
            name.typeText("\n")
            XCTAssertTrue(name.waitForNonExistence(timeout: 10))
            expectation(for: NSPredicate(format: "label == %@",
                originalRowLabel.replacingOccurrences(of: "Inline admission original", with: latest)), evaluatedWith: currentTitle)
            waitForExpectations(timeout: 10)
            XCTAssertTrue(error.waitForNonExistence(timeout: 10))
            XCTAssertTrue(row.isSelected); XCTAssertEqual(order(), originalOrder); XCTAssertEqual(pin.isSelected, originalPin)
        }
        let apply = app.buttons["workspace-manager-apply"]
        XCTAssertTrue(apply.isEnabled)
        workspaceActivate(apply)
        XCTAssertTrue(manager.waitForNonExistence(timeout: 15))
        app.terminate(); app.launch()
        XCTAssertTrue(row.waitForExistence(timeout: 30)); XCTAssertTrue(row.isSelected)
        XCTAssertEqual(row.label, originalRowLabel.replacingOccurrences(of: "Inline admission original", with: latest))
        XCTAssertEqual(order(), originalOrder); XCTAssertEqual(pin.isSelected, originalPin)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "workspace-inline-name-admission-" + theme)
    }
}
