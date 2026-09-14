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

    @MainActor func testLayerMenuDragUpward() throws {
        XCUIDevice.shared.orientation = .landscapeLeft
        let app = editorTestApplication()
        app.launchEnvironment["CAPY_ROW_MENU_PROBE"] = "1"
        app.launchEnvironment["CAPY_LAYER_INPUT_PROBE"] = "1"
        let actions = (0..<10).map { _ in ["type": "layer", "action": ["op": "new", "group": false, "clipped": false]] as [String: Any] }
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = String(data: try JSONSerialization.data(withJSONObject: actions), encoding: .utf8)
        app.launch()
        let list = app.descendants(matching: .any)["layer-rows"].firstMatch
        XCTAssertTrue(list.waitForExistence(timeout: 20))
        func order() -> [String] { (list.value as? String ?? "").split(separator: ",").map(String.init) }
        expectation(for: NSPredicate { _, _ in order().count == 12 }, evaluatedWith: app)
        waitForExpectations(timeout: 10)
        let initial = order(), movedID = initial[6]
        let row = app.otherElements["layer-row-" + movedID]
        XCTAssertTrue(row.isHittable)
        let destination = list.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0)).withOffset(CGVector(dx: 0, dy: 8))
        row.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.5)).press(forDuration: 0.8,
            thenDragTo: destination, withVelocity: .slow, thenHoldForDuration: 0.3)
        expectation(for: NSPredicate { _, _ in order().first == movedID }, evaluatedWith: app)
        waitForExpectations(timeout: 5)
        let moved = order()
        XCTAssertEqual(moved.filter { $0 != movedID }, initial.filter { $0 != movedID })
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

    @MainActor func testTitleBarCustomization() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkTitleBarCustomization(in: editorTestApplication())
    }

    @MainActor func testTitleBarToolDrawers() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkTitleBarToolDrawers(in: editorCaptureApplication())
    }

    @MainActor func testColumnStacks() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkColumnStacks(in: editorTestApplication())
    }

    @MainActor func testColumnStacksDark() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkColumnStacks(in: editorTestApplication(), theme: "dark")
    }

    @MainActor func testRendererRecovery() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkRendererRecovery(in: editorTestApplication())
    }

    @MainActor func testPaintDefaultColumns() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkPaintDefaultColumns(in: editorTestApplication())
    }

    @MainActor func testTitleBarSystemStatus() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkTitleBarSystemStatus(in: editorTestApplication())
    }

    @MainActor func testIndependentEditorWindows() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkIndependentEditorWindows(in: editorTestApplication())
    }

    @MainActor func testArtworkRecoveryAfterRestart() {
        XCUIDevice.shared.orientation = .landscapeLeft
        checkArtworkRecoveryAfterRestart(in: editorTestApplication())
    }



    @MainActor func testZenHidesChromeAndTabRestoresIt() {
        let app = editorTestApplication()
        XCUIDevice.shared.orientation = .landscapeLeft
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"zen_mode"}]"#
        app.launch()
        let canvas = app.otherElements["canvas"]
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
            attachColorFixture(name: "ipad-color-" + mode, state: state,
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

// Opt-in native provider acceptance; each run owns a new UUID-named folder.
extension EditorLaunchTests {
    @MainActor private func nativeFilesTestFolder() throws -> String {
        guard let token = ProcessInfo.processInfo.environment["CAPY_FILE_TEST_TOKEN"] else {
            throw XCTSkip("Native Files acceptance requires an isolated CAPY_FILE_TEST_TOKEN")
        }
        _ = try XCTUnwrap(UUID(uuidString: token))
        return "Capy Files " + token
    }

    @MainActor func testNativeFilesProjectRoundTrip() throws {
        let folder = try nativeFilesTestFolder()
        let app = editorTestApplication()
        XCUIDevice.shared.orientation = .landscapeLeft
        app.launchEnvironment["CAPY_INITIAL_ACTIONS"] = #"[{"type":"set_theme","theme":"light"},{"type":"invoke","command":"add_layer"},{"type":"invoke","command":"select_all"},{"type":"invoke","command":"fill_selection"},{"type":"invoke","command":"deselect"}]"#
        func capture(_ name: String) {
            let tree = XCTAttachment(string: app.debugDescription)
            tree.name = name + "-hierarchy"; tree.lifetime = .keepAlways; add(tree)
            let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            shot.name = name; shot.lifetime = .keepAlways; add(shot)
        }
        func command(_ id: String) {
            workspaceActivate(app.buttons["menu-File"])
            workspaceActivate(app.buttons["command-" + id])
        }
        func replace(_ field: XCUIElement, _ value: String) {
            workspaceActivate(field)
            field.typeKey("a", modifierFlags: .command)
            field.typeText(value)
        }
        func folderTitle(_ label: String) -> XCUIElement {
            app.navigationBars.descendants(matching: .any).matching(
                NSPredicate(format: "label == %@ OR label == %@", label, label + ", Actions Menu")).firstMatch
        }
        func location() {
            workspaceActivate(app.cells["DOC.sidebar.item.On My iPad"])
            XCTAssertTrue(folderTitle("On My iPad").waitForExistence(timeout: 10))
        }
        func enterTestFolder() {
            location()
            workspaceActivate(app.cells[folder + ", Folder"])
            XCTAssertTrue(folderTitle(folder).waitForExistence(timeout: 10))
        }
        let title = app.staticTexts["document-title"]
        let rows = app.otherElements.matching(NSPredicate(format: "identifier BEGINSWITH %@", "layer-row-"))
        let save = app.buttons["DOCPicker.actionButton"]
        let name = app.textFields["DOCPicker.filenameTextField"]
        app.launch()
        XCTAssertTrue(title.waitForExistence(timeout: 30))
        expectation(for: NSPredicate { _, _ in rows.count == 3 }, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        command("save_document_as")
        XCTAssertTrue(name.waitForExistence(timeout: 20))
        location()
        XCTAssertFalse(app.cells[folder + ", Folder"].exists, "Each native run must create a fresh test folder")
        workspaceActivate(app.buttons["New Folder"])
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 10))
        app.typeKey("a", modifierFlags: .command)
        app.typeText(folder + "\n")
        XCTAssertTrue(folderTitle(folder).waitForExistence(timeout: 10))
        replace(name, "RoundTrip")
        XCTAssertEqual(name.value as? String, "RoundTrip")
        workspaceActivate(save)
        XCTAssertTrue(save.waitForNonExistence(timeout: 20))
        XCTAssertEqual(app.state, .runningForeground)
        expectation(for: NSPredicate(format: "label CONTAINS %@", "RoundTrip"), evaluatedWith: title)
        waitForExpectations(timeout: 30)
        XCTAssertEqual(rows.count, 3)
        XCTAssertFalse(app.alerts.firstMatch.exists)
        capture("project-saved")
        app.terminate()
        app.launchEnvironment.removeValue(forKey: "CAPY_INITIAL_ACTIONS")
        app.launch()
        XCTAssertTrue(title.waitForExistence(timeout: 30))
        expectation(for: NSPredicate { _, _ in rows.count == 2 }, evaluatedWith: app)
        waitForExpectations(timeout: 30)
        command("open_document")
        enterTestFolder()
        let project = app.cells.matching(NSPredicate(format: "identifier BEGINSWITH %@", "RoundTrip.capy,")).firstMatch.images.firstMatch
        workspaceActivate(project)
        expectation(for: NSPredicate(format: "label CONTAINS %@", "RoundTrip"), evaluatedWith: title)
        waitForExpectations(timeout: 30)
        XCTAssertEqual(rows.count, 3, "The saved layer structure must survive a new process and native Open")
        XCTAssertFalse(app.alerts.firstMatch.exists)
        capture("project-reopened")
        command("export_document")
        XCTAssertTrue(name.waitForExistence(timeout: 20))
        enterTestFolder()
        replace(name, "Render")
        workspaceActivate(save)
        XCTAssertTrue(save.waitForNonExistence(timeout: 20))
        XCTAssertEqual(app.state, .runningForeground)
        XCTAssertTrue(title.label.contains("RoundTrip"), "PNG delivery must retain the editable project location")
        command("open_document")
        enterTestFolder()
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label == %@ OR label == %@", "Render", "Render.png")).firstMatch.waitForExistence(timeout: 15))
        capture("delivered-project-and-png")
        workspaceActivate(app.buttons["Cancel"].firstMatch)
        XCTAssertTrue(title.label.contains("RoundTrip"))
        XCTAssertFalse(app.staticTexts["Canvas error"].exists)
    }

    @MainActor func testNativeFilesProjectRoundTripCleanup() throws {
        let folder = try nativeFilesTestFolder()
        let files = XCUIApplication(bundleIdentifier: "com.apple.DocumentsApp")
        XCUIDevice.shared.orientation = .landscapeLeft
        files.activate()
        workspaceActivate(files.cells["DOC.sidebar.item.On My iPad"])
        let cell = files.cells[folder + ", Folder"]
        XCTAssertTrue(cell.waitForExistence(timeout: 10))
        guard cell.staticTexts.matching(NSPredicate(format: "label ENDSWITH %@", " - 2 items"))
            .firstMatch.waitForExistence(timeout: 5) else {
            XCTFail("Only the generated project and PNG may be present")
            return
        }
        cell.press(forDuration: 0.8)
        workspaceActivate(files.buttons["Delete"].firstMatch)
        XCTAssertTrue(cell.waitForNonExistence(timeout: 10))
    }
}
