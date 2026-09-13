import XCTest

extension EditorLaunchTests {
    @MainActor func testNativeZenContextAction() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launch()
        let zen = app.buttons["zen-button"]
        XCTAssertTrue(zen.waitForExistence(timeout: 20))
        zen.press(forDuration: 0.8)
        let icon = app.buttons["Change icon…"]
        XCTAssertTrue(icon.waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["layer-New layer"].exists, "Opening the native menu must suppress the Zen button tap")
        icon.tap()
        XCTAssertTrue(app.descendants(matching: .any)["preference-zen_icon"].firstMatch.waitForExistence(timeout: 10))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
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
        workspaceActivate(illustrator)
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
