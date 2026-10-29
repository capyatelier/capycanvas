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

    @MainActor func testNativeDrawingTabsDark() throws {
        try checkNativeDrawingTabs(in: editorCaptureApplication(), theme: "dark")
    }

    @MainActor func testNativeHDRColor() throws {
        try checkNativeHDRColor(in: editorCaptureApplication())
    }

    @MainActor func testNativeHDRColorLight() throws {
        try checkNativeHDRColor(in: editorCaptureApplication(), theme: "light")
    }

    @MainActor func testNativeFloatSourceCaption() async throws {
        try await checkNativeFloatSourceCaption(in: editorCaptureApplication(), theme: "light")
    }
    @MainActor func testNativeFloatSourceCaptionDark() async throws {
        try await checkNativeFloatSourceCaption(in: editorCaptureApplication(), theme: "dark")
    }

    @MainActor func testNativeDocumentColor() throws {
        try checkNativeDocumentColor(in: editorCaptureApplication())
    }

    @MainActor func testEditorKeyboardFocus() { checkEditorKeyboardFocus(in: editorCaptureApplication()) }
    @MainActor func testCommandSearch() { checkCommandSearch(in: ignoringSavedWindows(editorCaptureApplication())) }
    @MainActor func testCommandSearchFromMenu() { checkCommandSearch(in: ignoringSavedWindows(editorCaptureApplication()), fromMenu: true) }
    @MainActor func testNumericTextHistory() { checkNumericTextHistory(in: editorCaptureApplication()) }
    @MainActor func testNumericSettingsDone() { checkNumericSettingsDone(in: editorCaptureApplication()) }
    @MainActor func testNumericSettingsDoneDark() { checkNumericSettingsDone(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testSettingsNumericReset() { checkSettingsNumericReset(in: editorCaptureApplication()) }
    @MainActor func testPressureCalibrationLight() { checkPressureCalibration(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testPressureCalibrationDark() { checkPressureCalibration(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testTiltCalibrationLight() { checkTiltCalibration(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testTiltCalibrationDark() { checkTiltCalibration(in: editorCaptureApplication(), theme: "dark") }

    @MainActor func testSettingsTextState() {
        #if os(iOS)
        checkSettingsTextState(in: editorCaptureApplication(), keyboardSelection: false)
        #else
        checkSettingsTextState(in: editorCaptureApplication())
        #endif
    }

    @MainActor func testSettingsTextStateDark() {
        #if os(iOS)
        checkSettingsTextState(in: editorCaptureApplication(), theme: "dark", keyboardSelection: false)
        #else
        checkSettingsTextState(in: editorCaptureApplication(), theme: "dark")
        #endif
    }

    @MainActor func testSettingsChoicePresentation() { checkSettingsChoicePresentation(in: editorCaptureApplication()) }
    @MainActor func testSettingsDropdowns() { checkSettingsControls(in: editorCaptureApplication()) }

    @MainActor func testBlendAndLiquify() { checkBlendAndLiquify(in: editorCaptureApplication()) }
    @MainActor func testBlendAndLiquifyDark() { checkBlendAndLiquify(in: editorCaptureApplication(), theme: "dark") }

    @MainActor func testPaintingBrushes() { checkPaintingBrushes(in: editorCaptureApplication()) }
    @MainActor func testPaintingBrushesDark() { checkPaintingBrushes(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testDryMediaBrushes() { checkPaintingBrushes(in: editorCaptureApplication(), dryOnly: true) }
    @MainActor func testDryMediaBrushesDark() { checkPaintingBrushes(in: editorCaptureApplication(), theme: "dark", dryOnly: true) }
    @MainActor func testPenBrushOrder() { checkPenBrushOrder(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testPenBrushOrderDark() { checkPenBrushOrder(in: editorCaptureApplication(), theme: "dark") }

    @MainActor func testMaskTransforms() { checkMaskTransforms(in: editorCaptureApplication()) }
    @MainActor func testMaskTransformsDark() { checkMaskTransforms(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testMaskLinkControls() { checkMaskLinkControls(in: editorTestApplication(), theme: "light") }
    @MainActor func testMaskLinkControlsDark() { checkMaskLinkControls(in: editorTestApplication(), theme: "dark") }

    @MainActor func testMaskActionsAndHistory() { checkMaskActionsAndHistory(in: editorCaptureApplication()) }

    @MainActor func testLayerContentActionsAndHistory() { checkLayerContentActionsAndHistory(in: editorCaptureApplication()) }
    @MainActor func testAddPaper() { checkAddPaper(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testAddPaperDark() { checkAddPaper(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testLayerPropertiesDrawer() { checkLayerPropertiesDrawer(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testLayerPropertiesDrawerDark() { checkLayerPropertiesDrawer(in: editorCaptureApplication(), theme: "dark") }

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
    @MainActor func testToolbarComponentsDark() { checkToolbarComponents(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testStrengthAndSmoothing() { checkStrengthAndSmoothing(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testStrengthAndSmoothingDark() { checkStrengthAndSmoothing(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testColorPicker() { checkColorPicker(in: editorCaptureApplication()) }
    @MainActor func testColorPickerDark() { checkColorPicker(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testLiveColorTab() { checkLiveColorTab(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testLiveColorTabDark() { checkLiveColorTab(in: editorCaptureApplication(), theme: "dark") }
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
    @MainActor func testAboutAndApplicationMenusDark() { checkAboutAndApplicationMenus(in: editorCaptureApplication(), theme: "dark") }

    @MainActor func testApplicationLinkHandoff() { checkApplicationLinkHandoff(in: editorCaptureApplication()) }

    @MainActor func testSelectionAndTransform() { checkSelectionAndTransform(in: editorCaptureApplication()) }
    @MainActor func testSelectionAndTransformDark() { checkSelectionAndTransform(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testTransformToolAcceptance() { checkTransformToolAcceptance(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testTransformToolAcceptanceDark() { checkTransformToolAcceptance(in: editorCaptureApplication(), theme: "dark") }

    @MainActor func testCanvasSize() { checkCanvasSize(in: ignoringSavedWindows(editorCaptureApplication())) }
    @MainActor func testCanvasSizeLight() { checkCanvasSize(in: ignoringSavedWindows(editorCaptureApplication()), theme: "light") }
    @MainActor func testImageSize() { checkImageSize(in: ignoringSavedWindows(editorCaptureApplication())) }
    @MainActor func testImageSizeLight() { checkImageSize(in: ignoringSavedWindows(editorCaptureApplication()), theme: "light") }
    @MainActor func testFrequencySeparation() { checkFrequencySeparation(in: editorCaptureApplication()) }
    @MainActor func testPixelClipboard() throws { try checkPixelClipboard(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testPixelClipboardDark() throws { try checkPixelClipboard(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testImageObjects() throws { try checkImageObjects(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testImageObjectsDark() throws { try checkImageObjects(in: editorCaptureApplication(), theme: "dark") }
    #if os(macOS)
    @MainActor func testVectorKeyboard() throws { try checkVectorKeyboard(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testVectorKeyboardDark() throws { try checkVectorKeyboard(in: editorCaptureApplication(), theme: "dark") }
    #endif
    @MainActor func testReleaseEscapeOrder() { checkReleaseEscapeOrder(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testReleaseEscapeOrderDark() { checkReleaseEscapeOrder(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testReleaseToolbarBubbles() { checkReleaseToolbarBubbles(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testReleaseToolbarBubblesDark() { checkReleaseToolbarBubbles(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testReleaseToolbarZen() { checkReleaseToolbarZen(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testReleaseToolbarZenDark() { checkReleaseToolbarZen(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testVectorLayers() { checkVectorLayers(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testVectorLayersDark() { checkVectorLayers(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testLiveInterfaceLanguage() { checkLiveInterfaceLanguage(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testLiveInterfaceLanguageDark() { checkLiveInterfaceLanguage(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testCanvasActionBar() { checkCanvasActionBar(in: ignoringSavedWindows(editorCaptureApplication())) }
    @MainActor func testCanvasActionBarDark() { checkCanvasActionBar(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark") }
    @MainActor func testCanvasNotice() throws {
        #if os(iOS)
        throw XCTSkip("A finger tap does not run the Wand on iPadOS, and XCTest cannot synthesize Pencil")
        #else
        checkCanvasNotice(in: ignoringSavedWindows(editorCaptureApplication()))
        #endif
    }
    @MainActor func testZoomReadout() { checkZoomReadout(in: ignoringSavedWindows(editorCaptureApplication())) }
    @MainActor func testZoomReadoutDark() { checkZoomReadout(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark") }
    @MainActor func testZoomTool() throws { try checkZoomTool(in: ignoringSavedWindows(editorCaptureApplication()), theme: "light") }
    @MainActor func testZoomToolDark() throws { try checkZoomTool(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark") }
    @MainActor func testCanvasNavigationControls() throws { try checkCanvasNavigationControls(in: ignoringSavedWindows(editorCaptureApplication()), theme: "light") }
    @MainActor func testCanvasNavigationControlsDark() throws { try checkCanvasNavigationControls(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark") }
    @MainActor func testNavigationDrawingCycle() throws { try checkNavigationDrawingCycle(in: ignoringSavedWindows(editorCaptureApplication()), theme: "light") }
    @MainActor func testNavigationDrawingCycleDark() throws { try checkNavigationDrawingCycle(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark") }

    @MainActor func testShortcutSettingsPage() { checkShortcutSettingsPage(in: ignoringSavedWindows(editorCaptureApplication())) }

    @MainActor func testTouchGestureTaps() { checkTouchGestureTaps(in: ignoringSavedWindows(editorCaptureApplication())) }

    @MainActor func testPopupThemeFollowsExplicitAndSystem() { checkPopupThemeFollowsExplicitAndSystem() }

    @MainActor func testPhotoScopes() { checkPhotoScopes(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testPhotoScopesDark() { checkPhotoScopes(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testToolGroups() { checkToolGroups(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testToolGroupsDark() { checkToolGroups(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testToolBubbles() { checkToolBubbles(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testToolBubblesDark() { checkToolBubbles(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testEncloseFillTool() { checkEncloseFillTool(in: editorCaptureApplication()) }
    @MainActor func testEncloseFillToolDark() { checkEncloseFillTool(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testLayerColorModes() { checkLayerColorModesAndFilters(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testLayerColorModesDark() { checkLayerColorModesAndFilters(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testAlphaConversionFilters() { checkAlphaConversionFilters(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testAlphaConversionFiltersDark() { checkAlphaConversionFilters(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testEditColor() { checkEditColor(in: ignoringSavedWindows(editorCaptureApplication()), theme: "light") }
    @MainActor func testEditColorDark() { checkEditColor(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark") }
    @MainActor func testFillThumbnailColor() { checkFillThumbnailColor(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testFillThumbnailColorDark() { checkFillThumbnailColor(in: editorCaptureApplication(), theme: "dark") }
    @MainActor func testSwitcherVisibility() { checkSwitcherVisibility(in: editorTestApplication()) }
    @MainActor func testWorkspaceSwitcher() {
        checkWorkspaceSwitcher(in: editorTestApplication())
    }
    @MainActor func testWorkspaceSwitcherDark() {
        checkWorkspaceSwitcher(in: editorTestApplication(), theme: "dark")
    }

    @MainActor func testWorkspaceInlineNameAdmission() {
        checkWorkspaceInlineNameAdmission(in: editorTestApplication(), theme: "light")
    }

    @MainActor func testWorkspaceInlineNameAdmissionDark() {
        checkWorkspaceInlineNameAdmission(in: editorTestApplication(), theme: "dark")
    }

    @MainActor func testWorkspaceLibraryHistory() {
        checkWorkspaceLibraryHistory(in: editorTestApplication())
    }

    @MainActor func testDrawerDragAndDock() { checkDrawerDragAndDock(in: ignoringSavedWindows(editorTestApplication())) }
    @MainActor func testDrawerDragAndDockDark() { checkDrawerDragAndDock(in: ignoringSavedWindows(editorTestApplication()), theme: "dark") }

    @MainActor func testToolbarStylesAndActions() { checkToolbarStylesAndActions(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testTitleBarCustomization() { checkTitleBarCustomization(in: editorTestApplication()) }

    @MainActor func testTitleBarToolDrawers() { checkTitleBarToolDrawers(in: ignoringSavedWindows(editorCaptureApplication())) }
    @MainActor func testTitleBarToolDrawersDark() { checkTitleBarToolDrawers(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark") }
    @MainActor func testTitleBarDrawerBridges() { checkTitleBarToolDrawers(in: ignoringSavedWindows(editorCaptureApplication()), bridgesOnly: true) }
    @MainActor func testTitleBarDrawerBridgesDark() { checkTitleBarToolDrawers(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark", bridgesOnly: true) }

    @MainActor func testOrdinaryPaperLibrary() async throws {
        try await checkOrdinaryPaperLibrary(in:editorCaptureApplication(),theme:"light")
    }
    @MainActor func testOrdinaryPaperLibraryDark() async throws {
        try await checkOrdinaryPaperLibrary(in:editorCaptureApplication(),theme:"dark")
    }

    @MainActor func testColumnWidthReset() { checkColumnWidthReset(in: ignoringSavedWindows(editorTestApplication()), theme: "light") }
    @MainActor func testColumnWidthResetDark() { checkColumnWidthReset(in: ignoringSavedWindows(editorTestApplication()), theme: "dark") }
    @MainActor func testColumnStacks() { checkColumnStacks(in: editorTestApplication()) }

    @MainActor func testColumnStacksDark() { checkColumnStacks(in: editorTestApplication(), theme: "dark") }

    @MainActor func testPaintDefaultColumns() { checkDefaultWorkspaceColumns(in: editorTestApplication()) }

    @MainActor func testPhotoDefaultColumns() { checkDefaultWorkspaceColumns(in: editorTestApplication(), photo: true) }

    @MainActor func testRendererRecovery() { checkRendererRecovery(in: editorTestApplication()) }
    @MainActor func testRendererRecoveryDark() { checkRendererRecovery(in: editorTestApplication(), theme: "dark") }
    @MainActor func testRendererDiagnosticPrecedence() { checkRendererDiagnosticPrecedence(in: editorTestApplication()) }
    @MainActor func testRendererDiagnosticPrecedenceDark() { checkRendererDiagnosticPrecedence(in: editorTestApplication(), theme: "dark") }

    @MainActor func testTitleBarSystemStatus() { checkTitleBarSystemStatus(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testIndependentEditorWindows() { checkIndependentEditorWindows(in: ignoringSavedWindows(editorTestApplication())) }

    @MainActor func testArtworkRecoveryAfterRestart() { checkArtworkRecoveryAfterRestart(in: ignoringSavedWindows(editorTestApplication())) }
    @MainActor func testArtworkRecoveryAfterRestartDark() { checkArtworkRecoveryAfterRestart(in: ignoringSavedWindows(editorTestApplication()), theme: "dark") }

    @MainActor func testZenHidesChromeAndTabRestoresIt() {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"workspace_manager","command":{"type":"switch","id":"builtin:workspace:illustrator"}},{"type":"invoke","command":"zen_mode"}]"#
        app.launch()
        let canvas = app.descendants(matching: .any)["canvas"].firstMatch
        expectation(for: NSPredicate(format: "value == %@", "Canvas ready"), evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        XCTAssertTrue(app.buttons["panel-tab-sizes"].waitForNonExistence(timeout: 5))
        XCTAssertFalse(editorDocumentTitle(in: app).exists)
        XCTAssertTrue(app.descendants(matching: .any)["zen-button"].firstMatch.waitForExistence(timeout: 5),
            "Zen keeps the standalone Capy by default")
        app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [])
        XCTAssertTrue(app.buttons["panel-tab-sizes"].waitForExistence(timeout: 10))
        XCTAssertTrue(editorDocumentTitle(in: app).exists)
        XCTAssertTrue(app.buttons["zen-button"].exists)
        let paint = app.buttons["workspace-switch-builtin:workspace:illustrator"]
        XCTAssertTrue(paint.exists && paint.isEnabled && paint.isHittable)
        workspaceActivate(app.buttons["zen-button"])
        expectation(for: NSPredicate { _, _ in !paint.exists || !paint.isEnabled }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        attachEditor(in: app, name: "zen-live-hidden")
        workspaceActivate(app.buttons["zen-button"])
        expectation(for: NSPredicate { _, _ in paint.exists && paint.isEnabled && paint.isHittable }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
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

    @MainActor func testPanelConfigurationAndLiveDrag() throws { panelConfiguration(theme: "light") }
    @MainActor func testPanelConfigurationAndLiveDragDark() throws { panelConfiguration(theme: "dark") }
    @MainActor private func panelConfiguration(theme: String) {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"customize","action":{"type":"set_panel_visible","panel":"sizes","visible":true}},{"type":"customize","action":{"type":"show_all_controls","panel":"sizes"}}]"#
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
    @MainActor func testFilterArtworkAndHistoryDark() throws { checkFilterArtworkAndHistory(in: ignoringSavedWindows(editorCaptureApplication()), theme: "dark") }

    @MainActor func testFilterSearchPreviewAndProperties() throws { checkFilterSearchPreviewAndProperties(in: ignoringSavedWindows(editorTestApplication())) }
    @MainActor func testFilterSearchPreviewAndPropertiesDark() throws { checkFilterSearchPreviewAndProperties(in: ignoringSavedWindows(editorTestApplication()), theme: "dark") }

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
        #if os(iOS)
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        #else
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"new_document"}]"#
        #endif
        app.launch()
        checkNewDrawingAndExportCancellation(in: app)
    }

    @MainActor func testNewDrawingAndExportCancellationDark() throws {
        let app = ignoringSavedWindows(editorTestApplication())
        #if os(iOS)
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"}]"#
        #else
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"dark"},{"type":"invoke","command":"new_document"}]"#
        #endif
        app.launch()
        checkNewDrawingAndExportCancellation(in: app)
    }

    @MainActor func testNewDrawingDialog() throws { newDrawingDialog(theme: "light") }
    @MainActor func testNewDrawingDialogDark() throws { newDrawingDialog(theme: "dark") }
    @MainActor func testNewDrawingKeyboardHeight() { newDrawingDialog(theme: "light", keyboardOnly: true) }
    @MainActor func testNewDrawingKeyboardHeightDark() { newDrawingDialog(theme: "dark", keyboardOnly: true) }
    @MainActor private func newDrawingDialog(theme: String, keyboardOnly: Bool = false) {
        let app = ignoringSavedWindows(editorTestApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"\#(theme)"},{"type":"invoke","command":"new_document"}]"#
        app.launch()
        checkNewDrawingDialog(in: app, theme: theme, keyboardOnly: keyboardOnly)
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

    @MainActor func testPaintColorMenu() { checkPaintColorMenu(in: ignoringSavedWindows(editorTestApplication()), theme: "light") }
    @MainActor func testPaintColorMenuDark() { checkPaintColorMenu(in: ignoringSavedWindows(editorTestApplication()), theme: "dark") }

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

    @MainActor func testInlineLayerOpacity() { runInlineLayerOpacity(theme: "light") }

    @MainActor func testInlineLayerOpacityDark() { runInlineLayerOpacity(theme: "dark") }

    @MainActor private func runInlineLayerOpacity(theme: String) {
        let app = ignoringSavedWindows(editorCaptureApplication())
        #if os(macOS)
        app.launchArguments += ["-AppleInterfaceStyle", theme == "dark" ? "Dark" : "Light"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = "[{\"type\":\"set_theme\",\"theme\":\"\(theme)\"}]"
        app.launch()
        checkInlineLayerOpacity(in: app, theme: theme)
    }

    @MainActor func testEditorControlLayout() {
        let app = ignoringSavedWindows(editorCaptureApplication())
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkEditorControlLayout(in: app)
    }

    @MainActor func testNumericToolControls() throws { try runNumericToolControls(theme: "light") }

    @MainActor func testNumericToolControlsDark() throws { try runNumericToolControls(theme: "dark") }

    @MainActor private func runNumericToolControls(theme: String) throws {
        let app = ignoringSavedWindows(editorCaptureApplication())
        #if os(macOS)
        app.launchArguments += ["-AppleInterfaceStyle", theme == "dark" ? "Dark" : "Light"]
        #endif
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = "[{\"type\":\"set_theme\",\"theme\":\"\(theme)\"}]"
        app.launch()
        checkNumericToolControls(in: app, theme: theme)
        #if os(macOS)
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "mac-tool-controls-" + theme
        #else
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "ipad-tool-controls-" + theme
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

extension EditorLaunchTests {
    @MainActor func testRememberedPaperColdStart() async throws {
        try await checkRememberedPaperColdStart(in:ignoringSavedWindows(editorCaptureApplication()),theme:"light")
    }
    @MainActor func testRememberedPaperColdStartDark() async throws {
        try await checkRememberedPaperColdStart(in:ignoringSavedWindows(editorCaptureApplication()),theme:"dark")
    }
}


extension EditorLaunchTests {
    @MainActor func testPreservedPackageCopyOriginal() async throws { try await checkPreservedPackageCopyOriginal(in: editorCaptureApplication(), theme: "light") }
    @MainActor func testPreservedPackageCopyOriginalDark() async throws { try await checkPreservedPackageCopyOriginal(in: editorCaptureApplication(), theme: "dark") }
}
