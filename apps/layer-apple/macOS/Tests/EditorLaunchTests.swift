import XCTest

final class EditorLaunchTests: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }

    @MainActor func testWorkspaceLibraryHistory() {
        checkWorkspaceLibraryHistory(in: editorTestApplication())
    }

    @MainActor func testDrawerDragAndDock() {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        checkDrawerDragAndDock(in: app)
    }

    @MainActor func testToolbarStylesAndActions() {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        checkToolbarStylesAndActions(in: app)
    }

    @MainActor func testSystemStatusSetting() {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        checkSystemStatusSetting(in: app)
    }

    @MainActor func testIndependentEditorWindows() {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        checkIndependentEditorWindows(in: app)
    }

    @MainActor func testArtworkRecoveryAfterRestart() {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        checkArtworkRecoveryAfterRestart(in: app)
    }



    @MainActor func testPartialZenToolbar() throws {
        let app = editorTestApplication()
        #if os(iOS)
        XCUIDevice.shared.orientation = .landscapeLeft
        #else
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"zen_mode"}]"#
        app.launch()
        let section = app.descendants(matching: .any)["zen-toolbar-0"].firstMatch
        XCTAssertTrue(section.waitForExistence(timeout: 20))
        XCTAssertTrue(app.windows.firstMatch.frame.contains(section.frame))
        #if os(macOS)
        app.buttons["zen-button"].click()
        #else
        app.buttons["zen-button"].tap()
        #endif
        XCTAssertTrue(app.buttons["panel-tab-sizes"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func testCollapsedColumnsDrawersAndZen() throws {
        let app = editorTestApplication()
        #if os(iOS)
        XCUIDevice.shared.orientation = .landscapeLeft
        #else
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"move_panel","panel":"toolbar","target":{"kind":"tab","group":6},"viewport":[1376,1032]},{"type":"customize","action":{"type":"set_column_collapsed","group":6,"collapsed":true}}]"#
        app.launch()
        checkCollapsedColumnsDrawersAndZen(in: app)
    }

    @MainActor func testToolbarCustomization() throws {
        let app = editorTestApplication()
        #if os(iOS)
        XCUIDevice.shared.orientation = .landscapeLeft
        #else
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"new_toolbar"}]"#
        app.launch()
        checkToolbarCustomization(in: app)
    }

    @MainActor func testPanelConfigurationAndLiveDrag() throws {
        let app = editorTestApplication()
        #if os(iOS)
        XCUIDevice.shared.orientation = .landscapeLeft
        #else
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"customize","action":{"type":"show_all_controls","panel":"sizes"}}]"#
        app.launch()
        checkPanelConfigurationAndLiveDrag(in: app)
    }

    @MainActor func testNavigatorAndDiagnostics() throws {
        let app = editorTestApplication()
        #if os(iOS)
        XCUIDevice.shared.orientation = .landscapeLeft
        #else
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkNavigatorAndDiagnostics(in: app)
    }

    @MainActor func testFilterSearchPreviewAndProperties() throws {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"customize","action":{"type":"set_panel_visible","panel":"adjustments","visible":true}}]"#
        app.launch()
        checkFilterSearchPreviewAndProperties(in: app)
    }

    @MainActor func testShortcutConflictAndEditorEffect() throws {
        let app = editorTestApplication()
        #if os(iOS)
        XCUIDevice.shared.orientation = .landscapeLeft
        #else
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"invoke","command":"keyboard_shortcuts"}]"#
        app.launch()
        checkShortcutConflictAndEditorEffect(in: app)
    }

    @MainActor func testNewDrawingAndExportCancellation() throws {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"invoke","command":"new_document"}]"#
        app.launch()
        checkNewDrawingAndExportCancellation(in: app)
    }

    @MainActor func testSettingsAndWorkspaceRestart() throws {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        checkSettingsAndWorkspaceRestart(in: app)
    }

    @MainActor func testColorControls() throws {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"preferences","action":{"type":"edit","id":"theme","value":1}},{"type":"customize","action":{"type":"set_panel_visible","panel":"color","visible":true}},{"type":"set_color","rgba":[1,0,0,1]}]"#
        app.launchEnvironment["CAPY_COLOR_PROBE"] = "1"
        app.launch()
        checkColorControls(in: app) { mode, wheel, state in
            let window = app.windows.firstMatch
            attachColorFixture(name: "mac-color-" + mode, space: mode, state: state,
                screenshot: window.screenshot(), viewport: window.frame, wheel: wheel)
        }
    }

    @MainActor func testCompleteEditorCapture() {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        captureDefaultEditor(in: app)
    }

    @MainActor func testBlendChoices() {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkBlendChoices(in: app)
    }

    @MainActor func testEditorControlLayout() {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkEditorControlLayout(in: app)
    }

    @MainActor func testNumericToolControls() throws {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES", "-AppleInterfaceStyle", "Light"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkNumericToolControls(in: app)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "mac-tool-controls"; shot.lifetime = .keepAlways; add(shot)
    }

    @MainActor func testMetalLaunchCaptureAndMouseStroke() throws {
        let app = editorTestApplication()
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
