import XCTest

final class EditorLaunchTests: XCTestCase {
    override func setUp() async throws {
        continueAfterFailure = false
        #if os(iOS)
        await MainActor.run { XCUIDevice.shared.orientation = .landscapeLeft }
        #endif
    }

    @MainActor func testNativeDrawingTabs() throws {
        try checkNativeDrawingTabs(in: editorCaptureApplication())
    }

    @MainActor func testNativeHDRColor() throws {
        try checkNativeHDRColor(in: editorCaptureApplication())
    }

    @MainActor func testNativeDocumentColor() throws {
        try checkNativeDocumentColor(in: editorCaptureApplication())
    }

    @MainActor func testEditorKeyboardFocus() { checkEditorKeyboardFocus(in: editorCaptureApplication()) }
    @MainActor func testNumericTextHistory() { checkNumericTextHistory(in: editorCaptureApplication()) }
    @MainActor func testNumericSettingsDone() { checkNumericSettingsDone(in: editorCaptureApplication()) }
    @MainActor func testSettingsNumericReset() { checkSettingsNumericReset(in: editorCaptureApplication()) }

    @MainActor func testSettingsTextState() {
        #if os(iOS)
        checkSettingsTextState(in: editorCaptureApplication(), keyboardSelection: false)
        #else
        checkSettingsTextState(in: editorCaptureApplication())
        #endif
    }

    @MainActor func testSettingsChoicePresentation() { checkSettingsChoicePresentation(in: editorCaptureApplication()) }
    @MainActor func testSettingsDropdowns() { checkSettingsControls(in: editorCaptureApplication()) }

    @MainActor func testBlendAndLiquify() { checkBlendAndLiquify(in: editorCaptureApplication()) }

    @MainActor func testPaintingBrushes() { checkPaintingBrushes(in: editorCaptureApplication()) }

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

    @MainActor func testToolbarComponents() { checkToolbarComponents(in: editorCaptureApplication()) }
    @MainActor func testColorPicker() { checkColorPicker(in: editorCaptureApplication()) }
    @MainActor func testTonalSelection() { checkTonalSelection(in: editorCaptureApplication()) }
    @MainActor func testPalettes() { checkPalettes(in: editorCaptureApplication()) }
    @MainActor func testStrokeRecording() { checkStrokeRecording(in: editorCaptureApplication()) }
    @MainActor func testPanelTransparency() { checkPanelTransparency(in: editorCaptureApplication()) }
    @MainActor func testZenPreferences() { checkZenPreferences(in: editorCaptureApplication()) }

    @MainActor func testSelectionMasks() { checkSelectionMasks(in: editorCaptureApplication()) }

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

    @MainActor func testDrawerDragAndDock() { checkDrawerDragAndDock(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testToolbarStylesAndActions() { checkToolbarStylesAndActions(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testTitleBarCustomization() { checkTitleBarCustomization(in: editorTestApplication()) }

    @MainActor func testTitleBarToolDrawers() { checkTitleBarToolDrawers(in: editorCaptureApplication()) }

    @MainActor func testColumnStacks() { checkColumnStacks(in: editorTestApplication()) }

    @MainActor func testColumnStacksDark() { checkColumnStacks(in: editorTestApplication(), theme: "dark") }

    @MainActor func testPaintDefaultColumns() { checkDefaultWorkspaceColumns(in: editorTestApplication()) }

    @MainActor func testPhotoDefaultColumns() { checkDefaultWorkspaceColumns(in: editorTestApplication(), photo: true) }

    @MainActor func testRendererRecovery() { checkRendererRecovery(in: editorTestApplication()) }

    @MainActor func testTitleBarSystemStatus() { checkTitleBarSystemStatus(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testIndependentEditorWindows() { checkIndependentEditorWindows(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testArtworkRecoveryAfterRestart() { checkArtworkRecoveryAfterRestart(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testZenHidesChromeAndTabRestoresIt() {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"zen_mode"}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        XCTAssertTrue(canvas.waitForExistence(timeout: 30))
        XCTAssertTrue(app.buttons["panel-tab-sizes"].waitForNonExistence(timeout: 5))
        XCTAssertFalse(editorDocumentTitle(in: app).exists)
        XCTAssertTrue(app.descendants(matching: .any)["zen-button"].firstMatch.waitForExistence(timeout: 5),
            "Zen keeps the standalone Capy by default")
        app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [])
        XCTAssertTrue(app.buttons["panel-tab-sizes"].waitForExistence(timeout: 10))
        XCTAssertTrue(editorDocumentTitle(in: app).exists)
        XCTAssertTrue(app.buttons["zen-button"].exists)
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func testCollapsedColumnsDrawersAndZen() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"move_panel","panel":"toolbar","target":{"kind":"tab","group":6},"viewport":[1376,1032]},{"type":"customize","action":{"type":"set_column_collapsed","group":6,"collapsed":true}}]"#
        app.launch()
        checkCollapsedColumnsDrawersAndZen(in: app)
    }

    @MainActor func testToolbarCustomization() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"new_toolbar"}]"#
        app.launch()
        checkToolbarCustomization(in: app)
    }

    @MainActor func testLayerConfiguration() throws {
        checkLayerConfiguration(in: editorTestApplication())
    }

    @MainActor func testPanelConfigurationAndLiveDrag() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"customize","action":{"type":"show_all_controls","panel":"sizes"}}]"#
        app.launch()
        checkPanelConfigurationAndLiveDrag(in: app)
    }

