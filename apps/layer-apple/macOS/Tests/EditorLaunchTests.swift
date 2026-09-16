import XCTest

final class EditorLaunchTests: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }

    @MainActor func testNativeDocumentColor() throws {
        try checkNativeDocumentColor(in: editorCaptureApplication())
    }

    @MainActor func testNewEditorAfterLastWindowClose() {
        checkNewEditorAfterLastWindowClose(in: editorCaptureApplication())
    }

    @MainActor func testNativeProjectRoundTrip() throws {
        try checkNativeProjectRoundTrip(in: editorCaptureApplication())
    }

    @MainActor func testFailedProjectOpenPreservesArtwork() throws {
        try checkFailedProjectOpenPreservesArtwork(in: editorCaptureApplication())
    }

    @MainActor func testNativeImageImport() throws {
        try checkNativeImageImport(in: editorCaptureApplication())
    }

    @MainActor func testEditorKeyboardFocus() { checkEditorKeyboardFocus(in: editorCaptureApplication()) }
    @MainActor func testNumericTextHistory() { checkNumericTextHistory(in: editorCaptureApplication()) }
    @MainActor func testNumericSettingsDone() { checkNumericSettingsDone(in: editorCaptureApplication()) }
    @MainActor func testSettingsNumericReset() { checkSettingsNumericReset(in: editorCaptureApplication()) }
    @MainActor func testSettingsTextState() { checkSettingsTextState(in: editorCaptureApplication()) }
    @MainActor func testSettingsChoicePresentation() { checkSettingsChoicePresentation(in: editorCaptureApplication()) }
    @MainActor func testSettingsDropdownsAndPrediction() { checkSettingsControls(in: editorCaptureApplication()) }

    @MainActor func testBlendAndLiquify() { checkBlendAndLiquify(in: editorCaptureApplication()) }

    @MainActor func testPaintingBrushes() { checkPaintingBrushes(in: editorCaptureApplication()) }

    @MainActor func testFullscreenEditor() { checkFullscreenEditor(in: editorCaptureApplication()) }

    @MainActor func testMaskTransforms() { checkMaskTransforms(in: editorCaptureApplication()) }

    @MainActor func testMaskActionsAndHistory() { checkMaskActionsAndHistory(in: editorCaptureApplication()) }

    @MainActor func testLayerContentActionsAndHistory() { checkLayerContentActionsAndHistory(in: editorCaptureApplication()) }

    @MainActor func testGroupArtworkWorkflow() { checkGroupArtworkWorkflow(in: editorCaptureApplication()) }

    @MainActor func testMoveAndTransformCancellation() {
        checkMoveAndTransformCancellation(in: editorCaptureApplication())
    }

    @MainActor func testTransformFieldRetainsScroll() {
        checkTransformFieldRetainsScroll(in: editorCaptureApplication())
    }

    @MainActor func testTransformRotationAndHandles() {
        checkTransformRotationAndHandles(in: editorCaptureApplication())
    }

    @MainActor func testLassoControls() { checkLassoControls(in: editorCaptureApplication()) }

    @MainActor func testHandAndEyedropper() { checkHandAndEyedropper(in: editorCaptureApplication()) }

    @MainActor func testRegionSelectionAndFill() { checkRegionSelectionAndFill(in: editorCaptureApplication()) }

    @MainActor func testNativeRegionRefinement() throws { try checkNativeRegionRefinement(in: editorCaptureApplication()) }

    @MainActor func testSelectionInversion() { checkSelectionInversion(in: editorCaptureApplication()) }

    @MainActor func testRulerWorkflow() {
        checkRulerWorkflow(in: editorCaptureApplication())
    }

    @MainActor func testFiguresAndGradients() {
        checkFiguresAndGradients(in: editorCaptureApplication())
    }

    @MainActor func testAboutAndApplicationMenus() { checkAboutAndApplicationMenus(in: editorCaptureApplication()) }

    @MainActor func testApplicationLinkHandoff() { checkApplicationLinkHandoff(in: editorCaptureApplication()) }

    @MainActor func testSelectionAndTransform() { checkSelectionAndTransform(in: editorCaptureApplication()) }

    @MainActor func testPopupThemeFollowsExplicitAndSystem() { checkPopupThemeFollowsExplicitAndSystem() }

    @MainActor func testWorkspaceSwitcher() {
        checkWorkspaceSwitcher(in: editorTestApplication())
    }

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

    @MainActor func testTitleBarCustomization() { checkTitleBarCustomization(in: editorTestApplication()) }

    @MainActor func testTitleBarToolDrawers() { checkTitleBarToolDrawers(in: editorCaptureApplication()) }

    @MainActor func testColumnStacks() { checkColumnStacks(in: editorTestApplication()) }

    @MainActor func testColumnStacksDark() { checkColumnStacks(in: editorTestApplication(), theme: "dark") }

    @MainActor func testPaintDefaultColumns() { checkDefaultWorkspaceColumns(in: editorTestApplication()) }

    @MainActor func testPhotoDefaultColumns() { checkDefaultWorkspaceColumns(in: editorTestApplication(), photo: true) }

    @MainActor func testRendererRecovery() { checkRendererRecovery(in: editorTestApplication()) }

    @MainActor func testTitleBarSystemStatus() {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        checkTitleBarSystemStatus(in: app)
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



    @MainActor func testZenHidesChromeAndTabRestoresIt() {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"zen_mode"}]"#
        app.launch()
        let canvas = app.windows.firstMatch.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        XCTAssertTrue(app.buttons["panel-tab-sizes"].waitForNonExistence(timeout: 5))
        XCTAssertFalse(app.staticTexts["document-title"].exists)
        XCTAssertFalse(app.buttons["zen-button"].exists)
        app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [])
        XCTAssertTrue(app.buttons["panel-tab-sizes"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts["document-title"].exists)
        XCTAssertTrue(app.buttons["zen-button"].exists)
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

    @MainActor func testLayerConfiguration() throws {
        checkLayerConfiguration(in: editorTestApplication())
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

    @MainActor func testFilterArtworkAndHistory() throws {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        checkFilterArtworkAndHistory(in: app)
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

    @MainActor func testNativeSDRCreationAndPalettes() throws {
        let app = editorTestApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"customize","action":{"type":"set_panel_visible","panel":"color","visible":true}},{"type":"color","action":{"op":"definition","color":{"space":"ProPhoto","rgba":[0.12345678,0.23456789,0.34567891,0.654321]}}},{"type":"invoke","command":"new_document"}]"#
        app.launch()
        checkNativeSDRCreationAndPalettes(in: app)
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
            attachColorFixture(name: "mac-color-" + mode, state: state,
                screenshot: window.screenshot(), viewport: window.frame, wheel: wheel)
        }
    }

    @MainActor func testCompleteEditorCapture() {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        capturePaintEditor(in: app)
    }

    @MainActor func testCompleteEditorDarkCapture() {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        app.launch()
        capturePaintEditor(in: app, theme: "dark")
        for _ in 0..<4 { workspaceActivate(app.buttons["navigator-zoom_in"]) }
        capturePaintEditor(in: app, scenario: "paint-canvas-under-header", theme: "dark")
    }

    @MainActor func testBlendChoices() {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkBlendChoices(in: app)
    }

    @MainActor func testInlineLayerOpacity() {
        let app = editorCaptureApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkInlineLayerOpacity(in: app)
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
