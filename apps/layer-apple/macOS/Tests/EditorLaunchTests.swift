import XCTest

final class EditorLaunchTests: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }

    @MainActor func testNumericToolControls() throws {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES", "-AppleInterfaceStyle", "Light"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"customize","action":{"type":"set_panel_visible","panel":"tool_settings","visible":true}}]"#
        app.launch()
        checkNumericToolControls(in: app)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "mac-tool-controls"; shot.lifetime = .keepAlways; add(shot)
    }

    @MainActor func testMetalLaunchCaptureAndMouseStroke() throws {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES", "-AppleInterfaceStyle", "Light"]
        app.launch()
        let window = app.windows.firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 20))
        let canvas = window.descendants(matching: .any)["canvas"].firstMatch
        defer {
            if window.exists {
                let final = XCTAttachment(screenshot: window.screenshot())
                final.name = "mac-editor-final"; final.lifetime = .keepAlways; add(final)
            }
        }
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        expectation(for: NSPredicate(format: "value == %@", "Metal ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let zen = window.buttons["zen-button"]
        XCTAssertTrue(zen.exists)
        XCTAssertEqual(zen.frame.width, 36, accuracy: 1)
        XCTAssertEqual(zen.frame.height, 36, accuracy: 1)
        for identifier in ["_XCUI:CloseWindow", "_XCUI:MinimizeWindow", "_XCUI:FullScreenWindow"] {
            let control = window.buttons[identifier]
            XCTAssertFalse(control.frame.intersects(zen.frame), "Native window controls must clear the editor header")
        }
        XCTAssertEqual(canvas.frame.width, window.frame.width, accuracy: 1)
        XCTAssertEqual(canvas.frame.height, window.frame.height, accuracy: 1)
        for name in ["Edit", "View", "Workspace"] {
            XCTAssertFalse(window.menuButtons[name].exists, "Top-level Mac menus belong in the OS menu bar")
        }
        let initial = XCTAttachment(screenshot: window.screenshot())
        initial.name = "mac-editor-initial"; initial.lifetime = .keepAlways; add(initial)

        let undo = window.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", "Undo")).firstMatch
        XCTAssertTrue(undo.exists)
        XCTAssertFalse(undo.isEnabled)
        let start = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.42, dy: 0.45))
        let end = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.60, dy: 0.60))
        start.press(forDuration: 0.05, thenDragTo: end)
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: undo)
        waitForExpectations(timeout: 10)
        // Real event delivery through AppKit must commit ink at mouseUp and
        // expose exactly one undoable stroke through shared UI state.
        app.typeKey("z", modifierFlags: .command)
        expectation(for: NSPredicate(format: "enabled == NO"), evaluatedWith: undo)
        waitForExpectations(timeout: 10)
        app.typeKey("z", modifierFlags: [.command, .shift])
        expectation(for: NSPredicate(format: "enabled == YES"), evaluatedWith: undo)
        waitForExpectations(timeout: 10)
        let painted = XCTAttachment(screenshot: window.screenshot())
        painted.name = "mac-editor-mouse-stroke"; painted.lifetime = .keepAlways; add(painted)
        checkLayerControls(in: app)
    }
}