    @MainActor func testNavigatorAndDiagnostics() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkNavigatorAndDiagnostics(in: app)
    }

    @MainActor func testFilterArtworkAndHistory() throws { checkFilterArtworkAndHistory(in: ignoringSavedWindows(editorCaptureApplication())) }

    @MainActor func testFilterSearchPreviewAndProperties() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"customize","action":{"type":"set_panel_visible","panel":"adjustments","visible":true}}]"#
        app.launch()
        checkFilterSearchPreviewAndProperties(in: app)
    }

    @MainActor func testShortcutConflictAndEditorEffect() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"invoke","command":"keyboard_shortcuts"}]"#
        app.launch()
        checkShortcutConflictAndEditorEffect(in: app)
    }

    @MainActor func testNativeSDRCreationAndColorEditing() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"customize","action":{"type":"set_panel_visible","panel":"color","visible":true}},{"type":"color","action":{"op":"definition","color":{"space":"ProPhoto","rgba":[0.12345678,0.23456789,0.34567891,0.654321]}}},{"type":"invoke","command":"new_document"}]"#
        app.launch()
        checkNativeSDRCreationAndColorEditing(in: app)
    }

    @MainActor func testNewDrawingAndExportCancellation() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"invoke","command":"new_document"}]"#
        app.launch()
        checkNewDrawingAndExportCancellation(in: app)
    }

    @MainActor func testSettingsAndWorkspaceRestart() throws { checkSettingsAndWorkspaceRestart(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testColorControls() throws {
        let app = ignoringSavedWindows(editorCaptureApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"preferences","action":{"type":"edit","id":"theme","value":1}},{"type":"customize","action":{"type":"set_panel_visible","panel":"color","visible":true}},{"type":"set_color","rgba":[1,0,0,1]}]"#
        app.launchEnvironment["CAPY_COLOR_PROBE"] = "1"
        app.launch()
        checkColorControls(in: app) { mode, wheel, state in
            #if os(macOS)
            let window = app.windows.firstMatch
            attachColorFixture(name: "mac-color-" + mode, state: state,
                screenshot: window.screenshot(), viewport: window.frame, wheel: wheel)
            #else
            attachColorFixture(name: "ipad-color-" + mode, state: state,
                screenshot: XCUIScreen.main.screenshot(), viewport: app.frame, wheel: wheel)
            #endif
        }
    }

    @MainActor func testCompleteEditorCapture() {
        let app = ignoringSavedWindows(editorCaptureApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        capturePaintEditor(in: app)
    }

    @MainActor func testCompleteEditorDarkCapture() {
        let app = ignoringSavedWindows(editorCaptureApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        app.launch()
        capturePaintEditor(in: app, theme: "dark")
        for _ in 0..<4 { workspaceActivate(app.buttons["navigator-zoom_in"]) }
        capturePaintEditor(in: app, scenario: "paint-canvas-under-header", theme: "dark")
    }

    @MainActor func testBlendChoices() {
        let app = ignoringSavedWindows(editorCaptureApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkBlendChoices(in: app)
    }

    @MainActor func testInlineLayerOpacity() {
        let app = ignoringSavedWindows(editorCaptureApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkInlineLayerOpacity(in: app)
    }

    @MainActor func testEditorControlLayout() {
        let app = ignoringSavedWindows(editorCaptureApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkEditorControlLayout(in: app)
    }

    @MainActor func testNumericToolControls() throws {
        let app = ignoringSavedWindows(editorCaptureApplication())
        #if os(macOS)
        app.launchArguments += ["-AppleInterfaceStyle", "Light"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkNumericToolControls(in: app)
        #if os(macOS)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "mac-tool-controls"
        #else
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "ipad-tool-controls"
        #endif
        shot.lifetime = .keepAlways; add(shot)
    }

    @MainActor private func ignoringSavedWindows(_ app: XCUIApplication) -> XCUIApplication {
        #if os(macOS)
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        #endif
        return app
    }
}
