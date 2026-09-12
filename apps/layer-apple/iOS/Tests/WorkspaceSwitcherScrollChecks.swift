import XCTest

extension EditorLaunchTests {
    /// The simulator helper copies a coordinator-created database into a fresh
    /// test namespace. No repeated creation dialogs or production seeding hook.
    @MainActor func testWorkspaceSwitcherScrolling() throws {
        guard let value = ProcessInfo.processInfo.environment["CAPY_SWITCHER_SEED_NAMESPACE"],
              let namespace = UUID(uuidString: value) else {
            throw XCTSkip("Run scripts/test-workspace-scrolling.py with an iPad simulator")
        }
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launchEnvironment.removeValue(forKey: "CAPY_DISABLE_PERSISTENCE")
        app.launchEnvironment["CAPY_PERSISTENCE_NAMESPACE"] = namespace.uuidString
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"workspace_manager","command":{"type":"manage"}}]"#
        app.launch()
        let list = app.scrollViews["workspace-manager-rows"]
        let painter = app.buttons["workspace-select-builtin:workspace:painter"]
        let illustrator = app.buttons["workspace-select-builtin:workspace:illustrator"]
        let photographer = app.buttons["workspace-select-builtin:workspace:photographer"]
        let menu = app.descendants(matching: .any)["workspace-row-menu"].firstMatch
        XCTAssertTrue(list.waitForExistence(timeout: 30))
        XCTAssertTrue(painter.waitForExistence(timeout: 10))
        XCTAssertTrue(illustrator.isSelected)
        let originalY = painter.frame.minY
        photographer.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.05,
            thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.01)))
        XCTAssertTrue(!painter.isHittable || painter.frame.minY < originalY - 20,
            "Early row motion must scroll the native list")
        func returnToTop(_ first: XCUIElement) {
            for _ in 0..<6 {
                if first.exists && first.isHittable && first.frame.minY >= list.frame.minY { return }
                list.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.2)).press(forDuration: 0.05,
                    thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.9)))
            }
            XCTFail("Could not return to the first row with ordinary touch scrolling")
        }
        returnToTop(painter)
        XCTAssertLessThan(painter.frame.minY, illustrator.frame.minY)
        XCTAssertLessThan(illustrator.frame.minY, photographer.frame.minY)
        XCTAssertTrue(illustrator.isSelected, "Scrolling must preserve selection and order")

        painter.press(forDuration: 0.8)
        XCTAssertTrue(menu.waitForExistence(timeout: 10))
        // Start outside the menu, in a row body, with a new unheld contact.
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.85)).press(forDuration: 0.05,
            thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.2, dy: 0.15)))
        XCTAssertTrue(menu.waitForNonExistence(timeout: 10), "Ordinary scrolling must dismiss the row menu")
        returnToTop(painter)

        let grip = app.buttons["workspace-grip-builtin:workspace:painter"]
        let origin = grip.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let bottom = list.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
            dx: grip.frame.midX - list.frame.minX, dy: list.frame.height - 10))
        origin.press(forDuration: 0.05, thenDragTo: bottom, withVelocity: .slow, thenHoldForDuration: 0.8)
        XCTAssertFalse(menu.exists)
        XCTAssertFalse(illustrator.isHittable, "Holding the grip at the edge must scroll the original rows out of view")
        returnToTop(illustrator)
        XCTAssertTrue(illustrator.isSelected)
        XCTAssertLessThan(illustrator.frame.minY, photographer.frame.minY)
        let firstCustom = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
            "workspace-select-", "Drawing Task 00")).firstMatch
        XCTAssertTrue(firstCustom.exists)
        XCTAssertTrue(!painter.isHittable || painter.frame.minY > firstCustom.frame.minY,
            "The grip drop must persist below the original visible defaults after edge scrolling")
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "workspace-switcher-after-scrolling"; attachment.lifetime = .keepAlways; add(attachment)
    }
}
