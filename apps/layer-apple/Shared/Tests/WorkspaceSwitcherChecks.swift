import XCTest

extension XCTestCase {
    @MainActor func checkWorkspaceSwitcher(in app: XCUIApplication) {
        app.launchEnvironment.removeValue(forKey: "CAPY_DISABLE_PERSISTENCE")
        app.launchEnvironment["CAPY_PERSISTENCE_NAMESPACE"] = UUID().uuidString
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"workspace_manager","command":{"type":"manage"}}]"#
        app.launch()
        let painter = app.buttons["workspace-select-builtin:workspace:painter"]
        let illustrator = app.buttons["workspace-select-builtin:workspace:illustrator"]
        let photographer = app.buttons["workspace-select-builtin:workspace:photographer"]
        let grip = app.buttons["workspace-grip-builtin:workspace:painter"]
        let options = app.buttons["workspace-options-builtin:workspace:painter"]
        let menu = app.descendants(matching: .any)["workspace-row-menu"].firstMatch
        let pin = app.buttons["menu-action-Show in top bar"]
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
            #if os(macOS)
            point.click()
            #else
            point.tap()
            #endif
        }
        XCTAssertTrue(painter.waitForExistence(timeout: 30))
        workspaceActivate(illustrator)
        XCTAssertTrue(illustrator.isSelected)
        workspaceActivate(options)
        XCTAssertTrue(pin.waitForExistence(timeout: 10)); XCTAssertTrue(pin.isSelected)
        activateMenu(pin)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))

        // Native touch hold retains its menu after release. A fresh held contact
        // then continues into a drag; explicit grips require no hold.
        #if os(iOS)
        painter.press(forDuration: 0.8)
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        XCTAssertFalse(pin.isSelected)
        activateMenu(pin) // Restore visibility, closing the menu.
        #else
        workspaceActivate(options); activateMenu(pin)
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

        workspaceActivate(options); activateMenu(app.buttons["menu-action-Move Down"])
        expectation(for: NSPredicate { _, _ in painter.frame.minY > illustrator.frame.minY && painter.frame.minY < photographer.frame.minY }, evaluatedWith: painter)
        waitForExpectations(timeout: 10)
        workspaceActivate(options); activateMenu(pin)
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10))
        app.terminate(); app.launch()
        XCTAssertTrue(painter.waitForExistence(timeout: 30))
        XCTAssertGreaterThan(painter.frame.minY, illustrator.frame.minY)
        XCTAssertLessThan(painter.frame.minY, photographer.frame.minY)
        workspaceActivate(options)
        XCTAssertTrue(pin.waitForExistence(timeout: 10)); XCTAssertFalse(pin.isSelected)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        attachEditor(in: app, name: "workspace-switcher-restarted")
    }
}
