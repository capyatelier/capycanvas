import XCTest

final class EditorLaunchTests: XCTestCase {
    override func setUpWithError() throws { continueAfterFailure = false }

    @MainActor func testNativeWorkspaceContextAction() { checkNativeWorkspaceContextAction() }

    @MainActor func testPopupThemeFollowsExplicitAndSystem() { checkPopupThemeFollowsExplicitAndSystem() }

    @MainActor func testLayerGripAndChildActions() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launch()
        let add = app.buttons["layer-New layer"]
        XCTAssertTrue(add.waitForExistence(timeout: 20))
        add.tap()
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let addedID = rows.element(boundBy: 0).identifier
        let originalID = rows.element(boundBy: 1).identifier
        let original = app.otherElements[originalID]
        original.buttons["Select layer without changing drawing target"].tap()
        XCTAssertTrue(original.buttons["Select layer without changing drawing target"].isSelected)
        XCTAssertTrue(app.otherElements[addedID].buttons["Edit layer content"].isSelected)
        let grip = app.descendants(matching: .any)[addedID.replacingOccurrences(of: "layer-row-", with: "layer-grip-")].firstMatch
        grip.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.01,
            thenDragTo: original.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 0.85)))
        expectation(for: NSPredicate { _, _ in rows.element(boundBy: 0).identifier == originalID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        func command(_ label: String) {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-commands-", label)).firstMatch.tap()
        }
        command("Undo")
        expectation(for: NSPredicate { _, _ in rows.element(boundBy: 0).identifier == addedID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        command("Redo")
        expectation(for: NSPredicate { _, _ in rows.element(boundBy: 0).identifier == originalID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
    }

    @MainActor func testLayerListScrolling() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_LAYER_INPUT_PROBE"] = "1"
        let actions = (0..<22).map { _ in ["type": "layer", "action": ["op": "new", "group": false, "clipped": false]] as [String: Any] }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let list = app.descendants(matching: .any)["layer-rows"].firstMatch
        XCTAssertTrue(list.waitForExistence(timeout: 20))
        func order() -> [String] { (list.value as? String ?? "").split(separator: ",").map(String.init) }
        expectation(for: NSPredicate { _, _ in order().count == 24 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let initial = order(), movedID = initial[0]
        let first = app.descendants(matching: .any)["layer-grip-" + movedID].firstMatch
        let initialFrame = first.frame
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.8)).press(forDuration: 0.01,
            thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.2)))
        XCTAssertTrue(!first.exists || first.frame.maxY < initialFrame.maxY - 20,
            "Early touch movement must scroll the rows")
        XCTAssertEqual(order(), initial, "Early touch scrolling must preserve document order")
        list.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.15)).press(forDuration: 0.01,
            thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.9)))
        XCTAssertTrue(first.waitForExistence(timeout: 5))
        XCTAssertTrue(first.isHittable, "The source grip must be back inside the viewport")
        let edge = list.coordinate(withNormalizedOffset: CGVector(dx: 0.8, dy: 1)).withOffset(CGVector(dx: 0, dy: -10))
        first.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.01,
            thenDragTo: edge, withVelocity: .slow, thenHoldForDuration: 1.2)
        expectation(for: NSPredicate { _, _ in order() != initial }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let moved = order()
        XCTAssertEqual(moved.filter { $0 != movedID }, initial.filter { $0 != movedID })
        XCTAssertGreaterThan(moved.firstIndex(of: movedID) ?? -1, Int(list.frame.height / 40),
            "Holding at the edge must move the source beyond the initial visible rows")
        func command(_ label: String) {
            app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@ AND label == %@",
                "toolbar-tile-commands-", label)).firstMatch.tap()
        }
        command("Undo")
        expectation(for: NSPredicate { _, _ in order() == initial }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        command("Redo")
        expectation(for: NSPredicate { _, _ in order() == moved }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
    }

    @MainActor func testLayerContextMenuAnchors() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launch()
        XCTAssertTrue(app.buttons["layer-New layer"].waitForExistence(timeout: 20))
        checkLayerControls(in: app)
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let row = rows.element(boundBy: 1)
        row.buttons["Edit layer content"].press(forDuration: 0.6)
        let nativeMenuItem = app.buttons["Rename layer…"]
        XCTAssertTrue(nativeMenuItem.waitForExistence(timeout: 5))
        attachLayerMenu("layer-content-context-anchor")
        // Dismiss through the app canvas, then check the separate footer origin.
        app.otherElements["canvas"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        XCTAssertTrue(nativeMenuItem.waitForNonExistence(timeout: 5))
        app.buttons["layer-Layer actions"].tap()
        let menu = app.descendants(matching: .any)["layer-context-menu"].firstMatch
        XCTAssertTrue(menu.waitForExistence(timeout: 5))
        attachLayerMenu("layer-footer-context-anchor")
    }

    @MainActor private func attachLayerMenu(_ name: String) {
        let capture = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        capture.name = name; capture.lifetime = .keepAlways; add(capture)
    }

    @MainActor func testWorkspaceSwitcher() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkWorkspaceSwitcher(in: editorTestApplication())
    }

    @MainActor func testWorkspaceLibraryHistory() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkWorkspaceLibraryHistory(in: editorTestApplication())
    }

    @MainActor func testDrawerDragAndDock() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkDrawerDragAndDock(in: editorTestApplication())
    }

    @MainActor func testToolbarStylesAndActions() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkToolbarStylesAndActions(in: editorTestApplication())
    }

    @MainActor func testSystemStatusSetting() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkSystemStatusSetting(in: editorTestApplication())
    }

    @MainActor func testIndependentEditorWindows() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkIndependentEditorWindows(in: editorTestApplication())
    }

    @MainActor func testArtworkRecoveryAfterRestart() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkArtworkRecoveryAfterRestart(in: editorTestApplication())
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
        XCTAssertTrue(app.frame.contains(section.frame))
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
        XCUIDevice.shared.orientation = .landscapeLeft
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
        XCUIDevice.shared.orientation = .landscapeLeft
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"invoke","command":"new_document"}]"#
        app.launch()
        checkNewDrawingAndExportCancellation(in: app)
    }

    @MainActor func testSettingsAndWorkspaceRestart() throws {
        let app = editorTestApplication()
        XCUIDevice.shared.orientation = .landscapeLeft
        checkSettingsAndWorkspaceRestart(in: app)
    }

    @MainActor func testColorControls() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"preferences","action":{"type":"edit","id":"theme","value":1}},{"type":"customize","action":{"type":"set_panel_visible","panel":"color","visible":true}},{"type":"set_color","rgba":[1,0,0,1]}]"#
        app.launchEnvironment["CAPY_COLOR_PROBE"] = "1"
        app.launch()
        checkColorControls(in: app) { mode, wheel, state in
            attachColorFixture(name: "ipad-color-" + mode, space: mode, state: state,
                screenshot: XCUIScreen.main.screenshot(), viewport: app.frame, wheel: wheel)
        }
    }

    @MainActor func testCompleteEditorCapture() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        captureDefaultEditor(in: app)
    }

    @MainActor func testBlendChoices() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkBlendChoices(in: app)
    }

    @MainActor func testInlineLayerOpacity() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkInlineLayerOpacity(in: app)
    }

    @MainActor func testEditorControlLayout() {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkEditorControlLayout(in: app)
    }

    @MainActor func testNumericToolControls() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorCaptureApplication()
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"}]"#
        app.launch()
        checkNumericToolControls(in: app)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "ipad-tool-controls"; shot.lifetime = .keepAlways; add(shot)
    }

    @MainActor func testMetalLaunchAndEditorCapture() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launch()
        let canvas = app.otherElements["canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 20))
        let ready = NSPredicate(format: "value == %@", "Metal ready")
        expectation(for: ready, evaluatedWith: canvas)
        waitForExpectations(timeout: 30)
        let zen = app.buttons["zen-button"]
        XCTAssertTrue(zen.exists)
        XCTAssertEqual(zen.frame.width, 36, accuracy: 1)
        XCTAssertEqual(zen.frame.height, 36, accuracy: 1)
        XCTAssertGreaterThan(app.frame.width, app.frame.height, "Landscape capture must use a settled landscape window")
        XCTAssertEqual(canvas.frame.width, app.frame.width, accuracy: 1)
        XCTAssertEqual(canvas.frame.height, app.frame.height, accuracy: 1)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "ipad-editor-initial"
        shot.lifetime = .keepAlways
        add(shot)
        checkLayerControls(in: app)
        let layers = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        layers.name = "ipad-layer-added"; layers.lifetime = .keepAlways; add(layers)
    }
}
