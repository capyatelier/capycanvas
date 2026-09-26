import XCTest

extension EditorLaunchTests {
    @MainActor func testNativeProfiledExport() throws {
        try checkNativeProfiledExport(in: editorCaptureApplication())
    }

    @MainActor func testNativePhotoCorrections() throws {
        try checkNativePhotoCorrections(in: editorCaptureApplication())
    }

    @MainActor func testNativeHistogram() throws {
        try checkNativeHistogram(in: editorCaptureApplication())
    }

    @MainActor func testNativeSourceEditing() throws {
        try checkNativeSourceEditing(in: editorCaptureApplication())
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

    @MainActor func testSavePanelKeepsItsDocumentAcrossWindowFocus() throws {
        try checkSavePanelKeepsItsDocumentAcrossWindowFocus(in: editorCaptureApplication())
    }

    @MainActor func testNativeImageImport() throws {
        try checkNativeImageImport(in: editorCaptureApplication())
    }

    @MainActor func testNativeImageDrop() throws {
        try checkNativeImageDrop(in: editorCaptureApplication())
    }

    @MainActor func testNativeImagePaste() throws {
        try checkNativeImagePaste(in: editorCaptureApplication())
    }

    @MainActor func testFullscreenEditor() { checkFullscreenEditor(in: editorCaptureApplication()) }

    @MainActor func testSDRWindowSurfaceTransitions() {
        checkSDRWindowSurfaceTransitions(in: editorCaptureApplication())
    }

    @MainActor func testNativeRegionRefinement() throws { try checkNativeRegionRefinement(in: editorCaptureApplication()) }

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
