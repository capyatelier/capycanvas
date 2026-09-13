import XCTest

extension EditorLaunchTests {
    @MainActor func testNativeZenContextAction() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launch()
        let zen = app.buttons["zen-button"]
        XCTAssertTrue(zen.waitForExistence(timeout: 20))
        XCTAssertEqual(zen.frame.width, 36, accuracy: 1)
        XCTAssertEqual(zen.frame.height, 36, accuracy: 1)
        let toolbar = app.descendants(matching: .any)["zen-toolbar-0"].firstMatch
        zen.tap()
        XCTAssertTrue(toolbar.waitForExistence(timeout: 5), "An ordinary tap still enters partial zen")
        zen.tap()
        XCTAssertTrue(app.buttons["layer-New layer"].waitForExistence(timeout: 5))
        zen.press(forDuration: 0.8)
        let total = app.buttons["Total zen"]
        XCTAssertTrue(total.waitForExistence(timeout: 5))
        XCTAssertFalse(toolbar.exists, "Opening the native menu must suppress the button tap")
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        capture.name = "native-zen-context-menu"; capture.lifetime = .keepAlways; add(capture)
        total.tap()
        XCTAssertTrue(total.waitForNonExistence(timeout: 5))
        zen.tap()
        XCTAssertTrue(app.buttons["layer-New layer"].waitForNonExistence(timeout: 5))
        XCTAssertFalse(toolbar.exists, "The native menu action must change shared Total zen behavior")
        XCTAssertTrue(zen.waitForNonExistence(timeout: 5), "Total zen also hides its own button")
        XCTAssertTrue(app.otherElements["canvas"].exists)
    }
}

extension XCTestCase {
    @MainActor func checkNativeWorkspaceContextAction() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"},{"type":"workspace_manager","command":{"type":"manage"}}]"#
        app.launch()
        let painter = app.buttons["workspace-select-builtin:workspace:painter"]
        let illustrator = app.buttons["workspace-select-builtin:workspace:illustrator"]
        let photographer = app.buttons["workspace-select-builtin:workspace:photographer"]
        XCTAssertTrue(painter.waitForExistence(timeout: 30))
        XCTAssertTrue(illustrator.isSelected)
        painter.press(forDuration: 0.8)
        let pin = app.buttons["Show in top bar"]
        let down = app.buttons["Move Down"]
        XCTAssertTrue(pin.waitForExistence(timeout: 5))
        XCTAssertTrue(down.waitForExistence(timeout: 5))
        XCTAssertLessThan(pin.frame.maxY, down.frame.minY, "Row menus must retain a vertical layout")
        XCTAssertTrue(illustrator.isSelected, "A hold must suppress the row's ordinary selection")
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        capture.name = "native-workspace-context-dark"; capture.lifetime = .keepAlways; add(capture)
        down.tap()
        XCTAssertTrue(down.waitForNonExistence(timeout: 5))
        expectation(for: NSPredicate { _, _ in painter.frame.minY > illustrator.frame.minY && painter.frame.minY < photographer.frame.minY }, evaluatedWith: painter)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(illustrator.isSelected)
        app.terminate(); app.launch()
        XCTAssertTrue(painter.waitForExistence(timeout: 30))
        XCTAssertGreaterThan(painter.frame.minY, illustrator.frame.minY, "The native action must persist the shared order")
        XCTAssertLessThan(painter.frame.minY, photographer.frame.minY)
    }
}
